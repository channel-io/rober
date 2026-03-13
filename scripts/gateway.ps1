param(
    [int]$Port = 8090,
    [string]$Config = "configs/gateway/config.toml"
)

$ErrorActionPreference = "Stop"

Write-Host "Building gateway..." -ForegroundColor Cyan
cargo build --release -p rover-gateway
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "Starting gateway on port $Port..." -ForegroundColor Cyan
$env:GATEWAY_CONFIG = $Config
$gateway = Start-Process -FilePath ".\target\release\rover-gateway.exe" `
    -PassThru -NoNewWindow

Start-Sleep -Seconds 1
if ($gateway.HasExited) {
    Write-Host "Gateway failed to start" -ForegroundColor Red
    exit 1
}

Write-Host "Starting cloudflared tunnel..." -ForegroundColor Cyan
$tunnel = Start-Process -FilePath "cloudflared" `
    -ArgumentList "tunnel", "--url", "http://127.0.0.1:$Port" `
    -PassThru -NoNewWindow

Write-Host ""
Write-Host "Gateway running (PID $($gateway.Id))" -ForegroundColor Green
Write-Host "Tunnel  running (PID $($tunnel.Id))" -ForegroundColor Green
Write-Host "Press Ctrl+C to stop" -ForegroundColor Yellow

try {
    while (-not $gateway.HasExited -and -not $tunnel.HasExited) {
        Start-Sleep -Milliseconds 500
    }
} finally {
    Write-Host "`nShutting down..." -ForegroundColor Cyan
    if (-not $gateway.HasExited) { Stop-Process -Id $gateway.Id -Force -ErrorAction SilentlyContinue }
    if (-not $tunnel.HasExited) { Stop-Process -Id $tunnel.Id -Force -ErrorAction SilentlyContinue }
}
