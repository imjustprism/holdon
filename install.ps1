# Usage:
#   iwr https://raw.githubusercontent.com/imjustprism/holdon/main/install.ps1 -UseBasicParsing | iex
#   $env:HOLDON_VERSION = "v0.1.0"; $env:INSTALL_DIR = "$HOME\bin"; iwr ... | iex

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$Repo = 'imjustprism/holdon'
$Version = if ($env:HOLDON_VERSION) { $env:HOLDON_VERSION } else { 'latest' }
$InstallDir = if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'holdon\bin' }

function Resolve-Version {
    param([string]$Version)
    if ($Version -ne 'latest') { return $Version }
    $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
    if (-not $rel.tag_name) { throw 'could not resolve latest version' }
    return $rel.tag_name
}

function Get-Target {
    $arch = (Get-CimInstance Win32_Processor).Architecture
    switch ($arch) {
        9 { return 'x86_64-pc-windows-msvc' }
        12 { throw 'install: aarch64 Windows release not built yet' }
        default { throw "install: unsupported architecture code $arch" }
    }
}

$Version = Resolve-Version $Version
$Target = Get-Target
$Archive = "holdon-$Target.zip"
$Base = "https://github.com/$Repo/releases/download/$Version"

Write-Host "install: fetching holdon $Version for $Target"

$Tmp = New-Item -ItemType Directory -Path (Join-Path $env:TEMP "holdon-install-$([Guid]::NewGuid())") -Force
try {
    $archivePath = Join-Path $Tmp $Archive
    $sumsPath = Join-Path $Tmp 'SHA256SUMS'
    Invoke-WebRequest -Uri "$Base/$Archive" -OutFile $archivePath -UseBasicParsing
    Invoke-WebRequest -Uri "$Base/SHA256SUMS" -OutFile $sumsPath -UseBasicParsing

    $expectedLine = Select-String -Path $sumsPath -Pattern " $Archive`$" -SimpleMatch:$false | Select-Object -First 1
    if (-not $expectedLine) { throw "$Archive missing from SHA256SUMS" }
    $expected = ($expectedLine.Line -split '\s+')[0]
    $actual = (Get-FileHash -Algorithm SHA256 -Path $archivePath).Hash.ToLowerInvariant()
    if ($expected.ToLowerInvariant() -ne $actual) {
        throw "checksum mismatch for $Archive"
    }

    Expand-Archive -Path $archivePath -DestinationPath $Tmp -Force
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path (Join-Path $Tmp "holdon-$Target\holdon.exe") -Destination (Join-Path $InstallDir 'holdon.exe') -Force

    Write-Host "install: holdon installed to $InstallDir\holdon.exe"

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not ($userPath -split ';' | Where-Object { $_ -eq $InstallDir })) {
        Write-Host "install: add $InstallDir to PATH"
        Write-Host "  setx PATH `"`$env:Path;$InstallDir`""
    }
}
finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
