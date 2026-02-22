<#
.SYNOPSIS
    Start Kaizen MAX pipeline.

.DESCRIPTION
    Starts Kaizen core and Mission Control UI processes under a Windows Job Object.
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

function Start-ProcessWithNoWindowFallback {
    param(
        [hashtable]$StartArgs,
        [string]$Name,
        [bool]$PreferNoNewWindow
    )

    if ($PreferNoNewWindow) {
        $StartArgs["NoNewWindow"] = $true
    }

    try {
        return Start-Process @StartArgs
    } catch {
        $message = $_.Exception.Message
        $pipeClosed =
            ($message -match "(?i)0x800700e8") -or
            ($message -match "(?i)pipe is being closed")

        if ($PreferNoNewWindow -and $pipeClosed) {
            Write-Host "[Kaizen MAX] ${Name}: retrying without -NoNewWindow (detached console host)." -ForegroundColor Yellow
            $StartArgs.Remove("NoNewWindow") | Out-Null
            return Start-Process @StartArgs
        }

        throw
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
    $startArgs = @{
        FilePath = "cmd.exe"
        ArgumentList = @("/d", "/c", $Command)
        WorkingDirectory = $WorkingDirectory
        PassThru = $true
    }

    $process = Start-ProcessWithNoWindowFallback -StartArgs $startArgs -Name $Name -PreferNoNewWindow $true

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
        [IntPtr]$JobHandle,
        [switch]$NoNewWindow
    )

    Write-Host "[Kaizen MAX] Starting ${Name}: $ExecutablePath" -ForegroundColor Green
    $startArgs = @{
        FilePath = $ExecutablePath
        WorkingDirectory = $WorkingDirectory
        PassThru = $true
    }

    $process = Start-ProcessWithNoWindowFallback -StartArgs $startArgs -Name $Name -PreferNoNewWindow $NoNewWindow.IsPresent

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

function Stop-StaleKaizenProcesses {
    param([string]$RepoRoot)

    $repoRootLower = $RepoRoot.ToLowerInvariant()
    $selfPid = $PID
    $selfParentPid = 0

    $selfProc = Get-CimInstance Win32_Process -Filter "ProcessId = $selfPid" -ErrorAction SilentlyContinue
    if ($null -ne $selfProc) {
        $selfParentPid = [int]$selfProc.ParentProcessId
    }

    $stale = Get-CimInstance Win32_Process | Where-Object {
        if ($PSItem.ProcessId -eq $selfPid -or ($selfParentPid -gt 0 -and $PSItem.ProcessId -eq $selfParentPid)) {
            return $false
        }

        $name = if ($null -eq $PSItem.Name) { "" } else { $PSItem.Name.ToLowerInvariant() }
        $cmd = if ($null -eq $PSItem.CommandLine) { "" } else { $PSItem.CommandLine.ToLowerInvariant() }

        if (
            $name -eq "ui-dioxus.exe" -or
            $name -eq "kaizen-gateway.exe" -or
            $name -eq "zeroclaw-gateway.exe" -or
            $name -eq "kaizen max mission control.exe" -or
            $name -eq "kaizen_max_mission_control.exe" -or
            $name -eq "kaizen_mission_control.exe"
        ) {
            return $true
        }

        if ($name -eq "powershell.exe" -and $cmd -like "*start-max.ps1*") {
            return $true
        }

        if ($name -eq "cargo.exe") {
            return ($cmd -like "*$repoRootLower*\\core*") -or ($cmd -like "*$repoRootLower*\\ui-rust-native*")
        }

        if ($name -eq "trunk.exe") {
            return ($cmd -like "*$repoRootLower*\\ui-rust-native*")
        }

        return $false
    }

    foreach ($proc in $stale) {
        try {
            Stop-Process -Id $proc.ProcessId -Force -ErrorAction Stop
            Write-Host "[Kaizen MAX] Stopped stale process $($proc.Name) (PID $($proc.ProcessId))" -ForegroundColor Yellow
        } catch {
            Write-Host "[Kaizen MAX] Failed to stop stale process PID $($proc.ProcessId): $($_.Exception.Message)" -ForegroundColor Red
        }
    }
}

function Write-UiCrashReport {
    param(
        [int]$ExitCode,
        [string]$RepoRoot
    )

    $logsDir = Join-Path $RepoRoot "logs"
    if (-not (Test-Path $logsDir)) {
        New-Item -ItemType Directory -Path $logsDir -Force | Out-Null
    }

    $reportPath = Join-Path $logsDir "ui-crash-last.txt"
    $exitHex = [System.BitConverter]::ToUInt32([System.BitConverter]::GetBytes([int]$ExitCode), 0)
    $lines = New-Object System.Collections.Generic.List[string]

    $lines.Add("timestamp_utc=$((Get-Date).ToUniversalTime().ToString('o'))")
    $lines.Add("exit_code_signed=$ExitCode")
    $lines.Add(("exit_code_hex=0x{0:X8}" -f $exitHex))
    $lines.Add("")

    $events = Get-WinEvent -FilterHashtable @{ LogName = "Application"; StartTime = (Get-Date).AddMinutes(-10) } -ErrorAction SilentlyContinue |
        Where-Object {
            ($PSItem.Id -eq 1000 -or $PSItem.Id -eq 1001) -and
            $null -ne $PSItem.Message -and
            (
                $PSItem.Message -like "*ui-dioxus.exe*" -or
                $PSItem.Message -like "*kaizen max mission control.exe*" -or
                $PSItem.Message -like "*kaizen_max_mission_control.exe*" -or
                $PSItem.Message -like "*kaizen-gateway.exe*" -or
                $PSItem.Message -like "*zeroclaw-gateway.exe*"
            )
        } |
        Select-Object -First 3

    $eventList = @($events)
    if ($eventList.Count -eq 0) {
        $lines.Add("No recent Application Error/Windows Error Reporting events for Mission Control UI in the last 10 minutes.")
    } else {
        foreach ($event in $eventList) {
            $lines.Add("event_time=$($event.TimeCreated.ToUniversalTime().ToString('o'))")
            $lines.Add("event_id=$($event.Id)")
            $lines.Add("provider=$($event.ProviderName)")
            $lines.Add("message=$($event.Message)")
            $lines.Add("")
        }
    }

    Set-Content -Path $reportPath -Value $lines -Encoding UTF8
    Write-Host "[Kaizen MAX] Wrote UI crash report: $reportPath" -ForegroundColor Yellow
}

Ensure-EnvironmentFile -Path $EnvFile -CreateIfMissing:$InitEnv
Load-EnvironmentFile -Path $EnvFile
Write-Host "[Kaizen MAX] Loaded environment from $EnvFile" -ForegroundColor Cyan

$repoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$coreDir = Join-Path $repoRoot "core"
$uiDir = Join-Path $repoRoot "ui-rust-native"
Stop-StaleKaizenProcesses -RepoRoot $repoRoot
$jobHandle = New-KillOnCloseJob

$started = New-Object System.Collections.Generic.List[object]
$exitCode = 0

try {
    if (-not $UIOnly) {
        $coreExe = Join-Path $coreDir "target\release\kaizen-gateway.exe"
        if (-not (Test-Path $coreExe)) {
            $coreExe = Join-Path $coreDir "target\release\zeroclaw-gateway.exe"
        }
        if (Test-Path $coreExe) {
            $coreProcess = Start-ExecutableProcess -Name "Kaizen Core" -ExecutablePath $coreExe -WorkingDirectory $coreDir -JobHandle $jobHandle -NoNewWindow
        } else {
            Write-Host "[Kaizen MAX] Release core binary not found. Using cargo run." -ForegroundColor Yellow
            $cargoPath = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
            if (-not (Test-Path $cargoPath)) { $cargoPath = "cargo" }
            $coreProcess = Start-CommandProcess -Name "Kaizen Core" -WorkingDirectory $coreDir -Command "`"$cargoPath`" run --bin kaizen-gateway" -JobHandle $jobHandle
        }

        $started.Add([PSCustomObject]@{
            Name = "Kaizen Core"
            Process = $coreProcess
        })
    }

    if (-not $CoreOnly) {
        $uiExeCandidates = @(
            (Join-Path $uiDir "target\release\kaizen_mission_control.exe"),
            (Join-Path $uiDir "src-tauri\target\release\kaizen_mission_control.exe")
        )

        $uiExe = $uiExeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1

        if ($uiExe) {
            $uiProcess = Start-ExecutableProcess -Name "Kaizen MAX UI" -ExecutablePath $uiExe -WorkingDirectory $uiDir -JobHandle $jobHandle
        } else {
            Write-Host "[Kaizen MAX] Release Mission Control binary not found. Using cargo tauri dev." -ForegroundColor Yellow
            $cargoPath = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
            if (-not (Test-Path $cargoPath)) { $cargoPath = "cargo" }
            $uiProcess = Start-CommandProcess -Name "Kaizen MAX UI" -WorkingDirectory $uiDir -Command "`"$cargoPath`" tauri dev" -JobHandle $jobHandle
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

        if ($single.Name -eq "Kaizen MAX UI" -and $exitCode -ne 0) {
            Write-UiCrashReport -ExitCode $exitCode -RepoRoot $repoRoot
        }
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

        if ($trigger.Name -eq "Kaizen MAX UI" -and $exitCode -ne 0) {
            Write-UiCrashReport -ExitCode $exitCode -RepoRoot $repoRoot
        }
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
