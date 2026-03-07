[CmdletBinding(PositionalBinding = $false)]
param(
    [Parameter(Mandatory = $true)]
    [string]$RepoRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

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

function Get-ToolPath {
    param(
        [string]$PreferredPath = "",
        [string]$CommandName
    )

    if (-not [string]::IsNullOrWhiteSpace($PreferredPath) -and (Test-Path $PreferredPath)) {
        return $PreferredPath
    }

    $command = Get-Command $CommandName -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        return $command.Source
    }

    throw "Required tool '$CommandName' was not found."
}

$repoRoot = (Resolve-Path $RepoRoot).Path
if (-not (Test-Path (Join-Path $repoRoot ".git"))) {
    throw "Repo root '$repoRoot' does not look like a git checkout."
}

$logsDir = Join-Path $repoRoot "logs\updater"
New-Item -ItemType Directory -Force $logsDir | Out-Null
$logPath = Join-Path $logsDir "update-last.log"
$transcriptStarted = $false
try {
    Start-Transcript -Path $logPath -Force | Out-Null
    $transcriptStarted = $true
} catch {
}

function Start-Launcher {
    param(
        [string]$LauncherPath,
        [switch]$Rebuild
    )

    $arguments = @(
        "-NoLogo",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $LauncherPath
    )

    if ($Rebuild.IsPresent) {
        $arguments += "-Rebuild"
    }

    Start-Process -FilePath "powershell.exe" -ArgumentList $arguments -WindowStyle Hidden | Out-Null
}

try {
    Start-Sleep -Seconds 2

    $gitPath = Get-ToolPath -CommandName "git"
    $launcherPath = Join-Path $repoRoot "scripts\launch-kaizen-max.ps1"

    $branch = ((& $gitPath -C $repoRoot rev-parse --abbrev-ref HEAD) | Out-String).Trim()
    if ($branch -ne "main") {
        throw "Updater requires the repo checkout to be on 'main'. Current branch: $branch"
    }

    $status = ((& $gitPath -C $repoRoot status --porcelain) | Out-String).Trim()
    if (-not [string]::IsNullOrWhiteSpace($status)) {
        throw "Updater requires a clean worktree. Current git status:`n$status"
    }

    Invoke-CheckedCommand -WorkingDirectory $repoRoot -FilePath $gitPath -Arguments @("fetch", "origin", "main")

    $behind = ((& $gitPath -C $repoRoot rev-list --count HEAD..origin/main) | Out-String).Trim()
    if ([int]$behind -le 0) {
        Start-Launcher -LauncherPath $launcherPath
        exit 0
    }

    Invoke-CheckedCommand -WorkingDirectory $repoRoot -FilePath $gitPath -Arguments @("pull", "--ff-only", "origin", "main")

    Start-Launcher -LauncherPath $launcherPath -Rebuild
}
finally {
    if ($transcriptStarted) {
        Stop-Transcript | Out-Null
    }
}
