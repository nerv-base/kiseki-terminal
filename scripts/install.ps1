# Kiseki Terminal v2 standalone installer for Windows (PowerShell 5.1+ / 7+)
# Installs Kiseki, its optional tools and fonts from a tagged release.
#
# Usage:
#   irm https://kiseki-dl.nerv-base.com/install.ps1 | iex   # China CDN (EdgeOne)
#   irm https://dl.kisekiterm.dev/install.ps1 | iex         # Global (Cloudflare R2)
#   & ([scriptblock]::Create((irm https://dl.kisekiterm.dev/install.ps1))) -Version v2.0.0-alpha.1

<#
.SYNOPSIS
Installs Kiseki Terminal and its dependencies.
.PARAMETER SkipFonts
Skips all four font families for this run.
#>
[CmdletBinding()]
param(
    [string]$Version = $env:KISEKI_VERSION,
    [string]$InstallDir = $(if ($env:KISEKI_INSTALL_DIR) { $env:KISEKI_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\kiseki" }),
    [string]$Repo = $(if ($env:KISEKI_REPO) { $env:KISEKI_REPO } else { "nerv-base/kiseki-terminal" }),
    [string]$DownloadBase = $env:KISEKI_DL_BASE,
    [string]$Mirror = $env:KISEKI_MIRROR,
    [switch]$SkipFonts
)

$ErrorActionPreference = "Stop"
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {}

$okMark = [char]0x2713
$remedy = "Rerun the installer (see https://kisekiterm.dev)."
$cnOrigin = "https://kiseki-dl.nerv-base.com"
$globalOrigin = "https://dl.kisekiterm.dev"
$mirrorOrigins = New-Object System.Collections.Generic.List[string]
function Add-MirrorOrigin([string]$Raw) {
    if (-not $Raw) { return }
    $clean = $Raw.TrimEnd('/') -replace '/releases$', ''
    if ($clean -and -not $mirrorOrigins.Contains($clean)) { $mirrorOrigins.Add($clean) }
}
Add-MirrorOrigin $DownloadBase
$preferCn = ($Mirror -eq 'cn') -or ((-not $Mirror) -and $(try { [TimeZoneInfo]::Local.BaseUtcOffset.Hours -eq 8 } catch { $false }))
if ($preferCn) {
    Add-MirrorOrigin $cnOrigin
    Add-MirrorOrigin $globalOrigin
} else {
    Add-MirrorOrigin $globalOrigin
    Add-MirrorOrigin $cnOrigin
}

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
    param([string]$RequestedVersion, [string]$Repository, [string[]]$Origins)
    if ($RequestedVersion) { return $RequestedVersion }
    foreach ($origin in $Origins) {
        try {
            $meta = Invoke-RestMethod -Uri "$origin/releases/latest.json" -TimeoutSec 8 -Headers @{ "User-Agent" = "kiseki-installer" }
            if ($meta -and $meta.tag -cmatch '^v2\.[0-9A-Za-z.+-]+$') { return $meta.tag }
        } catch { }
    }

    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases" -Headers @{ "User-Agent" = "kiseki-installer" } -TimeoutSec 30
    $v2 = $releases | Where-Object { $_.tag_name -like "v2.*" -and -not $_.draft } | Select-Object -First 1
    if (-not $v2) { throw "No published v2 release found for $Repository." }
    return $v2.tag_name
}

function Invoke-MirroredDownload {
    param([string[]]$CandidateUrls, [int[]]$Timeouts, [string]$OutFile, [string]$Label = "")
    if ($Label) { Write-Host "  Downloading $Label..." -ForegroundColor DarkGray }
    $prevPref = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'
    try {
        for ($i = 0; $i -lt $CandidateUrls.Count; $i++) {
            $u = $CandidateUrls[$i]
            $t = $Timeouts[$i]
            $isLast = ($i -eq ($CandidateUrls.Count - 1))
            try {
                Invoke-WebRequest -Uri $u -OutFile $OutFile -TimeoutSec $t -UseBasicParsing
                return
            } catch {
                if ($isLast) { throw }
            }
        }
    } finally {
        $ProgressPreference = $prevPref
    }
}

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-ReleaseChecksum([string]$Name) {
    $line = Get-Content -LiteralPath $checksumsPath | Where-Object { $_ -match "^[0-9a-f]{64}\s+\*?$([regex]::Escape($Name))$" } | Select-Object -First 1
    if (-not $line) { throw "Checksum entry for '$Name' not found in checksums.txt." }
    return ($line -split '\s+')[0].ToLowerInvariant()
}

function Assert-Sha256([string]$Path, [string]$ExpectedHash) {
    $actualHash = Get-Sha256 $Path
    if ($actualHash -ne $ExpectedHash) { throw "SHA-256 mismatch for ${Path}. Expected $ExpectedHash, got $actualHash." }
}

function Save-Download([string]$Uri, [string]$Path, [string]$Label = "") {
    $asset = ([uri]$Uri).AbsolutePath.Split('/')[-1]
    $urls = @()
    $timeouts = @()
    foreach ($origin in $mirrorOrigins) {
        $urls += "$origin/releases/$tag/$asset"
        $timeouts += 60
    }
    $urls += $Uri
    $timeouts += 300
    Invoke-MirroredDownload -CandidateUrls $urls -Timeouts $timeouts -OutFile $Path -Label $Label
}

function Save-DependencyDownload([string]$Digest, [string]$Uri, [string]$Path, [string]$Label) {
    $urls = @()
    $timeouts = @()
    foreach ($origin in $mirrorOrigins) {
        $urls += "$origin/deps/$Digest"
        $timeouts += 90
    }
    $urls += $Uri
    $timeouts += 300
    Invoke-MirroredDownload -CandidateUrls $urls -Timeouts $timeouts -OutFile $Path -Label $Label
}

# Stage beside the destination and replace atomically so a failed copy leaves the old file usable.
function Install-File([string]$Source, [string]$Destination) {
    if ((Test-Path -LiteralPath $Destination -PathType Leaf) -and (Get-Sha256 $Source) -eq (Get-Sha256 $Destination)) {
        return $false
    }
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($Destination)) -Force | Out-Null
    $staged = "$Destination.tmp.$([Guid]::NewGuid().ToString('N'))"
    try {
        Copy-Item -LiteralPath $Source -Destination $staged
        if (Test-Path -LiteralPath $Destination) { [IO.File]::Replace($staged, $Destination, [NullString]::Value) }
        else { [IO.File]::Move($staged, $Destination) }
    } finally {
        Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
    }
    return $true
}

function Install-Core {
    $archive = "kiseki-$tag-$target.zip"
    $archivePath = Join-Path $tmpDir $archive
    try {
        Save-Download "$baseUrl/checksums.txt" $checksumsPath
        Save-Download "$baseUrl/$archive" $archivePath $archive
    } catch {
        Write-Warning $_.Exception.Message
        return $false
    }
    Assert-Sha256 $archivePath (Get-ReleaseChecksum $archive)
    Write-Host "$okMark SHA-256 verified" -ForegroundColor Green
    $extractDir = Join-Path $tmpDir "extracted"
    Assert-ZipSupport
    [System.IO.Compression.ZipFile]::ExtractToDirectory($archivePath, $extractDir)
    $binPath = Join-Path $extractDir "kiseki-$tag-$target\kiseki.exe"
    if (-not (Test-Path -LiteralPath $binPath -PathType Leaf)) { $binPath = Join-Path $extractDir "kiseki.exe" }
    $verOut = & $binPath --version
    if ($LASTEXITCODE -ne 0) { throw "Extracted binary failed health check (--version exited with $LASTEXITCODE)." }
    Install-File $binPath $destPath | Out-Null
    $receipt = Join-Path $tmpDir "core.receipt"
    Set-Content -LiteralPath $receipt -Value "$tag`t$(Get-Sha256 $binPath)" -Encoding Ascii
    Install-File $receipt $receiptPath | Out-Null
    Write-Host "$okMark Installed to $destPath ($verOut)" -ForegroundColor Green
    return $true
}

function Get-InstalledRelease([string]$RequestedTag) {
    if (-not (Test-Path -LiteralPath $receiptPath -PathType Leaf)) { return }
    $installedTag, $installedHash = (Get-Content -LiteralPath $receiptPath -TotalCount 1) -split "`t", 2
    if ($installedTag -notmatch '^v[0-9A-Za-z.+-]+$' -or ($RequestedTag -and $RequestedTag -ne $installedTag)) { return }
    Assert-Sha256 $destPath $installedHash
    & $destPath --version | Out-Null
    if ($LASTEXITCODE -ne 0) { return }
    Write-Host "$okMark Reusing verified Kiseki $installedTag (release server unavailable)." -ForegroundColor Green
    return $installedTag
}

function Install-Font([string]$Source, [string]$Name, [string]$FontDir) {
    if (-not ('Kiseki.NativeFonts' -as [type])) {
        Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
namespace Kiseki {
    public static class NativeFonts {
        [DllImport("gdi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
        public static extern int AddFontResourceW(string path);
        [DllImport("user32.dll", CharSet = CharSet.Auto)]
        public static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint Msg, System.IntPtr wParam, System.IntPtr lParam, uint fuFlags, uint uTimeout, out System.IntPtr lpdwResult);
    }
}
'@
    }
    $destination = Join-Path $FontDir $Name
    $changed = Install-File $Source $destination
    $key = 'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Fonts'
    $format = if ($Name.EndsWith('.otf')) { 'OpenType' } else { 'TrueType' }
    $displayName = "{0} ({1})" -f [IO.Path]::GetFileNameWithoutExtension($Name), $format
    $existing = Get-ItemProperty -LiteralPath $key -Name $displayName -ErrorAction SilentlyContinue
    if ($changed -or -not $existing -or $existing.$displayName -ne $destination) {
        if ([Kiseki.NativeFonts]::AddFontResourceW($destination) -eq 0) { throw "Could not load font $destination" }
        # Never recreate an existing key: `New-Item -Force` clears every registered per-user font.
        if (-not (Test-Path -LiteralPath $key)) { New-Item -Path $key -Force | Out-Null }
        New-ItemProperty -LiteralPath $key -Name $displayName -Value $destination -PropertyType String -Force | Out-Null
    }
}

function Assert-ZipSupport {
    if (('System.IO.Compression.ZipFile' -as [type]) -and ('System.IO.Compression.ZipFileExtensions' -as [type])) { return }
    try {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        if (-not ('System.IO.Compression.ZipFile' -as [type]) -or -not ('System.IO.Compression.ZipFileExtensions' -as [type])) {
            throw 'ZIP extraction types are unavailable.'
        }
    } catch { throw 'ZIP extraction requires the Windows .NET ZIP components. Repair .NET Framework 4.5 or later, or use PowerShell 7, then rerun the installer.' }
}

function Install-Dependency([string[]]$Row, [hashtable]$FontPatterns, [string]$FontDir) {
    $kind, $null, $uri, $digest, $relative, $mode = $Row
    $assetPath = Join-Path $tmpDir 'dependency.zip'
    if ($kind -eq 'tool' -and $mode -eq '0755' -and $relative -in 'vendor/bin/oh-my-posh.exe', 'vendor/bin/fastfetch.exe') {
        $name = [IO.Path]::GetFileName($relative)
        $destination = Join-Path (Join-Path $InstallDir 'vendor\bin') $name
        if ((Test-Path -LiteralPath $destination -PathType Leaf) -and (Get-Sha256 $destination) -eq $digest) { return }
        if ($name -eq 'fastfetch.exe') { Assert-ZipSupport }
        Save-DependencyDownload $digest $uri $assetPath $relative
        Assert-Sha256 $assetPath $digest
        $source = Join-Path $tmpDir $name
        if ($name -eq 'fastfetch.exe') {
            $zip = [IO.Compression.ZipFile]::OpenRead($assetPath)
            try {
                $entry = $zip.GetEntry($name)
                if (-not $entry) { throw "Fastfetch executable missing from $uri" }
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $source, $true)
            } finally { $zip.Dispose() }
        } else {
            Move-Item -LiteralPath $assetPath -Destination $source -Force
        }
        & $source --version | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "$name failed health check (--version exited with $LASTEXITCODE)." }
        Install-File $source $destination | Out-Null
    } elseif ($kind -eq 'font-raw' -and $mode -eq '0644' -and $relative -cmatch '^fonts/([A-Za-z0-9_-]+\.ttf)$') {
        $name = $Matches[1]
        if (-not $FontDir) { throw "Missing font directory for $name" }
        $destination = Join-Path $FontDir $name
        $source = $destination
        if (-not (Test-Path -LiteralPath $destination -PathType Leaf) -or (Get-Sha256 $destination) -ne $digest) {
            $source = Join-Path $tmpDir 'dependency.ttf'
            Save-DependencyDownload $digest $uri $source $relative
            Assert-Sha256 $source $digest
        }
        Install-Font $source $name $FontDir
        Write-Host "$okMark Font ready: $name" -ForegroundColor Green
    } elseif ($kind -eq 'font' -and $mode -eq '0644' -and $relative -match '^fonts/([a-z0-9-]+)$') {
        $family = $Matches[1]
        $pattern = $FontPatterns[$family]
        if (-not $FontDir -or -not $pattern) { throw "Missing font metadata for $family" }
        Assert-ZipSupport
        Save-DependencyDownload $digest $uri $assetPath $relative
        Assert-Sha256 $assetPath $digest
        $zip = [IO.Compression.ZipFile]::OpenRead($assetPath)
        try {
            $entries = @($zip.Entries | Where-Object { $_.Name -cmatch $pattern })
            if ($entries.Count -eq 0) { throw "No matching faces for $family" }
            foreach ($entry in $entries) {
                if ($entry.Name -match '[\\/:*?"<>|]') { throw "Invalid font file name: $($entry.Name)" }
                $face = Join-Path $tmpDir $entry.Name
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $face, $true)
                Install-Font $face $entry.Name $FontDir
            }
        } finally { $zip.Dispose() }
        Write-Host "$okMark Font family ready: $family" -ForegroundColor Green
    } else { throw "Unexpected dependency destination or mode: $relative $mode" }
}

function Install-Dependencies {
    $manifest = Join-Path $tmpDir 'deps.tsv'
    Save-Download "$baseUrl/deps.tsv" $manifest
    Assert-Sha256 $manifest (Get-ReleaseChecksum 'deps.tsv')
    $depPlatform = if ($target -like 'aarch64-*') { 'windows-arm64' } else { 'windows-amd64' }
    $fontPatterns = @{}
    $fontDir = ''
    $rows = Get-Content -LiteralPath $manifest
    foreach ($line in $rows) {
        $metadata = $line -split "`t"
        if ($metadata[0] -eq '# font') { $fontPatterns[$metadata[1]] = $metadata[2] }
        if ($metadata[0] -eq '# font-dir' -and $metadata[1] -eq 'windows') {
            $fontDir = [Environment]::ExpandEnvironmentVariables($metadata[2])
        }
    }
    foreach ($line in $rows) {
        if ($line.StartsWith('#')) { continue }
        $fields = $line -split "`t"
        if ($fields.Count -ne 6 -or $fields[1] -cne $depPlatform) { continue }
        if ($SkipFonts -and $fields[0] -in 'font', 'font-raw') { continue }
        try { Install-Dependency $fields $fontPatterns $fontDir }
        catch { Write-Warning "Could not install $($fields[4]). $remedy $($_.Exception.Message)" }
    }
    if (-not $SkipFonts -and ('Kiseki.NativeFonts' -as [type])) {
        # Tell already-running terminal hosts that the font set changed (WM_FONTCHANGE).
        $ignored = [IntPtr]::Zero
        try { [Kiseki.NativeFonts]::SendMessageTimeout([IntPtr]0xffff, 0x001D, [IntPtr]::Zero, [IntPtr]::Zero, 2, 1000, [ref]$ignored) | Out-Null } catch { }
    }
}

if ($Version) {
    if (-not $Version.StartsWith('v')) { $Version = "v$Version" }
    if ($Version -notmatch '^v[0-9A-Za-z.+-]+$') { throw "Invalid release tag: $Version" }
}
$target = Get-RustTarget
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$destPath = Join-Path $InstallDir 'kiseki.exe'
$receiptPath = Join-Path $InstallDir '.kiseki-release'
$tmpDir = Join-Path ([IO.Path]::GetTempPath()) ("kiseki-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
$checksumsPath = Join-Path $tmpDir 'checksums.txt'

try {
    Write-Host "Using mirror: $($mirrorOrigins[0] -replace '^https://','') (fallback: GitHub)" -ForegroundColor DarkGray
    $releaseAvailable = $true
    try { $tag = Resolve-ReleaseTag -RequestedVersion $Version -Repository $Repo -Origins $mirrorOrigins }
    catch {
        $tag = Get-InstalledRelease $Version
        if (-not $tag) { throw }
        $releaseAvailable = $false
    }
    if ($tag -notmatch '^v[0-9A-Za-z.+-]+$') { throw "Invalid release tag: $tag" }
    $baseUrl = "https://github.com/$Repo/releases/download/$tag"
    if ($releaseAvailable) {
        Write-Host "Installing Kiseki Terminal $tag ($target)..." -ForegroundColor Cyan
        if (-not (Install-Core) -and -not (Get-InstalledRelease $tag)) { throw "Could not download Kiseki $tag." }
    }

    # Retain a custom vendor root for generated hooks in future sessions.
    $env:KISEKI_INSTALL_DIR = $InstallDir
    if ($InstallDir -ne (Join-Path $env:LOCALAPPDATA 'Programs\kiseki')) {
        [Environment]::SetEnvironmentVariable('KISEKI_INSTALL_DIR', $InstallDir, 'User')
    }
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $segments = @($userPath -split ';' | Where-Object { $_ -ne '' })
    if ($segments -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable('Path', (($segments + $InstallDir) -join ';'), 'User')
        $env:Path = "$env:Path;$InstallDir"
        Write-Host "$okMark Added $InstallDir to User PATH" -ForegroundColor Green
    }
    if (Test-Path -LiteralPath $checksumsPath -PathType Leaf) {
        try { Install-Dependencies }
        catch { Write-Warning "Dependency manifest unavailable. $remedy $($_.Exception.Message)" }
    } else { Write-Warning "Could not reach the release server to check dependencies. $remedy" }

    Write-Host "`nNext step: run 'kiseki init' to launch the interactive setup wizard."
    try {
        & $destPath deps
        if ($LASTEXITCODE -ne 0) { throw "kiseki deps exited with $LASTEXITCODE" }
    } catch { Write-Warning 'Could not read the capability list. Run kiseki deps to retry.' }
    if ($SkipFonts) { Write-Host 'Fonts were skipped for this run (-SkipFonts).' }
    $global:LASTEXITCODE = 0
} finally {
    Remove-Item -LiteralPath $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
