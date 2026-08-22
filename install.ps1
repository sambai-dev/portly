# Portly installer for Windows — downloads the latest release binary.
#
#   irm https://raw.githubusercontent.com/sambai-dev/portly/main/install.ps1 | iex
#
# Installs to $env:LOCALAPPDATA\Programs\portly (override: PORTLY_INSTALL_DIR).

$ErrorActionPreference = "Stop"

$Repo = "sambai-dev/portly"
$BinName = "portly"
$Target = "x86_64-pc-windows-msvc"
$Asset = "$BinName-$Target.zip"

$InstallDir = if ($env:PORTLY_INSTALL_DIR) {
    $env:PORTLY_INSTALL_DIR
} else {
    Join-Path $env:LOCALAPPDATA "Programs\$BinName"
}

$tmp = New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP ("portly-" + [guid]::NewGuid().ToString("N")))
try {
    $url = "https://github.com/$Repo/releases/latest/download/$Asset"
    Write-Host "downloading $url"
    Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp $Asset) -RetryIntervalSec 2

    Expand-Archive -Path (Join-Path $tmp $Asset) -DestinationPath $tmp -Force

    $src = Join-Path $tmp "$BinName.exe"
    if (-not (Test-Path $src)) { throw "archive did not contain $BinName.exe" }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item $src (Join-Path $InstallDir "$BinName.exe") -Force

    Write-Host "installed $(Join-Path $InstallDir "$BinName.exe")"
    & (Join-Path $InstallDir "$BinName.exe") --version

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
        Write-Host "added $InstallDir to your user PATH (restart your shell)"
    }
}
finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
