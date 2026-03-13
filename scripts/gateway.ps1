param(
    [int]$Port = 8090,
    [string]$Config = "configs/gateway/config.toml"
)

$ErrorActionPreference = "Stop"

# --- cloudflared 설치 확인 / 자동 설치 ---
if (-not (Get-Command cloudflared -ErrorAction SilentlyContinue)) {
    Write-Host "cloudflared not found. Installing via winget..." -ForegroundColor Yellow
    winget install --id Cloudflare.cloudflared --accept-source-agreements --accept-package-agreements
    if ($LASTEXITCODE -ne 0) {
        Write-Host "winget install failed. Trying direct download..." -ForegroundColor Yellow
        $cfUrl = "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe"
        $cfPath = "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\cloudflared.exe"
        New-Item -ItemType Directory -Path (Split-Path $cfPath) -Force | Out-Null
        Invoke-WebRequest -Uri $cfUrl -OutFile $cfPath -UseBasicParsing
        $env:PATH += ";$(Split-Path $cfPath)"
        Write-Host "Downloaded cloudflared to $cfPath" -ForegroundColor Green
    }
    # winget 설치 후 PATH 갱신
    $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("PATH", "User")
    if (-not (Get-Command cloudflared -ErrorAction SilentlyContinue)) {
        Write-Host "cloudflared still not found in PATH. Please restart terminal or add it to PATH manually." -ForegroundColor Red
        exit 1
    }
}
Write-Host "cloudflared: $(cloudflared --version)" -ForegroundColor Green

# --- Build ---
Write-Host "Building gateway..." -ForegroundColor Cyan
cargo build --release -p rover-gateway
if ($LASTEXITCODE -ne 0) { exit 1 }

# --- Start gateway ---
Write-Host "Starting gateway on port $Port..." -ForegroundColor Cyan
$env:GATEWAY_CONFIG = $Config
$gateway = Start-Process -FilePath ".\target\release\rover-gateway.exe" `
    -PassThru -NoNewWindow

Start-Sleep -Seconds 1
if ($gateway.HasExited) {
    Write-Host "Gateway failed to start" -ForegroundColor Red
    exit 1
}

# --- Start tunnel ---
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
