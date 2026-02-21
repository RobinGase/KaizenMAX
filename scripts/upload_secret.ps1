param(
    [Parameter(Mandatory = $true)]
    [string]$SecretName,
    [Parameter(Mandatory = $true)]
    [string]$ValuePath,
    [string]$SecretType = "opaque",
    [string]$ApiBase = "http://127.0.0.1:9100",
    [string]$AdminToken = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $ValuePath)) {
    throw "Value file not found: $ValuePath"
}

$valueContent = Get-Content -LiteralPath $ValuePath -Raw -Encoding UTF8

$payload = @{
    value = $valueContent
    secret_type = $SecretType
} | ConvertTo-Json -Depth 5

$uri = "{0}/api/secrets/{1}" -f $ApiBase.TrimEnd('/'), $SecretName
$headers = @{}
if (-not [string]::IsNullOrWhiteSpace($AdminToken)) {
    $headers["x-admin-token"] = $AdminToken
}

Write-Host "Uploading secret '$SecretName' ..." -ForegroundColor Cyan
$response = Invoke-WebRequest -Uri $uri -Method PUT -ContentType "application/json" -Body $payload -Headers $headers -UseBasicParsing

Write-Host "Status Code: $($response.StatusCode)" -ForegroundColor Green
Write-Host $response.Content
