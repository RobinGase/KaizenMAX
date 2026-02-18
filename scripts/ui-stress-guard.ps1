param(
    [int]$DurationSec = 60,
    [double]$CpuLimit = 70,
    [double]$MemoryLimit = 70,
    [int]$SampleMs = 1000
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$coreDir = Join-Path $root "core"
$uiDir = Join-Path $root "ui-dioxus"
$logRoot = Join-Path $root "logs\stress"
$runStamp = Get-Date -Format "yyyyMMdd-HHmmss"
$runDir = Join-Path $logRoot $runStamp
New-Item -ItemType Directory -Force -Path $runDir | Out-Null

$metricsCsv = Join-Path $runDir "metrics.csv"
$eventsJson = Join-Path $runDir "events.json"
$summaryTxt = Join-Path $runDir "summary.txt"

"timestamp,cpu_percent,memory_percent,action" | Out-File -FilePath $metricsCsv -Encoding utf8

function Get-MemoryPercent {
    $os = Get-CimInstance Win32_OperatingSystem
    $total = [double]$os.TotalVisibleMemorySize
    $free = [double]$os.FreePhysicalMemory
    if ($total -le 0) { return 0 }
    return [math]::Round((($total - $free) / $total) * 100, 2)
}

function Get-CpuPercent {
    $value = (Get-Counter '\Processor(_Total)\% Processor Time').CounterSamples[0].CookedValue
    return [math]::Round($value, 2)
}

function Log-Metric([string]$action) {
    $ts = (Get-Date).ToString("o")
    $cpu = Get-CpuPercent
    $mem = Get-MemoryPercent
    "$ts,$cpu,$mem,$action" | Out-File -FilePath $metricsCsv -Append -Encoding utf8
    return @{ cpu = $cpu; mem = $mem; ts = $ts }
}

function Wait-Health {
    for ($i = 0; $i -lt 40; $i++) {
        try {
            $health = Invoke-RestMethod -Method Get -Uri "http://127.0.0.1:9100/health" -TimeoutSec 2
            if ($health.status -eq "ok") {
                return
            }
        } catch {}
        Start-Sleep -Milliseconds 500
    }
    throw "Core health endpoint did not become ready."
}

$coreProc = $null
$uiProc = $null
$hardStop = $false
$stopReason = "completed"

try {
    $coreProc = Start-Process cargo -WorkingDirectory $coreDir -ArgumentList "run" -PassThru
    Start-Sleep -Seconds 6
    $uiProc = Start-Process cargo -WorkingDirectory $uiDir -ArgumentList "run" -PassThru

    Wait-Health
    $null = Log-Metric "startup-ok"

    $agentIds = @()
    for ($i = 1; $i -le 4; $i++) {
        try {
            $payload = @{ agent_name = "Stress-$i"; task_id = "stress-$i"; objective = "UI stress run"; user_requested = $true } | ConvertTo-Json
            $spawned = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:9100/api/agents" -ContentType "application/json" -Body $payload
            if ($spawned.id) {
                $agentIds += $spawned.id
                $null = Log-Metric "spawn-$($spawned.id)"
            }
        } catch {
            $null = Log-Metric "spawn-failed"
        }
    }

    $deadline = (Get-Date).AddSeconds($DurationSec)
    $tick = 0

    while ((Get-Date) -lt $deadline) {
        $tick++

        $sample = Log-Metric "tick-$tick"
        if ($sample.cpu -ge $CpuLimit -or $sample.mem -ge $MemoryLimit) {
            $hardStop = $true
            $stopReason = "hard-stop cpu=$($sample.cpu) mem=$($sample.mem)"
            break
        }

        try {
            $msg = @{ message = "stress ping $tick" } | ConvertTo-Json
            Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:9100/api/chat" -ContentType "application/json" -Body $msg | Out-Null
            $null = Log-Metric "chat-kaizen"
        } catch {
            $null = Log-Metric "chat-kaizen-failed"
        }

        if ($agentIds.Count -gt 0) {
            $idx = ($tick - 1) % $agentIds.Count
            $aid = $agentIds[$idx]
            try {
                $msg = @{ message = "agent stress ping $tick"; agent_id = $aid } | ConvertTo-Json
                Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:9100/api/chat" -ContentType "application/json" -Body $msg | Out-Null
                $null = Log-Metric "chat-$aid"
            } catch {
                $null = Log-Metric "chat-$aid-failed"
            }
        }

        Start-Sleep -Milliseconds $SampleMs
    }

    try {
        $events = Invoke-RestMethod -Method Get -Uri "http://127.0.0.1:9100/api/events"
        $events | ConvertTo-Json -Depth 6 | Out-File -FilePath $eventsJson -Encoding utf8
    } catch {
        "event-export-failed: $_" | Out-File -FilePath $eventsJson -Encoding utf8
    }

    $endSample = Log-Metric "shutdown"
    $resultLabel = if ($hardStop) { "hard_stop" } else { "completed" }
    @(
        "result=$resultLabel",
        "reason=$stopReason",
        "last_cpu=$($endSample.cpu)",
        "last_memory=$($endSample.mem)",
        "logs=$runDir"
    ) | Out-File -FilePath $summaryTxt -Encoding utf8

    Write-Output "Stress run done: $runDir"
    if ($hardStop) {
        Write-Output "Hard stop triggered: $stopReason"
    }
}
finally {
    if ($uiProc -and -not $uiProc.HasExited) {
        Stop-Process -Id $uiProc.Id -Force
    }
    if ($coreProc -and -not $coreProc.HasExited) {
        Stop-Process -Id $coreProc.Id -Force
    }
    Get-Process ui-dioxus -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process zeroclaw-gateway -ErrorAction SilentlyContinue | Stop-Process -Force
}
