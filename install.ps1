<#
.SYNOPSIS
    Kiseki Terminal - One-Click Anime Theme Pack Installer for Windows Terminal and PowerShell
.DESCRIPTION
    Supports both local execution (.\install.ps1) and remote one-liner (Windows PowerShell 5.1 and PowerShell 7+):
      irm https://raw.githubusercontent.com/nerv-base/kiseki-terminal/main/install.ps1 | iex
.EXAMPLE
    .\install.ps1
    .\install.ps1 -Theme nerv-eva -SkipTools -SkipFonts
#>
[CmdletBinding()]
param(
    [ValidateSet('starlight-8bit', 'nerv-eva', 'shinkai-rain', 'kyoto-sakura')]
    [string]$Theme = 'starlight-8bit',
    [switch]$Static,
    [switch]$SkipTools,
    [switch]$SkipFonts
)

$ErrorActionPreference = 'Stop'
$prevProgress = $ProgressPreference
$ProgressPreference = 'SilentlyContinue'

# Ensure TLS 1.2 for Windows PowerShell 5.1 when talking to GitHub
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {}

function Write-KisekiFileUtf8NoBom {
    param([string]$Path, [string]$Content)
    $parent = Split-Path $Path -Parent
    if ($parent -and -not (Test-Path $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function Copy-KisekiTree {
    param([string]$SourceDir, [string]$DestDir)
    if (-not (Test-Path $SourceDir)) { return }
    if (-not (Test-Path $DestDir)) {
        New-Item -ItemType Directory -Path $DestDir -Force | Out-Null
    }
    Copy-Item -Path (Join-Path $SourceDir '*') -Destination $DestDir -Recurse -Force
}

$InstallRoot = "$HOME\.config\kiseki-terminal"
$TempClone   = $null
$star        = [char]0x2605
$spark       = [char]0x2726

Write-Host "`n$star Installing Kiseki Anime Terminal Pack..." -ForegroundColor Cyan

# 0. Support remote one-liner (`irm ... | iex`) when $PSScriptRoot is empty
$RepoRoot = $PSScriptRoot
if (-not $RepoRoot -or -not (Test-Path (Join-Path $RepoRoot 'themes'))) {
    $TempClone = Join-Path $env:TEMP ("kiseki-dl-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $TempClone -Force | Out-Null
    $zipPath = Join-Path $TempClone 'repo.zip'
    Write-Host "  [0/5] Downloading Kiseki Terminal from GitHub..." -ForegroundColor Cyan
    Invoke-WebRequest -Uri 'https://github.com/nerv-base/kiseki-terminal/archive/refs/heads/main.zip' -OutFile $zipPath -UseBasicParsing
    Expand-Archive -Path $zipPath -DestinationPath $TempClone -Force
    $RepoRoot = (Get-ChildItem $TempClone -Directory | Select-Object -First 1).FullName
}

try {
    # 1. Backup existing Windows Terminal & PowerShell configs
    $backupDir = "$HOME\.config\kiseki-backup-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
    New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
    $wtCandidates = @(
        "$env:LOCALAPPDATA\Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json",
        "$env:LOCALAPPDATA\Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json",
        "$env:LOCALAPPDATA\Microsoft\Windows Terminal\settings.json"
    )
    foreach ($c in $wtCandidates) {
        if (Test-Path $c) { Copy-Item $c "$backupDir\wt-settings.json" -Force; break }
    }
    if ($PROFILE -and (Test-Path $PROFILE)) { Copy-Item $PROFILE "$backupDir\profile.ps1" -Force }
    Write-Host "  [1/5] Backup saved to $backupDir" -ForegroundColor DarkGray

    # 2. Copy themes, scripts, shaders, version manifest, and default bat config
    New-Item -ItemType Directory -Path $InstallRoot -Force | Out-Null
    Copy-KisekiTree -SourceDir (Join-Path $RepoRoot 'themes')  -DestDir (Join-Path $InstallRoot 'themes')
    Copy-KisekiTree -SourceDir (Join-Path $RepoRoot 'scripts') -DestDir (Join-Path $InstallRoot 'scripts')
    Copy-KisekiTree -SourceDir (Join-Path $RepoRoot 'shaders') -DestDir (Join-Path $InstallRoot 'shaders')
    if (Test-Path (Join-Path $RepoRoot 'version.json')) {
        Copy-Item (Join-Path $RepoRoot 'version.json') (Join-Path $InstallRoot 'version.json') -Force
    }

    $batCfgDir = "$HOME\.config\bat"
    if (-not (Test-Path "$batCfgDir\config")) {
        $batCfg = @'
--theme="Catppuccin Mocha"
--style="numbers,changes,header,grid"
--italic-text=always
--paging=auto
'@
        Write-KisekiFileUtf8NoBom -Path "$batCfgDir\config" -Content $batCfg
    }
    $verLabel = if (Test-Path "$InstallRoot\version.json") { "v$((Get-Content "$InstallRoot\version.json" -Raw -Encoding utf8 | ConvertFrom-Json).version)" } else { '' }
    $themeCount = @(Get-ChildItem "$InstallRoot\themes" -Directory).Count
    Write-Host "  [2/5] Installed $themeCount anime themes ($verLabel) to $InstallRoot" -ForegroundColor Green

    # 3. Optional CLI tools via winget (non-fatal if a package is already installed or unavailable)
    if (-not $SkipTools -and (Get-Command winget -ErrorAction SilentlyContinue)) {
        Write-Host "  [3/5] Installing recommended CLI tools via winget..." -ForegroundColor Cyan
        $pkgs = @(
            'JanDeDobbeleer.OhMyPosh',
            'eza-community.eza',
            'sharkdp.bat',
            'junegunn.fzf',
            'ajeetdsouza.zoxide',
            'Fastfetch-cli.Fastfetch'
        )
        $prevNativeErr = $null
        if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
            $prevNativeErr = $PSNativeCommandUseErrorActionPreference
            $PSNativeCommandUseErrorActionPreference = $false
        }
        $oldEap = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            foreach ($p in $pkgs) {
                try {
                    & winget install --id $p --exact --silent --accept-package-agreements --accept-source-agreements --disable-interactivity 2>&1 | Out-Null
                } catch {
                    Write-Warning "winget skipped ${p}: $($_.Exception.Message)"
                }
            }
            # Bootstrap NuGet silently on fresh Windows PowerShell 5.1 before Install-Module
            try {
                if (-not (Get-PackageProvider -Name NuGet -ListAvailable -ErrorAction SilentlyContinue)) {
                    Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Scope CurrentUser -Force -Confirm:$false -ErrorAction SilentlyContinue | Out-Null
                }
            } catch {}
            foreach ($mod in @('Terminal-Icons', 'PSFzf')) {
                if (-not (Get-Module -ListAvailable $mod)) {
                    try {
                        Install-Module -Name $mod -Scope CurrentUser -Force -AllowClobber -Confirm:$false -ErrorAction SilentlyContinue | Out-Null
                    } catch {}
                }
            }
        } finally {
            $ErrorActionPreference = $oldEap
            if ($null -ne $prevNativeErr) {
                $PSNativeCommandUseErrorActionPreference = $prevNativeErr
            }
        }
        # Refresh current session PATH so newly installed oh-my-posh / fastfetch work immediately
        $regPath = @(
            [System.Environment]::GetEnvironmentVariable('Path', 'User')
            [System.Environment]::GetEnvironmentVariable('Path', 'Machine')
        ) -join ';'
        $curSet = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        foreach ($p in ($env:Path -split ';')) { if ($p) { [void]$curSet.Add($p.TrimEnd('\')) } }
        foreach ($p in ($regPath -split ';')) {
            if ($p -and $curSet.Add($p.TrimEnd('\'))) { $env:Path = "$env:Path;$p" }
        }
    } else {
        Write-Host "  [3/5] Skipped CLI tools install" -ForegroundColor DarkGray
    }

    # 4. Optional fonts (DepartureMono NF, Fusion Pixel 12px CN/JP, Maple Mono NF CN, JetBrainsMono NF)
    if (-not $SkipFonts) {
        Write-Host "  [4/5] Installing curated anime & pixel fonts..." -ForegroundColor Cyan
        $fontDir = "$env:LOCALAPPDATA\Microsoft\Windows\Fonts"
        $regPath = 'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Fonts'
        New-Item -ItemType Directory -Path $fontDir -Force | Out-Null
        if (-not (Test-Path $regPath)) { New-Item -Path $regPath -Force | Out-Null }
        $tmp = "$env:TEMP\kiseki-fonts"
        New-Item -ItemType Directory -Path $tmp -Force | Out-Null
        $headers = @{ 'User-Agent' = 'KisekiTerminalInstaller' }

        if (-not ('KisekiFontWin32' -as [type])) {
            Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class KisekiFontWin32 {
    [DllImport("gdi32.dll", CharSet = CharSet.Unicode)]
    public static extern int AddFontResourceW(string lpszFilename);
    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam, uint fuFlags, uint uTimeout, out IntPtr lpdwResult);
}
"@
        }

        $sources = @(
            @{ Repo = 'ryanoasis/nerd-fonts';      Pattern = '^DepartureMono\.zip$'; Filter = '\.(otf|ttf)$' },
            @{ Repo = 'ryanoasis/nerd-fonts';      Pattern = '^JetBrainsMono\.zip$'; Filter = 'JetBrainsMonoNerdFont-(Regular|Bold|Italic)\.ttf$' },
            @{ Repo = 'TakWolf/fusion-pixel-font'; Pattern = '12px-monospaced-ttf.*\.zip$'; Filter = '(zh_hans|ja)\.ttf$' },
            @{ Repo = 'subframe7536/maple-font';   Pattern = '^MapleMono-NF-CN\.zip$'; Filter = '-(Regular|Medium|Bold|Italic)\.ttf$' }
        )
        foreach ($s in $sources) {
            try {
                $rel = Invoke-RestMethod "https://api.github.com/repos/$($s.Repo)/releases/latest" -Headers $headers -TimeoutSec 20 -UseBasicParsing
                $asset = $rel.assets | Where-Object { $_.name -match $s.Pattern } | Select-Object -First 1
                if ($asset) {
                    $zip = Join-Path $tmp $asset.name
                    $ext = Join-Path $tmp ($asset.name -replace '\.zip$', '')
                    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
                    Expand-Archive $zip $ext -Force
                    Get-ChildItem $ext -Recurse -File | Where-Object { $_.FullName -notmatch '__MACOSX' -and $_.Name -notlike '._*' -and $_.Name -match $s.Filter } | ForEach-Object {
                        $dest = Join-Path $fontDir $_.Name
                        Copy-Item $_.FullName $dest -Force
                        $suffix = if ($_.Extension -eq '.otf') { '(OpenType)' } else { '(TrueType)' }
                        New-ItemProperty -Path $regPath -Name "$($_.BaseName) $suffix" -Value $dest -PropertyType String -Force | Out-Null
                        [KisekiFontWin32]::AddFontResourceW($dest) | Out-Null
                    }
                }
            } catch {
                Write-Warning "Could not download font from $($s.Repo): $_"
            }
        }
        $res = [IntPtr]::Zero
        [KisekiFontWin32]::SendMessageTimeout([IntPtr]0xffff, 0x001D, [IntPtr]::Zero, [IntPtr]::Zero, 2, 1000, [ref]$res) | Out-Null
        Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "  [4/5] Skipped font install" -ForegroundColor DarkGray
    }

    # 5. Ensure CurrentUser can run profile scripts, wire BOTH WinPS 5.1 & pwsh 7 profiles, and activate theme
    try {
        $effPol = Get-ExecutionPolicy
        $curPol = Get-ExecutionPolicy -Scope CurrentUser
        if (($effPol -eq 'Restricted' -or $effPol -eq 'AllSigned') -and ($curPol -eq 'Undefined' -or $curPol -eq 'Restricted')) {
            Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy RemoteSigned -Force -ErrorAction SilentlyContinue
        }
    } catch {}

    $docsDir = [Environment]::GetFolderPath('MyDocuments')
    if (-not $docsDir) { $docsDir = "$HOME\Documents" }
    $profileCandidates = @()
    if ($PROFILE) { $profileCandidates += [string]$PROFILE }
    $profileCandidates += (Join-Path $docsDir 'WindowsPowerShell\Microsoft.PowerShell_profile.ps1')
    $pwsh7Prof = Join-Path $docsDir 'PowerShell\Microsoft.PowerShell_profile.ps1'
    if ((Get-Command pwsh -ErrorAction SilentlyContinue) -or (Test-Path (Split-Path $pwsh7Prof -Parent))) {
        $profileCandidates += $pwsh7Prof
    }

    $seenProfiles = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $profilesToHook = @()
    foreach ($pf in $profileCandidates) {
        if ($pf -and $seenProfiles.Add($pf)) { $profilesToHook += $pf }
    }

    $hookMarker = '# >>> kiseki-terminal >>>'
    $hookBlock = @'
# >>> kiseki-terminal >>>
if (Test-Path "$HOME\.config\kiseki-terminal\scripts\profile-loader.ps1") {
    . "$HOME\.config\kiseki-terminal\scripts\profile-loader.ps1"
}
# <<< kiseki-terminal <<<
'@
    foreach ($pf in $profilesToHook) {
        $pDir = Split-Path $pf -Parent
        if (-not (Test-Path $pDir)) { New-Item -ItemType Directory -Path $pDir -Force | Out-Null }
        $existing = if (Test-Path $pf) { Get-Content $pf -Raw -Encoding utf8 } else { '' }
        if ($null -eq $existing) { $existing = '' }
        if ($existing -notmatch [regex]::Escape($hookMarker)) {
            $sep = if ($existing.Length -gt 0 -and -not $existing.EndsWith("`n")) { "`r`n`r`n" } else { "`r`n" }
            [System.IO.File]::WriteAllText($pf, ($existing + $sep + $hookBlock + "`r`n"), [System.Text.UTF8Encoding]::new($true))
        }
    }

    . "$InstallRoot\scripts\switch-theme.ps1"
    Write-KisekiFileUtf8NoBom -Path "$InstallRoot\live-mode.txt" -Content ($(if ($Static) { 'off' } else { 'on' }))
    Switch-KisekiTheme -Name $Theme -Quiet
    $modeLabel = if ($Static) { 'Static' } else { 'Live 60fps HLSL' }
    Write-Host "  [5/5] Activated theme: $Theme ($modeLabel)" -ForegroundColor Green
    Write-Host "`n$spark Done! Reopen Windows Terminal and try: themes | theme nerv-eva | theme shinkai-rain | theme kyoto-sakura | live | theme update`n" -ForegroundColor Magenta
}
finally {
    $ProgressPreference = $prevProgress
    if ($TempClone -and (Test-Path $TempClone)) {
        Remove-Item $TempClone -Recurse -Force -ErrorAction SilentlyContinue
    }
}
