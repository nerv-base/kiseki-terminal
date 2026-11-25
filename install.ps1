<#
.SYNOPSIS
    Kiseki Terminal — One-Click Anime Theme Pack Installer for Windows Terminal & PowerShell
.DESCRIPTION
    Installs themes, wallpapers, Oh My Posh prompts, Fastfetch emblems, CRT shader,
    and optionally installs recommended fonts & CLI tools (oh-my-posh, eza, bat, fzf, zoxide, fastfetch).
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
$RepoRoot = $PSScriptRoot
$InstallRoot = "$HOME\.config\kiseki-terminal"

Write-Host "`n★ Installing Kiseki Anime Terminal Pack..." -ForegroundColor Cyan

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

# 2. Copy themes, scripts, and shaders
New-Item -ItemType Directory -Path $InstallRoot -Force | Out-Null
Copy-Item "$RepoRoot\themes"  "$InstallRoot\" -Recurse -Force
Copy-Item "$RepoRoot\scripts" "$InstallRoot\" -Recurse -Force
Copy-Item "$RepoRoot\shaders" "$InstallRoot\" -Recurse -Force
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
            Install-Module -Name $mod -Scope CurrentUser -Force -AllowClobber
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
    $tmp = "$env:TEMP\kiseki-fonts"
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null
    $headers = @{ 'User-Agent' = 'KisekiTerminalInstaller' }

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
                }
            }
        } catch {
            Write-Warning "Could not download font from $($s.Repo): $_"
        }
    }
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
} else {
    Write-Host "  [4/5] Skipped font install" -ForegroundColor DarkGray
}

# 5. Wire PowerShell profile + activate default theme
$profileDir = Split-Path $PROFILE -Parent
if (-not (Test-Path $profileDir)) { New-Item -ItemType Directory -Path $profileDir -Force | Out-Null }
$hookMarker = '# >>> kiseki-terminal >>>'
$hookBlock = @'
# >>> kiseki-terminal >>>
if (Test-Path "$HOME\.config\kiseki-terminal\scripts\profile-loader.ps1") {
    . "$HOME\.config\kiseki-terminal\scripts\profile-loader.ps1"
}
# <<< kiseki-terminal <<<
'@
$existing = if (Test-Path $PROFILE) { Get-Content $PROFILE -Raw } else { '' }
if ($existing -notmatch [regex]::Escape($hookMarker)) {
    Add-Content -Path $PROFILE -Value "`n$hookBlock" -Encoding utf8
}

. "$InstallRoot\scripts\switch-theme.ps1"
Switch-KisekiTheme -Name $Theme -Quiet
Write-Host "  [5/5] Activated theme: $Theme" -ForegroundColor Green
Write-Host "`n✦ Done! Open a new Windows Terminal tab and try: themes | theme nerv-eva | theme shinkai-rain | theme kyoto-sakura | crt`n" -ForegroundColor Magenta
