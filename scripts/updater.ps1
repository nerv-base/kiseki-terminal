# ============================================================
# ★ Kiseki Terminal — Zero-Lag Update Checker & Self-Updater
# ============================================================

$script:KisekiRepoSlug        = 'nerv-base/kiseki-terminal'
$script:KisekiVersionFile     = Join-Path $script:KisekiRoot 'version.json'
$script:KisekiUpdateCacheFile = Join-Path $script:KisekiRoot 'update-cache.json'
$script:KisekiUpdateOptFile   = Join-Path $script:KisekiRoot 'update-check.txt'
$script:KisekiUpdateAttempt   = Join-Path $script:KisekiRoot 'update-last-attempt.txt'
$script:KisekiRemoteManifest  = "https://raw.githubusercontent.com/$($script:KisekiRepoSlug)/main/version.json"
$script:KisekiReleasesUrl     = "https://github.com/$($script:KisekiRepoSlug)/releases"

function Write-KisekiUtf8NoBom {
    param([string]$Path, [string]$Content)
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function ConvertTo-KisekiVersion {
    param([string]$Raw)
    if (-not $Raw) { return [version]'0.0.0' }
    $clean = ($Raw.Trim() -replace '^v', '') -replace '-.*$', ''
    $v = $null
    if ([version]::TryParse($clean, [ref]$v)) { return $v }
    return [version]'0.0.0'
}

function Get-KisekiLocalVersionInfo {
    if (Test-Path $script:KisekiVersionFile) {
        try {
            return (Get-Content $script:KisekiVersionFile -Raw -Encoding utf8 | ConvertFrom-Json)
        } catch {}
    }
    return [PSCustomObject]@{
        version    = '0.0.0'
        tag        = 'v0.0.0'
        releasedAt = ''
        changelog  = ''
        themes     = @()
    }
}

function Get-KisekiUpdateCheckEnabled {
    if ($env:KISEKI_NO_UPDATE_CHECK -eq '1') { return $false }
    if (Test-Path $script:KisekiUpdateOptFile) {
        return ((Get-Content $script:KisekiUpdateOptFile -Raw).Trim() -ne 'off')
    }
    return $true
}

function Start-KisekiBackgroundUpdateCheck {
    <# .SYNOPSIS Non-blocking 24h background check against raw GitHub version.json (zero prompt lag) #>
    if (-not (Get-KisekiUpdateCheckEnabled)) { return }

    $nowUtc = [DateTime]::UtcNow

    # Avoid hammering network on every tab if offline (15-min backoff on recent attempts)
    if (Test-Path $script:KisekiUpdateAttempt) {
        try {
            $lastTry = [DateTime]::Parse((Get-Content $script:KisekiUpdateAttempt -Raw).Trim(), $null, [System.Globalization.DateTimeStyles]::RoundtripKind)
            if (($nowUtc - $lastTry).TotalMinutes -lt 15) { return }
        } catch {}
    }

    # Skip if cache is fresher than 24 hours
    if (Test-Path $script:KisekiUpdateCacheFile) {
        try {
            $cache = Get-Content $script:KisekiUpdateCacheFile -Raw -Encoding utf8 | ConvertFrom-Json
            if ($cache.checkedAtUtc) {
                $checkedAt = [DateTime]::Parse($cache.checkedAtUtc, $null, [System.Globalization.DateTimeStyles]::RoundtripKind)
                if (($nowUtc - $checkedAt).TotalHours -lt 24) { return }
            }
        } catch {}
    }

    try {
        Write-KisekiUtf8NoBom -Path $script:KisekiUpdateAttempt -Content $nowUtc.ToString('o')
    } catch { return }

    # Lightweight async Runspace on ThreadPool (~2ms foreground overhead, PS 5.1 & PS 7 compatible)
    try {
        $ps = [powershell]::Create()
        [void]$ps.AddScript({
            param($ManifestUrl, $CacheFile, $ReleasesUrl)
            try {
                [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
                $headers = @{ 'User-Agent' = 'KisekiTerminalUpdater'; 'Cache-Control' = 'no-cache' }
                $remote = Invoke-RestMethod -Uri $ManifestUrl -Headers $headers -TimeoutSec 4 -UseBasicParsing
                if ($remote -and $remote.version) {
                    $payload = [PSCustomObject]@{
                        checkedAtUtc  = [DateTime]::UtcNow.ToString('o')
                        remoteVersion = [string]$remote.version
                        remoteTag     = if ($remote.tag) { [string]$remote.tag } else { "v$($remote.version)" }
                        changelog     = [string]$remote.changelog
                        themes        = @($remote.themes)
                        releaseUrl    = if ($remote.tag) { "$ReleasesUrl/tag/$($remote.tag)" } else { $ReleasesUrl }
                    }
                    $tmp = "$CacheFile.tmp"
                    [System.IO.File]::WriteAllText($tmp, ($payload | ConvertTo-Json -Depth 6), [System.Text.UTF8Encoding]::new($false))
                    Move-Item -Path $tmp -Destination $CacheFile -Force
                }
            } catch {
                # Silently ignore offline / DNS / timeout failures
            }
        }).AddArgument($script:KisekiRemoteManifest).AddArgument($script:KisekiUpdateCacheFile).AddArgument($script:KisekiReleasesUrl)
        $script:KisekiBgCheckPs     = $ps
        $script:KisekiBgCheckHandle = $ps.BeginInvoke()
    } catch {}
}

function Show-KisekiUpdateNotice {
    <# .SYNOPSIS Instant cache-only notice on terminal startup; triggers 24h async refresh in background #>
    if (-not (Get-KisekiUpdateCheckEnabled)) { return }

    if (Test-Path $script:KisekiUpdateCacheFile) {
        try {
            $local   = Get-KisekiLocalVersionInfo
            $cache   = Get-Content $script:KisekiUpdateCacheFile -Raw -Encoding utf8 | ConvertFrom-Json
            $vLocal  = ConvertTo-KisekiVersion $local.version
            $vRemote = ConvertTo-KisekiVersion $cache.remoteVersion
            if ($vRemote -gt $vLocal) {
                Write-Host ("  ✦ Kiseki Terminal v{0} available (current v{1})" -f $cache.remoteVersion, $local.version) -ForegroundColor Yellow -NoNewline
                Write-Host "  → run " -ForegroundColor DarkGray -NoNewline
                Write-Host "theme update" -ForegroundColor Cyan
                if ($cache.changelog) {
                    Write-Host ("    {0}" -f $cache.changelog) -ForegroundColor DarkGray
                }
            }
        } catch {}
    }

    Start-KisekiBackgroundUpdateCheck
}

function Update-KisekiTerminal {
    <#
    .SYNOPSIS
        Check for Kiseki Terminal updates or upgrade themes, shaders, and scripts in-place.
    .EXAMPLE
        theme update            # Check & upgrade if a newer version is available
        theme update check      # Only check for updates without modifying files
        theme update force      # Re-download & refresh current release even if up to date
        theme update edge       # Pull latest main branch instead of tagged release
        theme update mute       # Disable automatic startup update notices
        theme update unmute     # Re-enable automatic startup update notices
    #>
    [CmdletBinding()]
    param(
        [switch]$Check,
        [switch]$Force,
        [switch]$Edge,
        [switch]$Mute,
        [switch]$Unmute
    )

    if ($Mute) {
        Write-KisekiUtf8NoBom -Path $script:KisekiUpdateOptFile -Content 'off'
        Write-Host "✦ Auto update check: OFF (run 'theme update check' anytime to check manually)" -ForegroundColor Cyan
        return
    }
    if ($Unmute) {
        Write-KisekiUtf8NoBom -Path $script:KisekiUpdateOptFile -Content 'on'
        Write-Host "✦ Auto update check: ON (checks silently in background every 24h)" -ForegroundColor Cyan
        return
    }

    $local  = Get-KisekiLocalVersionInfo
    $vLocal = ConvertTo-KisekiVersion $local.version

    Write-Host "✦ Checking Kiseki Terminal updates (current: v$($local.version))..." -ForegroundColor Cyan
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $headers = @{ 'User-Agent' = 'KisekiTerminalUpdater'; 'Cache-Control' = 'no-cache' }
        $remote = Invoke-RestMethod -Uri $script:KisekiRemoteManifest -Headers $headers -TimeoutSec 10 -UseBasicParsing
    } catch {
        Write-Host "✖ Could not reach GitHub: $($_.Exception.Message)" -ForegroundColor Red
        return
    }

    if (-not $remote -or -not $remote.version) {
        Write-Host "✖ Remote version manifest is invalid." -ForegroundColor Red
        return
    }

    $remoteTag  = if ($remote.tag) { [string]$remote.tag } else { "v$($remote.version)" }
    $releaseUrl = "$($script:KisekiReleasesUrl)/tag/$remoteTag"
    $vRemote    = ConvertTo-KisekiVersion $remote.version

    # Refresh local cache immediately
    $cachePayload = [PSCustomObject]@{
        checkedAtUtc  = [DateTime]::UtcNow.ToString('o')
        remoteVersion = [string]$remote.version
        remoteTag     = $remoteTag
        changelog     = [string]$remote.changelog
        themes        = @($remote.themes)
        releaseUrl    = $releaseUrl
    }
    Write-KisekiUtf8NoBom -Path $script:KisekiUpdateCacheFile -Content ($cachePayload | ConvertTo-Json -Depth 6)
    Write-KisekiUtf8NoBom -Path $script:KisekiUpdateAttempt -Content ([DateTime]::UtcNow.ToString('o'))

    $hasUpdate = ($vRemote -gt $vLocal)
    if ($Check) {
        if ($hasUpdate) {
            Write-Host ("✦ Update available: v{0} → v{1}" -f $local.version, $remote.version) -ForegroundColor Yellow
            if ($remote.changelog) { Write-Host ("  Notes: {0}" -f $remote.changelog) -ForegroundColor Gray }
            Write-Host ("  Release: {0}" -f $releaseUrl) -ForegroundColor DarkGray
            Write-Host "  Run 'theme update' to upgrade in-place (keeps your active theme & live mode)." -ForegroundColor Cyan
        } else {
            Write-Host ("✓ Already up to date (v{0})." -f $local.version) -ForegroundColor Green
            Write-Host ("  Release: {0}" -f $releaseUrl) -ForegroundColor DarkGray
        }
        return
    }

    if (-not $hasUpdate -and -not $Force -and -not $Edge) {
        Write-Host ("✓ Already up to date (v{0}). Use 'theme update force' to refresh files anyway." -f $local.version) -ForegroundColor Green
        return
    }

    $oldThemes = @(Get-ChildItem (Join-Path $script:KisekiRoot 'themes') -Directory -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
    $tmpDir = Join-Path $env:TEMP ("kiseki-upd-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
    $prevProg = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'

    try {
        $zipPath = Join-Path $tmpDir 'release.zip'
        $urls = if ($Edge) {
            @("https://github.com/$($script:KisekiRepoSlug)/archive/refs/heads/main.zip")
        } else {
            @(
                "https://github.com/$($script:KisekiRepoSlug)/archive/refs/tags/$remoteTag.zip",
                "https://github.com/$($script:KisekiRepoSlug)/archive/refs/heads/main.zip"
            )
        }

        $downloaded = $false
        foreach ($u in $urls) {
            try {
                Write-Host "  Downloading $u ..." -ForegroundColor DarkGray
                Invoke-WebRequest -Uri $u -OutFile $zipPath -TimeoutSec 30 -UseBasicParsing
                $downloaded = $true
                break
            } catch {
                continue
            }
        }
        if (-not $downloaded) {
            throw "Failed to download release archive from GitHub."
        }

        Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force
        $extracted = (Get-ChildItem $tmpDir -Directory | Select-Object -First 1).FullName
        if (-not $extracted -or -not (Test-Path (Join-Path $extracted 'themes'))) {
            throw "Downloaded archive does not contain a valid Kiseki Terminal layout."
        }

        # Update themes, shaders, scripts, and version.json — user state files stay intact!
        foreach ($sub in @('themes', 'shaders', 'scripts')) {
            $srcSub = Join-Path $extracted $sub
            $dstSub = Join-Path $script:KisekiRoot $sub
            if (Test-Path $srcSub) {
                if (-not (Test-Path $dstSub)) { New-Item -ItemType Directory -Path $dstSub -Force | Out-Null }
                Copy-Item -Path (Join-Path $srcSub '*') -Destination $dstSub -Recurse -Force
            }
        }
        if (Test-Path (Join-Path $extracted 'version.json')) {
            Copy-Item (Join-Path $extracted 'version.json') $script:KisekiVersionFile -Force
        }

        # Hot-reload updated engine & re-apply active theme + Windows Terminal config
        $activeTheme = Get-KisekiActiveThemeId
        . (Join-Path $script:KisekiRoot 'scripts\switch-theme.ps1')
        Sync-KisekiWindowsTerminal -ThemeId $activeTheme
        Apply-KisekiShellTheme -ThemeId $activeTheme

        $newThemes = @(Get-ChildItem (Join-Path $script:KisekiRoot 'themes') -Directory -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
        $added = @($newThemes | Where-Object { $oldThemes -notcontains $_ })

        $label = if ($Edge) { "v$($remote.version) (main)" } else { "v$($remote.version)" }
        Write-Host "✦ Updated Kiseki Terminal to $label!" -ForegroundColor Green
        if ($remote.changelog) {
            Write-Host "  Changelog: $($remote.changelog)" -ForegroundColor Gray
        }
        if ($added.Count -gt 0) {
            Write-Host ("  New themes added: {0}  (try: theme {1})" -f ($added -join ', '), $added[0]) -ForegroundColor Magenta
        }
        Write-Host "  Active theme '$activeTheme' refreshed. Open a new tab if shaders updated." -ForegroundColor DarkGray
    } catch {
        Write-Host "✖ Update failed: $($_.Exception.Message)" -ForegroundColor Red
    } finally {
        $ProgressPreference = $prevProg
        if (Test-Path $tmpDir) {
            Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
