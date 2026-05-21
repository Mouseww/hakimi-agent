# ─────────────────────────────────────────────────────────────────────────────
# Hakimi Agent Installer for Windows (PowerShell)
#
# Usage:
#   irm https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.ps1 | iex
#
# Environment variables:
#   HAKIMI_INSTALL_DIR  — Installation directory (default: ~/.hakimi/bin)
#   HAKIMI_VERSION      — Version to install (default: latest)
# ─────────────────────────────────────────────────────────────────────────────

param(
    [string]$Version = "latest",
    [string]$InstallDir = "$env:USERPROFILE\.hakimi\bin"
)

$ErrorActionPreference = "Stop"

$Repo = "Mouseww/hakimi-agent"

Write-Host ""
Write-Host "  Hakimi Agent Installer" -ForegroundColor Cyan
Write-Host "  =======================" -ForegroundColor Cyan
Write-Host ""

# Detect architecture
$Arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { Write-Host "Error: 32-bit Windows is not supported" -ForegroundColor Red; exit 1 }
$Platform = "pc-windows-msvc"

Write-Host "[INFO]  Detected: $Arch-$Platform" -ForegroundColor Blue

# Build download URL
if ($Version -eq "latest") {
    $DownloadUrl = "https://github.com/$Repo/releases/latest/download/hakimi-$Arch-$Platform.zip"
} else {
    $DownloadUrl = "https://github.com/$Repo/releases/download/$Version/hakimi-$Arch-$Platform.zip"
}

Write-Host "[INFO]  Download: $DownloadUrl" -ForegroundColor Blue
Write-Host ""

# Create install directory
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Download
$TempFile = Join-Path $env:TEMP "hakimi-install.zip"
$TempExtract = Join-Path $env:TEMP "hakimi-extract"

try {
    Write-Host "Downloading..." -ForegroundColor Cyan
    try {
        Invoke-WebRequest -Uri $DownloadUrl -OutFile $TempFile -UseBasicParsing
    } catch {
        Write-Host ""
        Write-Host "[WARN]  No pre-built binary found for Windows." -ForegroundColor Yellow
        Write-Host ""
        Write-Host "  Build from source instead:" -ForegroundColor Yellow
        Write-Host "    1. Install Rust: https://rustup.rs" -ForegroundColor Yellow
        Write-Host "    2. Run: cargo install hakimi-agent" -ForegroundColor Yellow
        Write-Host ""
        exit 1
    }

    # Extract
    if (Test-Path $TempExtract) { Remove-Item -Recurse -Force $TempExtract }
    Expand-Archive -Path $TempFile -DestinationPath $TempExtract -Force

    # Find and copy binary
    $Exe = Get-ChildItem -Path $TempExtract -Filter "hakimi.exe" -Recurse | Select-Object -First 1
    if ($Exe) {
        Copy-Item $Exe.FullName -Destination (Join-Path $InstallDir "hakimi.exe") -Force
        Write-Host "[OK]    Installed to: $InstallDir\hakimi.exe" -ForegroundColor Green
    } else {
        Write-Host "[ERR]   hakimi.exe not found in archive" -ForegroundColor Red
        exit 1
    }

    # Add to PATH
    $CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($CurrentPath -notlike "*$InstallDir*") {
        $NewPath = "$InstallDir;$CurrentPath"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        $env:Path = "$InstallDir;$env:Path"
        Write-Host "[OK]    Added to PATH. Restart terminal to apply." -ForegroundColor Green
    } else {
        Write-Host "[OK]    Already in PATH." -ForegroundColor Green
    }

    # Verify
    Write-Host ""
    try {
        $verOutput = & (Join-Path $InstallDir "hakimi.exe") --version 2>&1
        if ($verOutput) { Write-Host "  $verOutput" -ForegroundColor White }
    } catch {
        # Binary exists but --version may not be implemented yet
    }

    Write-Host ""
    Write-Host "  Hakimi Agent installed successfully!" -ForegroundColor Green
    Write-Host "  Run 'hakimi --setup' to configure." -ForegroundColor Cyan
    Write-Host ""

} finally {
    if (Test-Path $TempFile) { Remove-Item $TempFile -Force -ErrorAction SilentlyContinue }
    if (Test-Path $TempExtract) { Remove-Item -Recurse -Force $TempExtract -ErrorAction SilentlyContinue }
}
