param(
    [string]$RunDir,
    [switch]$Latest,
    [string]$OutFile
)

$ErrorActionPreference = "Stop"

function Resolve-StressRunDir {
    param(
        [string]$RepoRoot,
        [string]$StressRoot,
        [string]$RequestedRunDir,
        [switch]$UseLatest
    )

    if (-not (Test-Path -Path $StressRoot -PathType Container)) {
        throw "Stress logs root not found: $StressRoot"
    }

    if ($RequestedRunDir) {
        $candidates = @()
        if ([System.IO.Path]::IsPathRooted($RequestedRunDir)) {
            $candidates += $RequestedRunDir
        } else {
            $candidates += (Join-Path $StressRoot $RequestedRunDir)
            $candidates += (Join-Path $RepoRoot $RequestedRunDir)
        }

        foreach ($candidate in $candidates) {
            if (Test-Path -Path $candidate -PathType Container) {
                return (Resolve-Path -Path $candidate).Path
            }
        }

        throw "Run directory not found. Checked: $($candidates -join ', ')"
    }

    if (-not $UseLatest) {
        throw "Either provide -RunDir or use -Latest."
    }

    $latestDir = Get-ChildItem -Path $StressRoot -Directory |
        Sort-Object -Property LastWriteTime -Descending |
        Select-Object -First 1

    if (-not $latestDir) {
        throw "No stress run directories found under: $StressRoot"
    }

    return $latestDir.FullName
}

function Parse-KeyValueFile {
    param([string]$Path)

    $map = @{}
    foreach ($line in Get-Content -Path $Path) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }

        $idx = $line.IndexOf("=")
        if ($idx -lt 1) {
            continue
        }

        $key = $line.Substring(0, $idx).Trim()
        $value = $line.Substring($idx + 1).Trim()
        if ($key) {
            $map[$key] = $value
        }
    }

    return $map
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$stressRoot = Join-Path $repoRoot "logs\stress"
$useLatest = $Latest.IsPresent -or (-not $RunDir)

$resolvedRunDir = Resolve-StressRunDir -RepoRoot $repoRoot -StressRoot $stressRoot -RequestedRunDir $RunDir -UseLatest:$useLatest
$metricsCsv = Join-Path $resolvedRunDir "metrics.csv"
$summaryTxt = Join-Path $resolvedRunDir "summary.txt"

if (-not (Test-Path -Path $metricsCsv -PathType Leaf)) {
    throw "Missing required file: $metricsCsv"
}

if (-not (Test-Path -Path $summaryTxt -PathType Leaf)) {
    throw "Missing required file: $summaryTxt"
}

$rows = Import-Csv -Path $metricsCsv
if (-not $rows -or $rows.Count -eq 0) {
    throw "No metric rows found in: $metricsCsv"
}

$summaryMap = Parse-KeyValueFile -Path $summaryTxt

$sampleCount = 0
$cpuTotal = 0.0
$memTotal = 0.0
$cpuMax = [double]::NegativeInfinity
$memMax = [double]::NegativeInfinity
$firstTs = [datetimeoffset]::MaxValue
$lastTs = [datetimeoffset]::MinValue
$actionCounts = @{}

foreach ($row in $rows) {
    $cpu = 0.0
    if (-not [double]::TryParse($row.cpu_percent, [System.Globalization.NumberStyles]::Float, [System.Globalization.CultureInfo]::InvariantCulture, [ref]$cpu)) {
        throw "Invalid cpu_percent value '$($row.cpu_percent)' at timestamp '$($row.timestamp)'"
    }

    $mem = 0.0
    if (-not [double]::TryParse($row.memory_percent, [System.Globalization.NumberStyles]::Float, [System.Globalization.CultureInfo]::InvariantCulture, [ref]$mem)) {
        throw "Invalid memory_percent value '$($row.memory_percent)' at timestamp '$($row.timestamp)'"
    }

    $ts = [datetimeoffset]::MinValue
    if (-not [datetimeoffset]::TryParse($row.timestamp, [System.Globalization.CultureInfo]::InvariantCulture, [System.Globalization.DateTimeStyles]::RoundtripKind, [ref]$ts)) {
        throw "Invalid timestamp value '$($row.timestamp)'"
    }

    $sampleCount++
    $cpuTotal += $cpu
    $memTotal += $mem

    if ($cpu -gt $cpuMax) { $cpuMax = $cpu }
    if ($mem -gt $memMax) { $memMax = $mem }
    if ($ts -lt $firstTs) { $firstTs = $ts }
    if ($ts -gt $lastTs) { $lastTs = $ts }

    $action = if ($row.action) { $row.action.Trim() } else { "(blank)" }
    if (-not $actionCounts.ContainsKey($action)) {
        $actionCounts[$action] = 0
    }
    $actionCounts[$action]++
}

$cpuAvg = if ($sampleCount -gt 0) { $cpuTotal / $sampleCount } else { 0.0 }
$memAvg = if ($sampleCount -gt 0) { $memTotal / $sampleCount } else { 0.0 }

$resultValue = ""
$reasonValue = ""
if ($summaryMap.ContainsKey("result")) { $resultValue = $summaryMap["result"] }
if ($summaryMap.ContainsKey("reason")) { $reasonValue = $summaryMap["reason"] }

$hardStop = ($resultValue -eq "hard_stop") -or ($reasonValue -match "hard-stop")
$hardStopText = if ($hardStop) { "yes" } else { "no" }

if (-not $OutFile) {
    $OutFile = Join-Path $resolvedRunDir "analysis.md"
} elseif (-not [System.IO.Path]::IsPathRooted($OutFile)) {
    $OutFile = Join-Path $repoRoot $OutFile
}

$outDir = Split-Path -Parent $OutFile
if ($outDir -and -not (Test-Path -Path $outDir -PathType Container)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}

$actionLines = @()
foreach ($key in ($actionCounts.Keys | Sort-Object)) {
    $actionLines += "- ``$key``: $($actionCounts[$key])"
}

$mdLines = @(
    "# Stress Run Summary",
    "",
    "- Run directory: ``$resolvedRunDir``",
    "- Total samples: $sampleCount",
    "- CPU max/avg: $([math]::Round($cpuMax, 2)) / $([math]::Round($cpuAvg, 2))",
    "- Memory max/avg: $([math]::Round($memMax, 2)) / $([math]::Round($memAvg, 2))",
    "- Hard stop occurred: $hardStopText",
    "- First timestamp: $($firstTs.ToString('o'))",
    "- Last timestamp: $($lastTs.ToString('o'))",
    "",
    "## Action Counts"
)

$mdLines += $actionLines

Set-Content -Path $OutFile -Value $mdLines -Encoding utf8

$sortedActionPairs = foreach ($k in ($actionCounts.Keys | Sort-Object)) { "${k}=$($actionCounts[$k])" }
$actionSummary = $sortedActionPairs -join ", "

Write-Output "RunDir: $resolvedRunDir"
Write-Output "Samples: $sampleCount | CPU avg/max: $([math]::Round($cpuAvg, 2))/$([math]::Round($cpuMax, 2)) | Memory avg/max: $([math]::Round($memAvg, 2))/$([math]::Round($memMax, 2))"
Write-Output "HardStop: $hardStopText | First: $($firstTs.ToString('o')) | Last: $($lastTs.ToString('o'))"
Write-Output "Actions: $actionSummary"
Write-Output "SummaryMarkdown: $OutFile"
