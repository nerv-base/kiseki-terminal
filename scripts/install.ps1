# Kiseki Terminal v2 standalone installer for Windows (PowerShell 5.1+ / 7+)
# Downloads a tagged GitHub Release zip, verifies SHA-256 against checksums.txt,
# installs `kiseki.exe` to %LOCALAPPDATA%\Programs\kiseki, and updates User PATH.
#
# Usage:
#   irm https://kisekiterm.dev/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://kisekiterm.dev/install.ps1))) -Version v2.0.0-alpha.1

[CmdletBinding()]
param(
    [string]$Version = $env:KISEKI_VERSION,
    [string]$InstallDir = $(if ($env:KISEKI_INSTALL_DIR) { $env:KISEKI_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\kiseki" }),
    [string]$Repo = $(if ($env:KISEKI_REPO) { $env:KISEKI_REPO } else { "nerv-base/kiseki-terminal" }),
    [string]$DownloadBase = $(if ($env:KISEKI_DL_BASE) { $env:KISEKI_DL_BASE } else { "https://dl.kisekiterm.dev/releases" })
)

$ErrorActionPreference = "Stop"
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {}

$okMark = [char]0x2713

function Get-RustTarget {
    $arch = $null
    try {
        $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    } catch {
        $arch = $null
    }
    if (-not $arch) {
        $raw = $env:PROCESSOR_ARCHITEW6432
        if (-not $raw) { $raw = $env:PROCESSOR_ARCHITECTURE }
        switch ($raw.ToUpperInvariant()) {
            "AMD64" { $arch = "x64" }
            "ARM64" { $arch = "arm64" }
            default { $arch = $raw.ToLowerInvariant() }
        }
    }
    switch ($arch) {
        "x64"   { return "x86_64-pc-windows-msvc" }
        "arm64" { return "aarch64-pc-windows-msvc" }
        default { throw "Unsupported Windows architecture: $arch" }
    }
}

function Resolve-ReleaseTag {
    param([string]$RequestedVersion, [string]$Repository, [string]$DlBase)
    if ($RequestedVersion) {
        if ($RequestedVersion.StartsWith("v")) { return $RequestedVersion }
        return "v$RequestedVersion"
    }

    try {
        $meta = Invoke-RestMethod -Uri "$DlBase/latest.json" -TimeoutSec 5 -Headers @{ "User-Agent" = "kiseki-installer" }
        if ($meta -and $meta.tag) { return $meta.tag }
    } catch { }

    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases" -Headers @{ "User-Agent" = "kiseki-installer" }
    $v2 = $releases | Where-Object { $_.tag_name -like "v2.*" -and -not $_.draft } | Select-Object -First 1
    if (-not $v2) {
        throw "No published v2 release found for $Repository. Pass -Version v2.0.0-alpha.1 once published."
    }
    return $v2.tag_name
}

function Download-ReleaseAsset {
    param([string]$PrimaryUrl, [string]$FallbackUrl, [string]$OutFile)
    try {
        Invoke-WebRequest -Uri $PrimaryUrl -OutFile $OutFile -TimeoutSec 15 -UseBasicParsing
        return
    } catch { }
    Invoke-WebRequest -Uri $FallbackUrl -OutFile $OutFile -UseBasicParsing
}

$target = Get-RustTarget
$tag = Resolve-ReleaseTag -RequestedVersion $Version -Repository $Repo -DlBase $DownloadBase
$archive = "kiseki-$tag-$target.zip"
$r2Url = "$DownloadBase/$tag"
$ghUrl = "https://github.com/$Repo/releases/download/$tag"

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("kiseki-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

try {
    Write-Host "Installing Kiseki Terminal $tag ($target)..." -ForegroundColor Cyan

    $archivePath = Join-Path $tmpDir $archive
    $checksumsPath = Join-Path $tmpDir "checksums.txt"

    Download-ReleaseAsset -PrimaryUrl "$r2Url/$archive" -FallbackUrl "$ghUrl/$archive" -OutFile $archivePath
    Download-ReleaseAsset -PrimaryUrl "$r2Url/checksums.txt" -FallbackUrl "$ghUrl/checksums.txt" -OutFile $checksumsPath

    $expectedLine = Get-Content $checksumsPath | Where-Object { $_ -match "\s+\*?$([regex]::Escape($archive))$" } | Select-Object -First 1
    if (-not $expectedLine) {
        throw "Checksum entry for '$archive' not found in checksums.txt."
    }
    $expectedHash = ($expectedLine -split '\s+')[0].ToLowerInvariant()
    $actualHash = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()

    if ($actualHash -ne $expectedHash) {
        throw "SHA-256 mismatch for $archive!`n  expected: $expectedHash`n  actual:   $actualHash"
    }
    Write-Host "$okMark SHA-256 verified" -ForegroundColor Green

    $extractDir = Join-Path $tmpDir "extracted"
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($archivePath, $extractDir)

    $binPath = Join-Path $extractDir "kiseki-$tag-$target\kiseki.exe"
    if (-not (Test-Path $binPath)) {
        $binPath = Join-Path $extractDir "kiseki.exe"
    }
    if (-not (Test-Path $binPath)) {
        throw "kiseki.exe not found inside archive."
    }

    # Verify extracted binary runs cleanly before replacing
    $verOut = & $binPath --version
    if ($LASTEXITCODE -ne 0) {
        throw "Extracted binary failed health check (--version exited with $LASTEXITCODE)."
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $destPath = Join-Path $InstallDir "kiseki.exe"
    $backupPath = "$destPath.old"

    if (Test-Path $backupPath) { Remove-Item $backupPath -Force -ErrorAction SilentlyContinue }
    if (Test-Path $destPath) { Move-Item -Path $destPath -Destination $backupPath -Force }
    try {
        Copy-Item -Path $binPath -Destination $destPath -Force
        Remove-Item $backupPath -Force -ErrorAction SilentlyContinue
    } catch {
        if (Test-Path $backupPath) { Move-Item -Path $backupPath -Destination $destPath -Force }
        throw
    }

    # Ensure InstallDir is on User PATH
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $segments = @()
    if ($userPath) { $segments = $userPath.Split(';') | Where-Object { $_ -ne "" } }
    if ($segments -notcontains $InstallDir) {
        $newPath = (@($segments) + $InstallDir) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        $env:Path = "$env:Path;$InstallDir"
        Write-Host "$okMark Added $InstallDir to User PATH" -ForegroundColor Green
    }

    Write-Host "$okMark Installed to $destPath ($verOut)" -ForegroundColor Green
    Write-Host ""
    Write-Host "Next step: run 'kiseki init' to launch the interactive setup wizard."
} finally {
    Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
