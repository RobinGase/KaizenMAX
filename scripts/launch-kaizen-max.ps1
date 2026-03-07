[CmdletBinding(PositionalBinding = $false)]
param(
    [switch]$Rebuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$coreDir = Join-Path $repoRoot "core"
$uiDir = Join-Path $repoRoot "ui-rust-native"
$envFile = Join-Path $repoRoot ".env"
$logsDir = Join-Path $repoRoot "logs\launcher"
$coreExe = Join-Path $coreDir "target\release\kaizen-gateway.exe"
$uiExe = Join-Path $uiDir "target\release\kaizen_mission_control.exe"

function Load-EnvironmentFile {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        return
    }

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

function Get-ToolPath {
    param(
        [string]$PreferredPath,
        [string]$CommandName
    )

    if (Test-Path $PreferredPath) {
        return $PreferredPath
    }

    $command = Get-Command $CommandName -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        return $command.Source
    }

    throw "Required tool '$CommandName' was not found."
}

function Get-LatestTimestampUtc {
    param([string[]]$Paths)

    $latest = [datetime]::MinValue

    foreach ($path in $Paths) {
        if (-not (Test-Path $path)) {
            continue
        }

        $item = Get-Item $path -ErrorAction Stop
        if ($item.PSIsContainer) {
            $candidate = Get-ChildItem $path -Recurse -File -ErrorAction SilentlyContinue |
                Sort-Object LastWriteTimeUtc -Descending |
                Select-Object -First 1
            if ($null -ne $candidate -and $candidate.LastWriteTimeUtc -gt $latest) {
                $latest = $candidate.LastWriteTimeUtc
            }
        } elseif ($item.LastWriteTimeUtc -gt $latest) {
            $latest = $item.LastWriteTimeUtc
        }
    }

    return $latest
}

function Test-RebuildNeeded {
    param(
        [string]$TargetPath,
        [string[]]$SourcePaths,
        [switch]$Force
    )

    if ($Force.IsPresent -or -not (Test-Path $TargetPath)) {
        return $true
    }

    $targetTime = (Get-Item $TargetPath).LastWriteTimeUtc
    $sourceTime = Get-LatestTimestampUtc -Paths $SourcePaths
    return $sourceTime -gt $targetTime
}

function Invoke-CheckedCommand {
    param(
        [string]$WorkingDirectory,
        [string]$FilePath,
        [string[]]$Arguments
    )

    Push-Location $WorkingDirectory
    try {
        & $FilePath @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
        }
    } finally {
        Pop-Location
    }
}

function Stop-StaleProcesses {
    $names = @("kaizen-gateway", "kaizen_mission_control")
    foreach ($name in $names) {
        Get-Process $name -ErrorAction SilentlyContinue | ForEach-Object {
            Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
        }
    }
}

function Wait-CoreHealthy {
    param(
        [string]$HealthUrl,
        [string]$StdoutPath,
        [string]$StderrPath
    )

    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        Start-Sleep -Milliseconds 500
        try {
            $health = Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 2
            if ($health.status -eq "ok") {
                return
            }
        } catch {
        }
    }

    $stdout = if (Test-Path $StdoutPath) { Get-Content $StdoutPath -Tail 80 | Out-String } else { "" }
    $stderr = if (Test-Path $StderrPath) { Get-Content $StderrPath -Tail 80 | Out-String } else { "" }
    throw "Core did not become healthy.`nSTDOUT:`n$stdout`nSTDERR:`n$stderr"
}

Load-EnvironmentFile -Path $envFile
New-Item -ItemType Directory -Force $logsDir | Out-Null

$cargoPath = Get-ToolPath -PreferredPath (Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe") -CommandName "cargo"

$coreSources = @(
    (Join-Path $coreDir "Cargo.toml"),
    (Join-Path $coreDir "Cargo.lock"),
    (Join-Path $coreDir "src")
)

$uiSources = @(
    (Join-Path $uiDir "Cargo.toml"),
    (Join-Path $uiDir "Cargo.lock"),
    (Join-Path $uiDir "frontend\Cargo.toml"),
    (Join-Path $uiDir "frontend\index.html"),
    (Join-Path $uiDir "frontend\src"),
    (Join-Path $uiDir "src-tauri\Cargo.toml"),
    (Join-Path $uiDir "src-tauri\Cargo.lock"),
    (Join-Path $uiDir "src-tauri\tauri.conf.json"),
    (Join-Path $uiDir "src-tauri\src"),
    (Join-Path $uiDir "src-tauri\icons")
)

if (Test-RebuildNeeded -TargetPath $coreExe -SourcePaths $coreSources -Force:$Rebuild) {
    Invoke-CheckedCommand -WorkingDirectory $coreDir -FilePath $cargoPath -Arguments @("build", "--release", "--bin", "kaizen-gateway")
}

if (Test-RebuildNeeded -TargetPath $uiExe -SourcePaths $uiSources -Force:$Rebuild) {
    Invoke-CheckedCommand -WorkingDirectory $uiDir -FilePath $cargoPath -Arguments @("tauri", "build")
}

Stop-StaleProcesses

[System.Environment]::SetEnvironmentVariable("KAIZEN_HOST", "127.0.0.1", "Process")
[System.Environment]::SetEnvironmentVariable("KAIZEN_CORE_URL", "http://127.0.0.1:9100", "Process")

if ([string]::IsNullOrWhiteSpace($env:KAIZEN_INFERENCE_PROVIDER)) {
    [System.Environment]::SetEnvironmentVariable("KAIZEN_INFERENCE_PROVIDER", "codex-cli", "Process")
}

if ([string]::IsNullOrWhiteSpace($env:KAIZEN_INFERENCE_MODEL)) {
    [System.Environment]::SetEnvironmentVariable("KAIZEN_INFERENCE_MODEL", "gpt-5.4", "Process")
}

$coreStdout = Join-Path $logsDir "core-last.log"
$coreStderr = Join-Path $logsDir "core-last.err.log"
Remove-Item $coreStdout, $coreStderr -ErrorAction SilentlyContinue

$coreProc = Start-Process -FilePath $coreExe -WorkingDirectory $coreDir -PassThru -WindowStyle Hidden -RedirectStandardOutput $coreStdout -RedirectStandardError $coreStderr

try {
    Wait-CoreHealthy -HealthUrl "http://127.0.0.1:9100/health" -StdoutPath $coreStdout -StderrPath $coreStderr
} catch {
    Stop-Process -Id $coreProc.Id -Force -ErrorAction SilentlyContinue
    throw
}

Start-Process -FilePath $uiExe -WorkingDirectory (Split-Path -Parent $uiExe) | Out-Null
Write-Host "Kaizen MAX launched." -ForegroundColor Green
