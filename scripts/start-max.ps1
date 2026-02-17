<#
.SYNOPSIS
    Start Kaizen MAX pipeline.

.DESCRIPTION
    Starts ZeroClaw core and UI processes under a Windows Job Object.
    If one process exits, the remaining processes are stopped.
    If the terminal closes, the Job Object ensures child processes are terminated.
    Use -InitEnv to create .env from .env.example when .env is missing.
#>

param(
    [switch]$CoreOnly,
    [switch]$UIOnly,
    [switch]$InitEnv,
    [string]$EnvFile = "$PSScriptRoot\..\.env"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($CoreOnly -and $UIOnly) {
    throw "CoreOnly and UIOnly cannot be used together."
}

if (-not ("KaizenMax.NativeJob" -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

namespace KaizenMax
{
    public static class NativeJob
    {
        [StructLayout(LayoutKind.Sequential)]
        public struct JOBOBJECT_BASIC_LIMIT_INFORMATION
        {
            public long PerProcessUserTimeLimit;
            public long PerJobUserTimeLimit;
            public uint LimitFlags;
            public UIntPtr MinimumWorkingSetSize;
            public UIntPtr MaximumWorkingSetSize;
            public uint ActiveProcessLimit;
            public IntPtr Affinity;
            public uint PriorityClass;
            public uint SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct IO_COUNTERS
        {
            public ulong ReadOperationCount;
            public ulong WriteOperationCount;
            public ulong OtherOperationCount;
            public ulong ReadTransferCount;
            public ulong WriteTransferCount;
            public ulong OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION
        {
            public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
            public IO_COUNTERS IoInfo;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern IntPtr CreateJobObject(IntPtr lpJobAttributes, string lpName);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool SetInformationJobObject(
            IntPtr hJob,
            int JobObjectInfoClass,
            IntPtr lpJobObjectInfo,
            uint cbJobObjectInfoLength
        );

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool AssignProcessToJobObject(IntPtr hJob, IntPtr hProcess);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool CloseHandle(IntPtr hObject);
    }
}
"@
}

function Ensure-EnvironmentFile {
    param(
        [string]$Path,
        [bool]$CreateIfMissing
    )

    if (Test-Path $Path) {
        return
    }

    if (-not $CreateIfMissing) {
        throw "No .env file found at $Path. Create it from .env.example or run with -InitEnv."
    }

    $parentDir = Split-Path -Parent $Path
    $examplePath = Join-Path $parentDir ".env.example"
    if (-not (Test-Path $examplePath)) {
        throw "No .env file found and .env.example is missing at $examplePath"
    }

    Copy-Item -Path $examplePath -Destination $Path
    Write-Host "[Kaizen MAX] Created .env from .env.example at $Path" -ForegroundColor Yellow
}

function Load-EnvironmentFile {
    param([string]$Path)

    Get-Content $Path | ForEach-Object {
        if ($_ -match '^\s*([^#][^=]+)=(.*)$') {
            [System.Environment]::SetEnvironmentVariable(
                $Matches[1].Trim(),
                $Matches[2].Trim(),
                "Process"
            )
        }
    }
}

function New-KillOnCloseJob {
    $jobName = "KaizenMAX_$PID"
    $jobHandle = [KaizenMax.NativeJob]::CreateJobObject([IntPtr]::Zero, $jobName)
    if ($jobHandle -eq [IntPtr]::Zero) {
        throw "Failed to create Job Object."
    }

    $killOnCloseFlag = 0x2000
    $extendedInfoClass = 9

    $jobInfo = New-Object KaizenMax.NativeJob+JOBOBJECT_EXTENDED_LIMIT_INFORMATION
    $jobInfo.BasicLimitInformation.LimitFlags = $killOnCloseFlag

    $jobInfoLength = [System.Runtime.InteropServices.Marshal]::SizeOf($jobInfo)
    $jobInfoPtr = [System.Runtime.InteropServices.Marshal]::AllocHGlobal($jobInfoLength)

    try {
        [System.Runtime.InteropServices.Marshal]::StructureToPtr($jobInfo, $jobInfoPtr, $false)
        $ok = [KaizenMax.NativeJob]::SetInformationJobObject(
            $jobHandle,
            $extendedInfoClass,
            $jobInfoPtr,
            [uint32]$jobInfoLength
        )
        if (-not $ok) {
            $lastError = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()
            throw "Failed to configure Job Object. Win32Error=$lastError"
        }
    } finally {
        [System.Runtime.InteropServices.Marshal]::FreeHGlobal($jobInfoPtr)
    }

    return $jobHandle
}

function Add-ProcessToJob {
    param(
        [IntPtr]$JobHandle,
        [System.Diagnostics.Process]$Process
    )

    $ok = [KaizenMax.NativeJob]::AssignProcessToJobObject($JobHandle, $Process.Handle)
    if (-not $ok) {
        $lastError = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "Failed to assign process $($Process.Id) to Job Object. Win32Error=$lastError"
    }
}

function Start-CommandProcess {
    param(
        [string]$Name,
        [string]$WorkingDirectory,
        [string]$Command,
        [IntPtr]$JobHandle
    )

    Write-Host "[Kaizen MAX] Starting ${Name}: $Command" -ForegroundColor Green
    $process = Start-Process -FilePath "cmd.exe" -ArgumentList "/d", "/c", $Command -WorkingDirectory $WorkingDirectory -PassThru -NoNewWindow

    try {
        Add-ProcessToJob -JobHandle $JobHandle -Process $process
    } catch {
        Stop-ProcessTree -Process $process -Name $Name
        throw
    }

    return $process
}

function Start-ExecutableProcess {
    param(
        [string]$Name,
        [string]$ExecutablePath,
        [string]$WorkingDirectory,
        [IntPtr]$JobHandle
    )

    Write-Host "[Kaizen MAX] Starting ${Name}: $ExecutablePath" -ForegroundColor Green
    $process = Start-Process -FilePath $ExecutablePath -WorkingDirectory $WorkingDirectory -PassThru

    try {
        Add-ProcessToJob -JobHandle $JobHandle -Process $process
    } catch {
        Stop-ProcessTree -Process $process -Name $Name
        throw
    }

    return $process
}

function Stop-ProcessTree {
    param(
        [System.Diagnostics.Process]$Process,
        [string]$Name
    )

    if ($null -eq $Process) {
        return
    }

    try {
        if ($Process.HasExited) {
            return
        }
    } catch {
        return
    }

    Write-Host "[Kaizen MAX] Stopping $Name (PID $($Process.Id))" -ForegroundColor Yellow
    & taskkill /PID $Process.Id /T /F *> $null
}

Ensure-EnvironmentFile -Path $EnvFile -CreateIfMissing:$InitEnv
Load-EnvironmentFile -Path $EnvFile
Write-Host "[Kaizen MAX] Loaded environment from $EnvFile" -ForegroundColor Cyan

$repoRoot = Resolve-Path "$PSScriptRoot\.."
$coreDir = Join-Path $repoRoot "core"
$uiDir = Join-Path $repoRoot "ui"
$jobHandle = New-KillOnCloseJob

$started = New-Object System.Collections.Generic.List[object]
$exitCode = 0

try {
    if (-not $UIOnly) {
        $coreExe = Join-Path $coreDir "target\release\zeroclaw-gateway.exe"
        if (Test-Path $coreExe) {
            $coreProcess = Start-ExecutableProcess -Name "ZeroClaw Core" -ExecutablePath $coreExe -WorkingDirectory $coreDir -JobHandle $jobHandle
        } else {
            Write-Host "[Kaizen MAX] Release core binary not found. Using cargo run." -ForegroundColor Yellow
            $coreProcess = Start-CommandProcess -Name "ZeroClaw Core" -WorkingDirectory $coreDir -Command "cargo run" -JobHandle $jobHandle
        }

        $started.Add([PSCustomObject]@{
            Name = "ZeroClaw Core"
            Process = $coreProcess
        })
    }

    if (-not $CoreOnly) {
        $desktopCandidates = @(
            (Join-Path $uiDir "desktop\KaizenMAX.exe"),
            (Join-Path $uiDir "dist\KaizenMAX.exe"),
            (Join-Path $uiDir "build\KaizenMAX.exe")
        )

        $desktopExe = $desktopCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1

        if ($desktopExe) {
            $uiProcess = Start-ExecutableProcess -Name "Kaizen MAX UI" -ExecutablePath $desktopExe -WorkingDirectory $uiDir -JobHandle $jobHandle
        } else {
            Write-Host "[Kaizen MAX] Packaged UI not found. Using npm run dev." -ForegroundColor Yellow
            $uiProcess = Start-CommandProcess -Name "Kaizen MAX UI" -WorkingDirectory $uiDir -Command "npm run dev" -JobHandle $jobHandle
        }

        $started.Add([PSCustomObject]@{
            Name = "Kaizen MAX UI"
            Process = $uiProcess
        })
    }

    if ($started.Count -eq 0) {
        throw "No processes were started."
    }

    if ($started.Count -eq 1) {
        $single = $started[0]
        Write-Host "[Kaizen MAX] Running $($single.Name). Press Ctrl+C to stop." -ForegroundColor Cyan
        $single.Process.WaitForExit()
        $exitCode = $single.Process.ExitCode
        Write-Host "[Kaizen MAX] $($single.Name) exited with code $exitCode" -ForegroundColor Yellow
    } else {
        Write-Host "[Kaizen MAX] Pipeline is running. If any process exits, all processes are stopped." -ForegroundColor Cyan
        $trigger = $null
        while ($null -eq $trigger) {
            foreach ($entry in $started) {
                if ($entry.Process.HasExited) {
                    $trigger = $entry
                    break
                }
            }

            if ($null -eq $trigger) {
                Start-Sleep -Milliseconds 400
            }
        }

        $exitCode = $trigger.Process.ExitCode
        Write-Host "[Kaizen MAX] $($trigger.Name) exited with code $exitCode. Stopping remaining processes." -ForegroundColor Yellow
    }
} finally {
    foreach ($entry in $started) {
        Stop-ProcessTree -Process $entry.Process -Name $entry.Name
    }

    if ($jobHandle -ne [IntPtr]::Zero) {
        [KaizenMax.NativeJob]::CloseHandle($jobHandle) | Out-Null
    }

    Write-Host "[Kaizen MAX] Pipeline stopped. No child processes left running." -ForegroundColor Cyan
}

exit $exitCode
