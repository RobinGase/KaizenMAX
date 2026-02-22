param(
    [string]$BaseUrl = "http://127.0.0.1:9100",
    [int]$StartupTimeoutSec = 120,
    [int]$RequestTimeoutSec = 5,
    [switch]$UseStartMax
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$coreDir = Join-Path $repoRoot "core"
$uiDir = Join-Path $repoRoot "ui-rust-native"
$startMaxPath = Join-Path $PSScriptRoot "start-max.ps1"

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$reportDir = Join-Path $repoRoot "logs\validation\$timestamp"
New-Item -ItemType Directory -Path $reportDir -Force | Out-Null
$reportPath = Join-Path $reportDir "report.md"

$startedProcesses = New-Object System.Collections.Generic.List[System.Diagnostics.Process]
$results = New-Object System.Collections.Generic.List[object]

function Load-EnvironmentFile {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        return
    }

    Get-Content -Path $Path | ForEach-Object {
        if ($_ -match '^\s*([^#][^=]+)=(.*)$') {
            [System.Environment]::SetEnvironmentVariable(
                $Matches[1].Trim(),
                $Matches[2].Trim(),
                "Process"
            )
        }
    }
}

function Stop-ProcessTree {
    param([System.Diagnostics.Process]$Process)

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

    & taskkill /PID $Process.Id /T /F *> $null
}

function Add-Result {
    param(
        [string]$Check,
        [string]$Endpoint,
        [bool]$Passed,
        [string]$Details
    )

    $results.Add([PSCustomObject]@{
        Check = $Check
        Endpoint = $Endpoint
        Passed = $Passed
        Details = $Details
    })
}

function Has-Result {
    param(
        [string]$Check,
        [string]$Endpoint
    )

    foreach ($row in $results) {
        if ($row.Check -eq $Check -and $row.Endpoint -eq $Endpoint) {
            return $true
        }
    }

    return $false
}

function Wait-Health {
    param(
        [string]$HealthUrl,
        [int]$TimeoutSec,
        [int]$RequestTimeout,
        [System.Collections.Generic.List[System.Diagnostics.Process]]$TrackedProcesses
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    $lastError = "No response"

    while ((Get-Date) -lt $deadline) {
        foreach ($proc in $TrackedProcesses) {
            if ($null -eq $proc) {
                continue
            }

            try {
                if ($proc.HasExited) {
                    throw "Process PID $($proc.Id) exited with code $($proc.ExitCode) before health check was ready."
                }
            } catch {
                throw $_
            }
        }

        try {
            $health = Invoke-RestMethod -Method Get -Uri $HealthUrl -TimeoutSec $RequestTimeout
            if ($null -ne $health -and $health.status -eq "ok") {
                return @{ Ready = $true; Details = "status=ok" }
            }

            $lastError = "Unexpected health payload"
        } catch {
            $lastError = $_.Exception.Message
        }

        Start-Sleep -Milliseconds 500
    }

    return @{ Ready = $false; Details = $lastError }
}

function Invoke-EndpointValidation {
    param(
        [string]$Path,
        [int]$TimeoutSec,
        [ScriptBlock]$Validator
    )

    $url = "$BaseUrl$Path"
    try {
        $response = Invoke-RestMethod -Method Get -Uri $url -TimeoutSec $TimeoutSec
        $validation = & $Validator $response

        if ($validation.Passed) {
            Add-Result -Check "Endpoint" -Endpoint $Path -Passed $true -Details $validation.Details
        } else {
            Add-Result -Check "Endpoint" -Endpoint $Path -Passed $false -Details $validation.Details
        }
    } catch {
        Add-Result -Check "Endpoint" -Endpoint $Path -Passed $false -Details $_.Exception.Message
    }
}

Load-EnvironmentFile -Path (Join-Path $repoRoot ".env")

try {
    if ($UseStartMax) {
        if (-not (Test-Path $startMaxPath)) {
            throw "Cannot find start-max script at $startMaxPath"
        }

        $startMaxProc = Start-Process -FilePath "powershell.exe" -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $startMaxPath -WorkingDirectory $repoRoot -PassThru
        $startedProcesses.Add($startMaxProc)
    } else {
        $coreProc = Start-Process -FilePath "cargo" -ArgumentList "run", "--bin", "kaizen-gateway" -WorkingDirectory $coreDir -PassThru
        $uiProc = Start-Process -FilePath "cargo" -ArgumentList "tauri", "dev" -WorkingDirectory $uiDir -PassThru
        $startedProcesses.Add($coreProc)
        $startedProcesses.Add($uiProc)
    }

    $healthPath = "/health"
    $healthUrl = "$BaseUrl$healthPath"
    $healthResult = Wait-Health -HealthUrl $healthUrl -TimeoutSec $StartupTimeoutSec -RequestTimeout $RequestTimeoutSec -TrackedProcesses $startedProcesses

    Add-Result -Check "Health" -Endpoint $healthPath -Passed $healthResult.Ready -Details $healthResult.Details

    if ($healthResult.Ready) {
        Invoke-EndpointValidation -Path "/api/gates" -TimeoutSec $RequestTimeoutSec -Validator {
            param($response)
            if ($null -eq $response) {
                return @{ Passed = $false; Details = "Response is null" }
            }

            if ($response.PSObject.Properties.Name -contains "gates") {
                return @{ Passed = $true; Details = "Received gate snapshot" }
            }

            return @{ Passed = $true; Details = "Received non-null response" }
        }

        Invoke-EndpointValidation -Path "/api/events" -TimeoutSec $RequestTimeoutSec -Validator {
            param($response)
            if ($response -is [System.Array]) {
                return @{ Passed = $true; Details = "Received array with $($response.Count) item(s)" }
            }

            return @{ Passed = $false; Details = "Expected array response" }
        }

        Invoke-EndpointValidation -Path "/api/agents" -TimeoutSec $RequestTimeoutSec -Validator {
            param($response)
            if ($response -is [System.Array]) {
                return @{ Passed = $true; Details = "Received array with $($response.Count) item(s)" }
            }

            return @{ Passed = $false; Details = "Expected array response" }
        }
    } else {
        Add-Result -Check "Endpoint" -Endpoint "/api/gates" -Passed $false -Details "Skipped: health check failed"
        Add-Result -Check "Endpoint" -Endpoint "/api/events" -Passed $false -Details "Skipped: health check failed"
        Add-Result -Check "Endpoint" -Endpoint "/api/agents" -Passed $false -Details "Skipped: health check failed"
    }
}
catch {
    $errorMessage = $_.Exception.Message
    Add-Result -Check "Launcher" -Endpoint "core+ui" -Passed $false -Details $errorMessage

    if (-not (Has-Result -Check "Health" -Endpoint "/health")) {
        Add-Result -Check "Health" -Endpoint "/health" -Passed $false -Details "Skipped: launcher failed"
    }

    if (-not (Has-Result -Check "Endpoint" -Endpoint "/api/gates")) {
        Add-Result -Check "Endpoint" -Endpoint "/api/gates" -Passed $false -Details "Skipped: launcher failed"
    }

    if (-not (Has-Result -Check "Endpoint" -Endpoint "/api/events")) {
        Add-Result -Check "Endpoint" -Endpoint "/api/events" -Passed $false -Details "Skipped: launcher failed"
    }

    if (-not (Has-Result -Check "Endpoint" -Endpoint "/api/agents")) {
        Add-Result -Check "Endpoint" -Endpoint "/api/agents" -Passed $false -Details "Skipped: launcher failed"
    }
}
finally {
    foreach ($proc in $startedProcesses) {
        Stop-ProcessTree -Process $proc
    }

    Start-Sleep -Milliseconds 300
    Get-Process kaizen-gateway -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process zeroclaw-gateway -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process "kaizen max mission control" -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process kaizen_max_mission_control -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process trunk -ErrorAction SilentlyContinue | Stop-Process -Force
}

$passCount = @($results | Where-Object { $_.Passed }).Count
$failCount = @($results | Where-Object { -not $_.Passed }).Count

$lines = New-Object System.Collections.Generic.List[string]
$lines.Add("# Launch Validation Report")
$lines.Add("")
$lines.Add("- Timestamp: $((Get-Date).ToUniversalTime().ToString('o'))")
$lines.Add("- Base URL: $BaseUrl")
$lines.Add("- Startup mode: $(if ($UseStartMax) { 'start-max.ps1' } else { 'direct cargo run' })")
$lines.Add("- Result: $passCount passed, $failCount failed")
$lines.Add("")
$lines.Add("| Check | Endpoint | Result | Details |")
$lines.Add("| --- | --- | --- | --- |")

foreach ($row in $results) {
    $state = if ($row.Passed) { "PASS" } else { "FAIL" }
    $detail = (($row.Details -replace "`r", " ") -replace "`n", " ").Trim()
    if ([string]::IsNullOrWhiteSpace($detail)) {
        $detail = "-"
    }
    $lines.Add("| $($row.Check) | $($row.Endpoint) | $state | $detail |")
}

Set-Content -Path $reportPath -Value $lines -Encoding UTF8

Write-Output "Report path: $reportPath"
Write-Output "Pass: $passCount"
Write-Output "Fail: $failCount"

if ($failCount -gt 0) {
    exit 1
}

exit 0
