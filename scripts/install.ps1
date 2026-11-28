# Kiseki Terminal v2 standalone installer for Windows (PowerShell 5.1+ / 7+)
# Stage 1: Downloads, SHA-256-verifies and installs `kiseki.exe` with a live progress bar.
# Stage 2: Hands off parallel tool & font installation to `kiseki.exe deps install`.
#
# Usage:
#   irm https://kisekiterm.dev/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://kisekiterm.dev/install.ps1))) -Version v2.0.0-beta.1

<#
.SYNOPSIS
Installs Kiseki Terminal and its dependencies.
.PARAMETER SkipFonts
Skips all four font families for this run.
.PARAMETER Concurrency
Maximum parallel dependency downloads (1-8, default 4).
#>
[CmdletBinding()]
param(
    [string]$Version = $env:KISEKI_VERSION,
    [string]$InstallDir = $(if ($env:KISEKI_INSTALL_DIR) { $env:KISEKI_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\kiseki" }),
    [string]$Repo = $(if ($env:KISEKI_REPO) { $env:KISEKI_REPO } else { "nerv-base/kiseki-terminal" }),
    [string]$DownloadBase = $env:KISEKI_DL_BASE,
    [string]$Mirror = $env:KISEKI_MIRROR,
    [switch]$SkipFonts,
    [int]$Concurrency = 0
)

$ErrorActionPreference = "Stop"
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {}

$okMark = [char]0x2713
$remedy = "Run 'kiseki deps install' or rerun the installer (see https://kisekiterm.dev)."
$cnOrigin = "https://kiseki-dl.nerv-base.com"
$globalOrigin = "https://dl.kisekiterm.dev"
$mirrorOrigins = New-Object System.Collections.Generic.List[string]

function Test-LoopbackOrigin([string]$Origin) {
    $rest = $Origin -replace '^https?://', ''
    return ($rest -match '^(127\.0\.0\.1|localhost|\[::1\])')
}

function Add-MirrorOrigin([string]$Raw) {
    if (-not $Raw) { return }
    foreach ($part in ($Raw -split ',')) {
        $clean = $part.Trim().TrimEnd('/') -replace '/releases$', ''
        if ($clean -and -not $mirrorOrigins.Contains($clean)) { $mirrorOrigins.Add($clean) }
    }
}

Add-MirrorOrigin $DownloadBase
$loopbackOnly = ($mirrorOrigins.Count -gt 0) -and (@($mirrorOrigins | Where-Object { -not (Test-LoopbackOrigin $_) }).Count -eq 0)
if (-not $loopbackOnly) {
    $preferCn = ($Mirror -eq 'cn') -or ((-not $Mirror) -and $(try { [TimeZoneInfo]::Local.BaseUtcOffset.Hours -eq 8 } catch { $false }))
    if ($preferCn) {
        Add-MirrorOrigin $cnOrigin
        Add-MirrorOrigin $globalOrigin
    } else {
        Add-MirrorOrigin $globalOrigin
        Add-MirrorOrigin $cnOrigin
    }
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
    param([string]$RequestedVersion, [string]$Repository, [string[]]$Origins, [bool]$LocalOnly)
    if ($RequestedVersion) { return $RequestedVersion }
    foreach ($origin in $Origins) {
        try {
            $meta = Invoke-RestMethod -Uri "$origin/releases/latest.json" -TimeoutSec 8 -Headers @{ "User-Agent" = "kiseki-installer" }
            if ($meta -and $meta.tag -cmatch '^v2\.[0-9A-Za-z.+-]+$') { return $meta.tag }
        } catch { }
    }
    if ($LocalOnly) { throw "Could not resolve release tag from configured local mirror." }

    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases" -Headers @{ "User-Agent" = "kiseki-installer" } -TimeoutSec 30
    $v2 = $releases | Where-Object { $_.tag_name -like "v2.*" -and -not $_.draft } | Select-Object -First 1
    if (-not $v2) { throw "No published v2 release found for $Repository." }
    return $v2.tag_name
}

function Format-Size([long]$Bytes) {
    if ($Bytes -ge 1048576) { return "{0:N1} MB" -f ($Bytes / 1048576.0) }
    if ($Bytes -ge 1024) { return "{0:N1} KB" -f ($Bytes / 1024.0) }
    return "$Bytes B"
}

function Invoke-StreamDownload {
    param(
        [string]$Uri,
        [string]$OutFile,
        [int]$ConnectTimeoutMs = 10000,
        [int]$ReadTimeoutMs = 30000,
        [switch]$ShowProgress
    )
    $req = [System.Net.HttpWebRequest]::Create($Uri)
    $req.Method = "GET"
    $req.UserAgent = "kiseki-installer"
    $req.AllowAutoRedirect = $true
    $req.Timeout = $ConnectTimeoutMs
    $req.ReadWriteTimeout = $ReadTimeoutMs

    $resp = $null
    $inStream = $null
    $outStream = $null
    $interactive = $ShowProgress -and $(try { -not [Console]::IsOutputRedirected } catch { $false })
    try {
        $resp = $req.GetResponse()
        $total = $resp.ContentLength
        $inStream = $resp.GetResponseStream()
        $outStream = [System.IO.File]::Create($OutFile)
        $buffer = New-Object byte[] 65536
        [long]$received = 0
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $lastRenderMs = -200
        while (($read = $inStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
            $outStream.Write($buffer, 0, $read)
            $received += $read
            if ($interactive -and (($sw.ElapsedMilliseconds - $lastRenderMs) -ge 100)) {
                $lastRenderMs = $sw.ElapsedMilliseconds
                $secs = [Math]::Max(0.001, $sw.Elapsed.TotalSeconds)
                $mbps = ($received / 1048576.0) / $secs
                if ($total -gt 0) {
                    $ratio = [Math]::Min(1.0, [double]$received / [double]$total)
                    $filled = [int][Math]::Round($ratio * 20)
                    $bar = (([string][char]0x2588) * $filled) + (([string][char]0x2591) * (20 - $filled))
                    $pct = [int][Math]::Round($ratio * 100)
                    $line = ("`r  [{0}] {1,3}%  {2,8} / {3,-8}  {4:N1} MB/s   " -f $bar, $pct, (Format-Size $received), (Format-Size $total), $mbps)
                } else {
                    $line = ("`r  Downloading {0} ({1:N1} MB/s)   " -f (Format-Size $received), $mbps)
                }
                [Console]::Write($line)
            }
        }
        $outStream.Flush()
        if ($interactive) {
            $secs = [Math]::Max(0.001, $sw.Elapsed.TotalSeconds)
            $mbps = ($received / 1048576.0) / $secs
            $bar = ([string][char]0x2588) * 20
            [Console]::WriteLine(("`r  [{0}] 100%  {1}  {2:N1} MB/s         " -f $bar, (Format-Size $received), $mbps))
        }
    } finally {
        if ($outStream) { $outStream.Dispose() }
        if ($inStream) { $inStream.Dispose() }
        if ($resp) { $resp.Close() }
    }
}

function Invoke-MirroredDownload {
    param([string[]]$CandidateUrls, [string]$OutFile, [string]$Label = "", [switch]$ShowProgress)
    if ($Label) { Write-Host "  Downloading $Label..." -ForegroundColor DarkGray }
    for ($i = 0; $i -lt $CandidateUrls.Count; $i++) {
        $u = $CandidateUrls[$i]
        $isLast = ($i -eq ($CandidateUrls.Count - 1))
        try {
            Invoke-StreamDownload -Uri $u -OutFile $OutFile -ShowProgress:$ShowProgress
            return
        } catch {
            Remove-Item -LiteralPath $OutFile -Force -ErrorAction SilentlyContinue
            if ($isLast) { throw }
        }
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

function Save-Download([string]$Uri, [string]$Path, [string]$Label = "", [switch]$ShowProgress) {
    $asset = ([uri]$Uri).AbsolutePath.Split('/')[-1]
    $urls = @()
    foreach ($origin in $mirrorOrigins) {
        $urls += "$origin/releases/$tag/$asset"
    }
    if (-not $loopbackOnly) {
        $urls += $Uri
    }
    Invoke-MirroredDownload -CandidateUrls $urls -OutFile $Path -Label $Label -ShowProgress:$ShowProgress
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

function Assert-ZipSupport {
    if (('System.IO.Compression.ZipFile' -as [type]) -and ('System.IO.Compression.ZipFileExtensions' -as [type])) { return }
    try {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        if (-not ('System.IO.Compression.ZipFile' -as [type]) -or -not ('System.IO.Compression.ZipFileExtensions' -as [type])) {
            throw 'ZIP extraction types are unavailable.'
        }
    } catch { throw 'ZIP extraction requires the Windows .NET ZIP components. Repair .NET Framework 4.5 or later, or use PowerShell 7, then rerun the installer.' }
}

function Install-Core {
    $archive = "kiseki-$tag-$target.zip"
    $archivePath = Join-Path $tmpDir $archive
    try {
        Save-Download "$baseUrl/checksums.txt" $checksumsPath
        Save-Download "$baseUrl/$archive" $archivePath $archive -ShowProgress
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

function Invoke-Stage2Deps {
    $depArgs = @('deps', 'install', '--version', $tag, '--repo', $Repo)
    if (Test-Path -LiteralPath $checksumsPath -PathType Leaf) {
        $depArgs += @('--checksums', $checksumsPath)
    }
    if ($Mirror) {
        $depArgs += @('--mirror', $Mirror)
    }
    if ($DownloadBase) {
        $depArgs += @('--dl-base', $DownloadBase)
    }
    if ($SkipFonts) {
        $depArgs += '--skip-fonts'
    }
    if ($Concurrency -gt 0) {
        $depArgs += @('--concurrency', "$Concurrency")
    }
    & $destPath @depArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "Some dependencies could not be installed. $remedy"
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
    Write-Host "Using mirror: $($mirrorOrigins[0] -replace '^https?://','') (fallback: GitHub)" -ForegroundColor DarkGray
    $releaseAvailable = $true
    try { $tag = Resolve-ReleaseTag -RequestedVersion $Version -Repository $Repo -Origins $mirrorOrigins -LocalOnly $loopbackOnly }
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

    if ($releaseAvailable) {
        try { Invoke-Stage2Deps }
        catch { Write-Warning "Dependency installation encountered an error. $remedy $($_.Exception.Message)" }
    } else {
        Write-Warning "Could not reach the release server to check dependencies. $remedy"
    }

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
