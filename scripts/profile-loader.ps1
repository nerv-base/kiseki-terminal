# ============================================================
# ★ Kiseki Terminal — Shared PowerShell Profile Loader (pwsh 7 & WinPS 5.1)
# ============================================================

# UTF-8 encoding for Nerd Font icons, 8-bit glyphs, and CJK
if ($PSVersionTable.PSEdition -eq 'Core') {
    [Console]::InputEncoding  = [System.Text.UTF8Encoding]::new($false)
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
    $OutputEncoding           = [System.Text.UTF8Encoding]::new($false)
} else {
    $env:PSModulePath = "$HOME\Documents\PowerShell\Modules;" + $env:PSModulePath
}

# Sync newly installed WinGet / User PATH entries into running shells if needed
if (-not (Get-Command eza -ErrorAction SilentlyContinue)) {
    $regPath = @(
        [System.Environment]::GetEnvironmentVariable('Path', 'User')
        [System.Environment]::GetEnvironmentVariable('Path', 'Machine')
    ) -join ';'
    $curSet = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($p in ($env:Path -split ';')) { if ($p) { [void]$curSet.Add($p.TrimEnd('\')) } }
    foreach ($p in ($regPath -split ';')) {
        if ($p -and $curSet.Add($p.TrimEnd('\'))) { $env:Path = "$env:Path;$p" }
    }
}

# --- Core Modules ---
Import-Module Terminal-Icons -ErrorAction SilentlyContinue
Import-Module PSReadLine -ErrorAction SilentlyContinue

# --- PSReadLine Behavior ---
if ($Host.UI.SupportsVirtualTerminal -and -not [Console]::IsOutputRedirected) {
    Set-PSReadLineOption -PredictionSource History -ErrorAction SilentlyContinue
    if ($PSVersionTable.PSEdition -eq 'Core') {
        Set-PSReadLineOption -PredictionViewStyle InlineView -ErrorAction SilentlyContinue
    } else {
        Set-PSReadLineOption -PredictionViewStyle ListView -ErrorAction SilentlyContinue
    }
}
Set-PSReadLineOption -EditMode Windows -ErrorAction SilentlyContinue
Set-PSReadLineOption -HistoryNoDuplicates -ErrorAction SilentlyContinue
Set-PSReadLineOption -HistorySearchCursorMovesToEnd -ErrorAction SilentlyContinue
Set-PSReadLineOption -MaximumHistoryCount 8192 -ErrorAction SilentlyContinue
Set-PSReadLineOption -BellStyle None -ErrorAction SilentlyContinue

# --- Keybindings ---
Set-PSReadLineKeyHandler -Key Tab -Function MenuComplete
Set-PSReadLineKeyHandler -Key UpArrow -Function HistorySearchBackward
Set-PSReadLineKeyHandler -Key DownArrow -Function HistorySearchForward
if ($PSVersionTable.PSEdition -eq 'Core') {
    Set-PSReadLineKeyHandler -Key Ctrl+d -Function DeleteCharOrExit
}

# --- Load Kiseki Theme Engine & Apply Active Theme ---
. "$HOME\.config\kiseki-terminal\scripts\switch-theme.ps1"
Apply-KisekiShellTheme

# --- PSFzf (Ctrl+R history / Ctrl+T files) ---
if ((Get-Command fzf -ErrorAction SilentlyContinue) -and (Get-Module -ListAvailable PSFzf)) {
    Import-Module PSFzf -ErrorAction SilentlyContinue
    Set-PsFzfOption -PSReadlineChordProvider 'Ctrl+t' -PSReadlineChordReverseHistory 'Ctrl+r'
}

# --- Zoxide (Smart `z` directory jump) ---
if (Get-Command zoxide -ErrorAction SilentlyContinue) {
    Invoke-Expression (& { (zoxide init powershell | Out-String) })
}

# --- Bat (Syntax-highlighted `cat`) ---
$env:BAT_CONFIG_PATH = "$HOME\.config\bat\config"
if (Get-Command bat -ErrorAction SilentlyContinue) {
    function global:cat { bat @args }
}

# --- Eza (Modern icon + git colorized `ls`, `ll`, `la`, `lt`) ---
if (Get-Command eza -ErrorAction SilentlyContinue) {
    Remove-Item Alias:ls -Force -ErrorAction SilentlyContinue
    function global:ls { eza --icons=auto --group-directories-first @args }
    function global:ll { eza -l --icons=auto --group-directories-first --git --time-style=relative @args }
    function global:la { eza -la --icons=auto --group-directories-first --git --time-style=relative @args }
    function global:lt { eza --tree --level=2 --icons=auto --group-directories-first @args }
} else {
    Set-Alias ll Get-ChildItem -Scope Global
}

Set-Alias grep Select-String -Scope Global

# --- Fastfetch (Active Theme RPG Emblem & Status Card) ---
function global:fetch {
    if (Get-Command fastfetch -ErrorAction SilentlyContinue) {
        $cfg = if ($script:KisekiFastfetchConfig -and (Test-Path $script:KisekiFastfetchConfig)) {
            $script:KisekiFastfetchConfig
        } else {
            "$HOME\.config\kiseki-terminal\themes\$(Get-KisekiActiveThemeId)\fastfetch.jsonc"
        }
        fastfetch --config $cfg @args
    }
}
Set-Alias neofetch fetch -Scope Global

# --- Auto-Greeting in Top-Level Interactive Windows Terminal Tabs ---
if ($env:WT_SESSION -and $Host.UI.SupportsVirtualTerminal -and -not [Console]::IsOutputRedirected -and -not $env:TERM_PROGRAM -and -not $env:NO_FETCH) {
    fetch
}
