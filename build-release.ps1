# Multiplatform Release Builder for Redragon M719-PRO Suite
# Produces standardized release artifacts in ./dist/
# Naming pattern: redragon_m719_pro-<os>-<arch>[.exe]

$ErrorActionPreference = "Stop"

$DistDir = Join-Path $PSScriptRoot "dist"
if (!(Test-Path $DistDir)) {
    New-Item -ItemType Directory -Path $DistDir | Out-Null
}

function Build-Target {
    param (
        [string]$Target,
        [string]$BinName,
        [string]$OutName
    )
    Write-Host "`n========================================================" -ForegroundColor Cyan
    Write-Host " Building Target: $Target -> dist/$OutName" -ForegroundColor Cyan
    Write-Host "========================================================" -ForegroundColor Cyan

    rustup target add $Target
    cargo build --release --bin redragon_m719_pro --target $Target

    $SrcPath = Join-Path $PSScriptRoot "target\$Target\release\$BinName"
    $DstPath = Join-Path $DistDir $OutName
    Copy-Item -Path $SrcPath -Destination $DstPath -Force
    Write-Host "[OK] Artifact created: $DstPath" -ForegroundColor Green
}

if ($IsWindows -or ($env:OS -eq "Windows_NT")) {
    # 1. Windows 64-bit (x64)
    Build-Target -Target "x86_64-pc-windows-msvc" -BinName "redragon_m719_pro.exe" -OutName "redragon_m719_pro-windows-x64.exe"

    # 2. Windows 32-bit (x86)
    Build-Target -Target "i686-pc-windows-msvc" -BinName "redragon_m719_pro.exe" -OutName "redragon_m719_pro-windows-x86.exe"
}
elseif ($IsLinux) {
    # 1. Linux 64-bit (x64)
    Build-Target -Target "x86_64-unknown-linux-gnu" -BinName "redragon_m719_pro" -OutName "redragon_m719_pro-linux-x64"

    # 2. Linux 32-bit (x86)
    Build-Target -Target "i686-unknown-linux-gnu" -BinName "redragon_m719_pro" -OutName "redragon_m719_pro-linux-x86"
}
elseif ($IsMacOS) {
    # 1. macOS 64-bit Intel (x64)
    Build-Target -Target "x86_64-apple-darwin" -BinName "redragon_m719_pro" -OutName "redragon_m719_pro-macos-x64"

    # 2. macOS Apple Silicon (arm64)
    Build-Target -Target "aarch64-apple-darwin" -BinName "redragon_m719_pro" -OutName "redragon_m719_pro-macos-arm64"
}

Write-Host "`nAll local platform artifacts generated in: $DistDir" -ForegroundColor Yellow
Get-ChildItem $DistDir | Select-Object Name, Length, LastWriteTime
