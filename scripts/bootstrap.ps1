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

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]]$Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "command failed ($LASTEXITCODE): $FilePath $($Arguments -join ' ')"
    }
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

function Get-VsWherePath {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        return $vswhere
    }

    return $null
}

function Get-VisualStudioInstallationPath {
    $vswhere = Get-VsWherePath
    if (-not $vswhere) {
        return $null
    }

    $path = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0) {
        return $null
    }

    if ($path) {
        return $path.Trim()
    }

    return $null
}

function Get-VisualStudioDevCmdPath {
    $installPath = Get-VisualStudioInstallationPath
    if (-not $installPath) {
        $vswhere = Get-VsWherePath
        if (-not $vswhere) {
            return $null
        }

        $installPath = & $vswhere -latest -products * -property installationPath
        if ($LASTEXITCODE -ne 0 -or -not $installPath) {
            return $null
        }

        $installPath = $installPath.Trim()
    }

    $candidates = @(
        (Join-Path $installPath "Common7\Tools\LaunchDevCmd.bat"),
        (Join-Path $installPath "Common7\Tools\VsDevCmd.bat")
    )

    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            return $candidate
        }
    }

    return $null
}

function Test-MsvcBuildTools {
    return (Test-CommandExists link.exe) -or (Test-CommandExists cl.exe)
}

function Import-MsvcBuildEnvironment {
    if (Test-MsvcBuildTools) {
        return
    }

    $devCmd = Get-VisualStudioDevCmdPath
    if (-not $devCmd) {
        return
    }

    Write-Step "loading Visual Studio build environment"
    $envDump = & cmd.exe /s /c "`"$devCmd`" -arch=amd64 -host_arch=amd64 >nul && set"
    if ($LASTEXITCODE -ne 0) {
        return
    }

    foreach ($line in $envDump) {
        if ($line -match "^(.*?)=(.*)$") {
            [System.Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process")
        }
    }
}

function Ensure-MsvcBuildTools {
    Import-MsvcBuildEnvironment

    if (Test-MsvcBuildTools) {
        return
    }

    throw @"
MSVC build tools were not detected.

Install Visual Studio Build Tools and make sure the "Desktop development with C++" workload is selected:
  winget install Microsoft.VisualStudio.2022.BuildTools

If Build Tools are already installed, open the Visual Studio Installer and confirm the "Desktop development with C++" workload is enabled.
After that, open a new terminal and re-run the script.
"@
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
    Invoke-Checked winget install --id Git.Git -e --source winget --accept-package-agreements --accept-source-agreements

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
    Invoke-Checked winget install Rustlang.Rustup --accept-package-agreements --accept-source-agreements

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
    Invoke-Checked git clone --depth 1 https://github.com/zeroclaw-labs/zeroclaw.git $sourceDir

    Write-Step "installing zeroclaw from source"
    Push-Location $sourceDir
    try {
        Ensure-MsvcBuildTools
        Invoke-Checked cargo install --path . --force --locked
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
    Ensure-MsvcBuildTools

    Write-Step "running cargo test"
    Push-Location $Script:RepoRoot
    try {
        Invoke-Checked cargo test
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
    Ensure-MsvcBuildTools
    Write-Step "running rover-probe doctor"

    Push-Location $Script:RepoRoot
    try {
        Invoke-Checked cargo run -p rover-probe -- doctor
    }
    finally {
        Pop-Location
    }
}

Ensure-ZeroClaw
Run-WorkspaceTests
Run-Doctor
Write-Step "bootstrap complete"
