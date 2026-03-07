[CmdletBinding(PositionalBinding = $false)]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$launcherPath = Join-Path $repoRoot "scripts\launch-kaizen-max.ps1"

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

$gitPath = Get-ToolPath -PreferredPath (Join-Path ${env:ProgramFiles} "Git\cmd\git.exe") -CommandName "git"
$powershellPath = Get-ToolPath -PreferredPath (Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe") -CommandName "powershell"

if (-not (Test-Path $launcherPath)) {
    throw "Launcher script is missing: $launcherPath"
}

$statusOutput = & $gitPath -C $repoRoot status --porcelain
if ($LASTEXITCODE -ne 0) {
    throw "Unable to read git status."
}
if (-not [string]::IsNullOrWhiteSpace(($statusOutput | Out-String).Trim())) {
    throw "Refusing to update because the repo has local changes."
}

$branch = (& $gitPath -C $repoRoot rev-parse --abbrev-ref HEAD).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "Unable to determine the current git branch."
}
if ($branch -ne "main") {
    throw "Refusing to update because this install is on branch '$branch' instead of 'main'."
}

Invoke-CheckedCommand -WorkingDirectory $repoRoot -FilePath $gitPath -Arguments @("fetch", "--quiet", "origin", "main")

$behindRaw = (& $gitPath -C $repoRoot rev-list --count HEAD..origin/main).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "Unable to compare the local checkout with origin/main."
}

$behindCount = 0
if (-not [int]::TryParse($behindRaw, [ref]$behindCount)) {
    throw "Unexpected git compare result: $behindRaw"
}

if ($behindCount -gt 0) {
    Invoke-CheckedCommand -WorkingDirectory $repoRoot -FilePath $gitPath -Arguments @("pull", "--ff-only", "origin", "main")
}

& $powershellPath -NoProfile -ExecutionPolicy Bypass -File $launcherPath -Rebuild
if ($LASTEXITCODE -ne 0) {
    throw "Launcher failed after updating."
}
