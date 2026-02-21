param(
    [string]$ApiBase = "http://127.0.0.1:9100",
    [string]$Provider = "kaizen",
    [string]$Model = "",
    [string]$Message = "Reply with exactly: OK",
    [string]$Mode = "build"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$healthUrl = "{0}/health" -f $ApiBase.TrimEnd('/')
$chatUrl = "{0}/api/chat" -f $ApiBase.TrimEnd('/')

$health = Invoke-WebRequest -Uri $healthUrl -Method GET -UseBasicParsing
if ([int]$health.StatusCode -ne 200) {
    throw "Health check failed: $($health.StatusCode)"
}

$bodyObj = @{
    message = $Message
    provider = $Provider
    mode = $Mode
}

if (-not [string]::IsNullOrWhiteSpace($Model)) {
    $bodyObj.model = $Model
}

$body = $bodyObj | ConvertTo-Json -Depth 5

try {
    $resp = Invoke-WebRequest -Uri $chatUrl -Method POST -ContentType "application/json" -Body $body -UseBasicParsing
    Write-Host "Smoke passed: HTTP $($resp.StatusCode)" -ForegroundColor Green
    Write-Host $resp.Content
} catch {
    Write-Host "Smoke failed." -ForegroundColor Red
    if ($_.Exception -and $_.Exception.Response) {
        Write-Host $_.Exception.Response.StatusCode
        $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
        Write-Host $reader.ReadToEnd()
        $reader.Close()
    } else {
        Write-Host $_.Exception.Message
    }
    throw
}
