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

    # SHA256SUMS.sig and SHA256SUMS.pem come from cosign sign-blob in
    # release.yml. If the user has cosign installed, verify the bundle
    # before trusting any hash in the file. Without cosign we still get
    # TLS to github.com but no cryptographic proof the file matches a
    # release that ran through the published workflow.
    if (Get-Command cosign -ErrorAction SilentlyContinue) {
        $sigPath = Join-Path $Tmp 'SHA256SUMS.sig'
        $pemPath = Join-Path $Tmp 'SHA256SUMS.pem'
        Invoke-WebRequest -Uri "$Base/SHA256SUMS.sig" -OutFile $sigPath -UseBasicParsing
        Invoke-WebRequest -Uri "$Base/SHA256SUMS.pem" -OutFile $pemPath -UseBasicParsing
        $identity = "https://github.com/$Repo/.github/workflows/release.yml@refs/tags/v.*"
        & cosign verify-blob `
            --certificate $pemPath `
            --signature $sigPath `
            --certificate-identity-regexp $identity `
            --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' `
            $sumsPath 2>$null | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw 'cosign signature verification failed for SHA256SUMS'
        }
        Write-Host 'install: SHA256SUMS cosign signature verified'
    } else {
        Write-Warning 'install: cosign not found, skipping signature verification (install cosign for stronger supply-chain checks)'
    }

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
