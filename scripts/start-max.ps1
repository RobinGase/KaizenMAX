<#
.SYNOPSIS
    Start Kaizen MAX - native Windows UI + ZeroClaw core.

.DESCRIPTION
    Launches the ZeroClaw runtime and the Kaizen MAX UI dashboard.
    Reads configuration from .env at the repository root.

.NOTES
    Phase G deliverable per implementation_plan.md
#>

param(
    [switch]$CoreOnly,
    [switch]$UIOnly,
    [string]$EnvFile = "$PSScriptRoot\..\.env"
)

$ErrorActionPreference = "Stop"

# ---- Load .env ----
if (Test-Path $EnvFile) {
    Get-Content $EnvFile | ForEach-Object {
        if ($_ -match '^\s*([^#][^=]+)=(.*)$') {
            [System.Environment]::SetEnvironmentVariable($Matches[1].Trim(), $Matches[2].Trim(), "Process")
        }
    }
    Write-Host "[Kaizen MAX] Loaded environment from $EnvFile" -ForegroundColor Cyan
} else {
    Write-Warning "[Kaizen MAX] No .env file found at $EnvFile - using defaults."
}

$RepoRoot  = Resolve-Path "$PSScriptRoot\.."
$CoreDir   = Join-Path $RepoRoot "core"
$UIDir     = Join-Path $RepoRoot "ui"

# ---- Start ZeroClaw Core ----
if (-not $UIOnly) {
    $coreBin = Join-Path $CoreDir "target\release\zeroclaw-gateway.exe"
    if (Test-Path $coreBin) {
        Write-Host "[Kaizen MAX] Starting ZeroClaw core..." -ForegroundColor Green
        Start-Process -FilePath $coreBin -WorkingDirectory $CoreDir -NoNewWindow
    } else {
        Write-Warning "[Kaizen MAX] ZeroClaw binary not found at $coreBin. Run 'cargo build --release' in core/ first."
    }
}

# ---- Start UI ----
if (-not $CoreOnly) {
    $desktopCandidates = @(
        (Join-Path $UIDir "desktop\KaizenMAX.exe"),
        (Join-Path $UIDir "dist\KaizenMAX.exe"),
        (Join-Path $UIDir "build\KaizenMAX.exe")
    )

    $desktopExe = $desktopCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1

    if ($desktopExe) {
        Write-Host "[Kaizen MAX] Starting packaged desktop UI from $desktopExe" -ForegroundColor Green
        Start-Process -FilePath $desktopExe -WorkingDirectory $UIDir
    } else {
        Write-Host "[Kaizen MAX] Packaged UI not found. Starting Vite dev server..." -ForegroundColor Yellow
        Push-Location $UIDir
        try {
            & npm run dev
        } finally {
            Pop-Location
        }
    }
}
