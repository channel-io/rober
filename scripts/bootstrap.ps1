[CmdletBinding()]
param(
    [switch]$ForceZeroClawInstall,
    [switch]$SkipZeroClawInstall,
    [switch]$SourceBuild,
    [switch]$SkipTests,
    [switch]$SkipDoctor,
    [switch]$InstallRust,
    [switch]$InstallGit
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Script:RepoRoot = Split-Path -Parent $PSScriptRoot
$Script:InstallDir = if ($env:ZEROCLAW_INSTALL_DIR) { $env:ZEROCLAW_INSTALL_DIR } else { Join-Path $HOME ".cargo\bin" }

function Write-Step {
    param([string]$Message)
    Write-Host "==> $Message"
}

function Write-WarnLine {
    param([string]$Message)
    Write-Warning $Message
}

function Test-CommandExists {
    param([string]$Name)
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Resolve-ZeroClawBinary {
    if ($env:ZEROCLAW_BIN) {
        if (Test-Path $env:ZEROCLAW_BIN) {
            return (Resolve-Path $env:ZEROCLAW_BIN).Path
        }

        throw "ZEROCLAW_BIN points to a missing path: $($env:ZEROCLAW_BIN)"
    }

    $command = Get-Command zeroclaw -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $installedBinary = Join-Path $Script:InstallDir "zeroclaw.exe"
    if (Test-Path $installedBinary) {
        return $installedBinary
    }

    return $null
}

function Ensure-Winget {
    if (-not (Test-CommandExists winget)) {
        throw "winget is required for automatic Rust installation. Install winget or install Rust manually."
    }
}

function Ensure-Git {
    if (Test-CommandExists git) {
        return
    }

    if (-not $InstallGit) {
        throw @"
Git was not found.

Install it manually with:
  winget install --id Git.Git -e --source winget

Or re-run this script with:
  -InstallGit
"@
    }

    Ensure-Winget
    Write-Step "installing Git via winget"
    winget install --id Git.Git -e --source winget --accept-package-agreements --accept-source-agreements

    if (-not (Test-CommandExists git)) {
        throw "Git installation finished but git is still not visible. Open a new terminal and re-run the script."
    }
}

function Ensure-RustToolchain {
    if ((Test-CommandExists cargo) -and (Test-CommandExists rustc)) {
        return
    }

    if (-not $InstallRust) {
        throw @"
Rust toolchain was not found.

Install it manually with the official command from the zeroclaw README:
  winget install Rustlang.Rustup

Then open a new terminal and re-run this script.
"@
    }

    Ensure-Winget
    Write-Step "installing Rust toolchain via winget"
    winget install Rustlang.Rustup --accept-package-agreements --accept-source-agreements

    $cargoBin = Join-Path $HOME ".cargo\bin"
    if (Test-Path $cargoBin) {
        $env:PATH = "$cargoBin;$env:PATH"
    }

    if (-not (Test-CommandExists cargo) -or -not (Test-CommandExists rustc)) {
        throw "Rust installation finished but cargo/rustc are still not visible. Open a new terminal and re-run the script."
    }
}

function Install-ZeroClawPrebuilt {
    $releaseUri = "https://github.com/zeroclaw-labs/zeroclaw/releases/latest/download/zeroclaw-x86_64-pc-windows-msvc.zip"
    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("zeroclaw-bootstrap-" + [System.Guid]::NewGuid().ToString("N"))
    $archivePath = Join-Path $tempRoot "zeroclaw.zip"
    $extractDir = Join-Path $tempRoot "extract"

    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
    New-Item -ItemType Directory -Force -Path $extractDir | Out-Null

    Write-Step "downloading zeroclaw prebuilt binary"
    Invoke-WebRequest -Uri $releaseUri -OutFile $archivePath

    Write-Step "extracting zeroclaw"
    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force

    $binary = Get-ChildItem -Path $extractDir -Filter "zeroclaw.exe" -File -Recurse | Select-Object -First 1
    if (-not $binary) {
        throw "prebuilt archive did not contain zeroclaw.exe"
    }

    New-Item -ItemType Directory -Force -Path $Script:InstallDir | Out-Null
    $destination = Join-Path $Script:InstallDir "zeroclaw.exe"
    Copy-Item -Path $binary.FullName -Destination $destination -Force
    $env:ZEROCLAW_BIN = $destination

    Write-Step "installed zeroclaw to $destination"
}

function Install-ZeroClawFromSource {
    Ensure-Git
    Ensure-RustToolchain

    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("zeroclaw-source-" + [System.Guid]::NewGuid().ToString("N"))
    $sourceDir = Join-Path $tempRoot "zeroclaw"
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null

    Write-Step "cloning zeroclaw source"
    git clone --depth 1 https://github.com/zeroclaw-labs/zeroclaw.git $sourceDir

    Write-Step "installing zeroclaw from source"
    Push-Location $sourceDir
    try {
        cargo install --path . --force --locked
    }
    finally {
        Pop-Location
    }

    $installed = Join-Path $Script:InstallDir "zeroclaw.exe"
    if (Test-Path $installed) {
        $env:ZEROCLAW_BIN = $installed
    }
}

function Ensure-ZeroClaw {
    if ($SkipZeroClawInstall) {
        Write-WarnLine "skipping zeroclaw installation by request"
        return
    }

    $existing = Resolve-ZeroClawBinary
    if ($existing -and -not $ForceZeroClawInstall) {
        $env:ZEROCLAW_BIN = $existing
        Write-Step "using existing zeroclaw at $existing"
        return
    }

    if ($SourceBuild) {
        Install-ZeroClawFromSource
    }
    else {
        Install-ZeroClawPrebuilt
    }
}

function Run-WorkspaceTests {
    if ($SkipTests) {
        Write-WarnLine "skipping cargo test by request"
        return
    }

    Ensure-RustToolchain

    if (-not (Test-CommandExists link.exe) -and -not (Test-CommandExists cl.exe)) {
        Write-WarnLine @"
MSVC build tools were not detected.
`cargo test` may fail until Visual Studio Build Tools are installed with the "Desktop development with C++" workload.
Official zeroclaw README command:
  winget install Microsoft.VisualStudio.2022.BuildTools
"@
    }

    Write-Step "running cargo test"
    Push-Location $Script:RepoRoot
    try {
        cargo test
    }
    finally {
        Pop-Location
    }
}

function Run-Doctor {
    if ($SkipDoctor) {
        Write-WarnLine "skipping doctor by request"
        return
    }

    Ensure-RustToolchain
    Write-Step "running rover-probe doctor"

    Push-Location $Script:RepoRoot
    try {
        cargo run -p rover-probe -- doctor
    }
    finally {
        Pop-Location
    }
}

Ensure-ZeroClaw
Run-WorkspaceTests
Run-Doctor
Write-Step "bootstrap complete"
