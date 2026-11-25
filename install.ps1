<#
.SYNOPSIS
    Kiseki Terminal — One-Click Anime Theme Pack Installer for Windows Terminal & PowerShell
.DESCRIPTION
    Supports both local execution (.\install.ps1) and remote one-liner:
      irm https://raw.githubusercontent.com/nerv-base/kiseki-terminal/main/install.ps1 | iex
.EXAMPLE
    .\install.ps1
    .\install.ps1 -Theme nerv-eva -SkipTools -SkipFonts
#>
[CmdletBinding()]
param(
    [ValidateSet('starlight-8bit', 'nerv-eva', 'shinkai-rain', 'kyoto-sakura')]
    [string]$Theme = 'starlight-8bit',
    [switch]$SkipTools,
    [switch]$SkipFonts
)

$ErrorActionPreference = 'Stop'
$InstallRoot = "$HOME\.config\kiseki-terminal"
$TempClone = $null

Write-Host "`n★ Installing Kiseki Anime Terminal Pack..." -ForegroundColor Cyan

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
    if (Test-Path $PROFILE) { Copy-Item $PROFILE "$backupDir\profile.ps1" -Force }
    Write-Host "  [1/5] Backup saved to $backupDir" -ForegroundColor DarkGray

    # 2. Copy themes, scripts, shaders, and default bat config
    New-Item -ItemType Directory -Path $InstallRoot -Force | Out-Null
    Copy-Item "$RepoRoot\themes"  "$InstallRoot\" -Recurse -Force
    Copy-Item "$RepoRoot\scripts" "$InstallRoot\" -Recurse -Force
    Copy-Item "$RepoRoot\shaders" "$InstallRoot\" -Recurse -Force

    $batCfgDir = "$HOME\.config\bat"
    if (-not (Test-Path "$batCfgDir\config")) {
        New-Item -ItemType Directory -Path $batCfgDir -Force | Out-Null
        @'
--theme="Catppuccin Mocha"
--style="numbers,changes,header,grid"
--italic-text=always
--paging=auto
'@ | Set-Content "$batCfgDir\config" -Encoding utf8NoBOM
    }
    Write-Host "  [2/5] Installed 4 anime themes to $InstallRoot" -ForegroundColor Green

    # 3. Optional CLI tools via winget
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
        foreach ($p in $pkgs) {
            winget install --id $p --exact --silent --accept-package-agreements --accept-source-agreements --disable-interactivity | Out-Null
        }
        foreach ($mod in @('Terminal-Icons', 'PSFzf')) {
            if (-not (Get-Module -ListAvailable $mod)) {
                Install-Module -Name $mod -Scope CurrentUser -Force -AllowClobber -ErrorAction SilentlyContinue
            }
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
                $rel = Invoke-RestMethod "https://api.github.com/repos/$($s.Repo)/releases/latest" -Headers $headers -TimeoutSec 20
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

    # 5. Wire PowerShell profile (both current host & pwsh7/WinPS5 if present) + activate theme
    $profilesToHook = @($PROFILE)
    $pwsh7Prof = "$HOME\Documents\PowerShell\Microsoft.PowerShell_profile.ps1"
    if ($profilesToHook -notcontains $pwsh7Prof -and (Get-Command pwsh -ErrorAction SilentlyContinue)) {
        $profilesToHook += $pwsh7Prof
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
        $existing = if (Test-Path $pf) { Get-Content $pf -Raw } else { '' }
        if ($existing -notmatch [regex]::Escape($hookMarker)) {
            Add-Content -Path $pf -Value "`n$hookBlock" -Encoding utf8
        }
    }

    . "$InstallRoot\scripts\switch-theme.ps1"
    Switch-KisekiTheme -Name $Theme -Quiet
    Write-Host "  [5/5] Activated theme: $Theme" -ForegroundColor Green
    Write-Host "`n✦ Done! Reopen Windows Terminal and try: themes | theme nerv-eva | theme shinkai-rain | theme kyoto-sakura | crt`n" -ForegroundColor Magenta
}
finally {
    if ($TempClone -and (Test-Path $TempClone)) {
        Remove-Item $TempClone -Recurse -Force -ErrorAction SilentlyContinue
    }
}
