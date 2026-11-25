# ============================================================
# ★ Kiseki Terminal — Live Anime Theme Engine for Windows Terminal & PowerShell
# ============================================================

$script:KisekiRoot        = "$HOME\.config\kiseki-terminal"
$script:KisekiCurrentFile = Join-Path $script:KisekiRoot 'current-theme.txt'
$script:KisekiLiveFile    = Join-Path $script:KisekiRoot 'live-mode.txt'

function Get-WindowsTerminalSettingsPath {
    $candidates = @(
        "$env:LOCALAPPDATA\Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json",
        "$env:LOCALAPPDATA\Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json",
        "$env:LOCALAPPDATA\Microsoft\Windows Terminal\settings.json"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { return $c }
    }
    return $candidates[0]
}

function Get-KisekiActiveThemeId {
    if (Test-Path $script:KisekiCurrentFile) {
        $id = (Get-Content $script:KisekiCurrentFile -Raw).Trim()
        if ($id -and (Test-Path (Join-Path $script:KisekiRoot "themes\$id\theme.json"))) {
            return $id
        }
    }
    return 'starlight-8bit'
}

function Get-KisekiLiveMode {
    if (Test-Path $script:KisekiLiveFile) {
        return ((Get-Content $script:KisekiLiveFile -Raw).Trim() -ne 'off')
    }
    return $true
}

function Get-KisekiThemeData {
    param([string]$ThemeId)
    $path = Join-Path $script:KisekiRoot "themes\$ThemeId\theme.json"
    if (-not (Test-Path $path)) {
        throw "Theme '$ThemeId' not found at $path"
    }
    return (Get-Content $path -Raw -Encoding utf8 | ConvertFrom-Json)
}

function Apply-KisekiShellTheme {
    <# .SYNOPSIS Apply theme colors, Oh My Posh prompt, FZF, EZA, and Bat to the current PowerShell session #>
    param([string]$ThemeId = (Get-KisekiActiveThemeId))

    $data = Get-KisekiThemeData -ThemeId $ThemeId
    $themeDir = Join-Path $script:KisekiRoot "themes\$ThemeId"
    $script:KisekiActiveTheme = $ThemeId
    $script:KisekiFastfetchConfig = Join-Path $themeDir 'fastfetch.jsonc'

    # 1. Oh My Posh (rotate POSH_SESSION_ID so OMP v29 never serves stale cached theme!)
    $ompPath = Join-Path $themeDir 'prompt.omp.json'
    if ((Get-Command oh-my-posh -ErrorAction SilentlyContinue) -and (Test-Path $ompPath)) {
        $env:POSH_SESSION_ID = [guid]::NewGuid().ToString()
        $shellName = if ($PSVersionTable.PSEdition -eq 'Core') { 'pwsh' } else { 'powershell' }
        oh-my-posh init $shellName --config $ompPath | Invoke-Expression
    }

    # 2. PSReadLine syntax colors
    if (Get-Module PSReadLine) {
        $colors = @{}
        foreach ($prop in $data.psReadLine.PSObject.Properties) {
            if ($PSVersionTable.PSEdition -eq 'Core' -or $prop.Name -notmatch 'Prediction') {
                $colors[$prop.Name] = $prop.Value
            }
        }
        Set-PSReadLineOption -Colors $colors -ErrorAction SilentlyContinue
    }

    # 3. FZF palette & borders
    $env:FZF_DEFAULT_OPTS = @(
        '--height=45%'
        '--layout=reverse'
        "--border=$($data.fzf.border)"
        "--prompt=`"$($data.fzf.prompt)`""
        "--pointer=`"$($data.fzf.pointer)`""
        "--marker=`"$($data.fzf.marker)`""
        "--color=$($data.fzf.colors)"
    ) -join ' '

    # 4. EZA colors & Bat theme
    $env:EZA_COLORS = $data.ezaColors
    $env:BAT_THEME  = $data.batTheme
}

function Sync-KisekiWindowsTerminal {
    <# .SYNOPSIS Register all Kiseki themes into Windows Terminal settings.json and activate one (Live 60fps HLSL or Static) #>
    param(
        [Parameter(Mandatory)][string]$ThemeId,
        [double]$BgOpacity = -1,
        [Nullable[bool]]$LiveMode = $null
    )
    $wtPath = Get-WindowsTerminalSettingsPath
    if (-not (Test-Path $wtPath)) {
        Write-Warning "Windows Terminal settings.json not found at $wtPath"
        return
    }

    $wt = Get-Content $wtPath -Raw -Encoding utf8 | ConvertFrom-Json
    if (-not $wt.schemes)  { $wt | Add-Member -NotePropertyName schemes  -NotePropertyValue @() -Force }
    if (-not $wt.themes)   { $wt | Add-Member -NotePropertyName themes   -NotePropertyValue @() -Force }
    if (-not $wt.profiles) { $wt | Add-Member -NotePropertyName profiles -NotePropertyValue ([PSCustomObject]@{ defaults = [PSCustomObject]@{} }) -Force }
    if (-not $wt.profiles.defaults) { $wt.profiles | Add-Member -NotePropertyName defaults -NotePropertyValue ([PSCustomObject]@{}) -Force }

    # Upsert all themes in themes/ so user can also browse them inside WT Settings UI
    $allThemes = Get-ChildItem (Join-Path $script:KisekiRoot 'themes') -Directory | Sort-Object Name
    foreach ($dir in $allThemes) {
        $metaPath = Join-Path $dir.FullName 'theme.json'
        if (-not (Test-Path $metaPath)) { continue }
        $t = Get-Content $metaPath -Raw -Encoding utf8 | ConvertFrom-Json

        $schemes = @($wt.schemes | Where-Object { $_.name -ne $t.scheme.name })
        $wt.schemes = @($t.scheme) + $schemes

        $uiThemes = @($wt.themes | Where-Object { $_.name -ne $t.uiTheme.name })
        $wt.themes = @($t.uiTheme) + $uiThemes
    }

    # Activate chosen theme
    $active    = Get-KisekiThemeData -ThemeId $ThemeId
    $wallpaper = Join-Path $script:KisekiRoot "themes\$ThemeId\wallpaper.png"
    $liveHlsl  = Join-Path $script:KisekiRoot "themes\$ThemeId\live.hlsl"
    $finalBgOp = if ($BgOpacity -ge 0) { $BgOpacity } else { $active.terminal.backgroundImageOpacity }
    $useLive   = if ($null -ne $LiveMode) { [bool]$LiveMode } else { Get-KisekiLiveMode }

    $wt.theme = $active.uiTheme.name
    $def = $wt.profiles.defaults
    $def | Add-Member -NotePropertyName colorScheme      -NotePropertyValue $active.scheme.name -Force
    $def | Add-Member -NotePropertyName cursorShape      -NotePropertyValue $active.terminal.cursorShape -Force
    $def | Add-Member -NotePropertyName opacity          -NotePropertyValue $active.terminal.opacity -Force
    $def | Add-Member -NotePropertyName useAcrylic       -NotePropertyValue $active.terminal.useAcrylic -Force
    $def | Add-Member -NotePropertyName padding          -NotePropertyValue $active.terminal.padding -Force
    $def | Add-Member -NotePropertyName intenseTextStyle -NotePropertyValue 'all' -Force
    $def | Add-Member -NotePropertyName scrollbarState   -NotePropertyValue 'hidden' -Force
    $def | Add-Member -NotePropertyName bellStyle        -NotePropertyValue 'none' -Force
    $def | Add-Member -NotePropertyName font -NotePropertyValue ([PSCustomObject]@{
        face       = $active.font.face
        size       = $active.font.size
        cellHeight = $active.font.cellHeight
        weight     = $active.font.weight
    }) -Force

    if ($useLive -and (Test-Path $liveHlsl)) {
        # Live 60fps HLSL mode: wallpaper bound to register(t1), animated directly on GPU!
        foreach ($prop in @('backgroundImage', 'backgroundImageOpacity', 'backgroundImageAlignment', 'backgroundImageStretchMode')) {
            if ($def.PSObject.Properties[$prop]) { $def.PSObject.Properties.Remove($prop) }
        }
        $def | Add-Member -NotePropertyName 'experimental.pixelShaderPath'      -NotePropertyValue $liveHlsl -Force
        $def | Add-Member -NotePropertyName 'experimental.pixelShaderImagePath' -NotePropertyValue $wallpaper -Force
        $def | Add-Member -NotePropertyName unfocusedAppearance -NotePropertyValue ([PSCustomObject]@{
            opacity = $active.terminal.unfocusedOpacity
        }) -Force
    } else {
        # Static zero-redraw mode: native backgroundImage
        foreach ($prop in @('experimental.pixelShaderPath', 'experimental.pixelShaderImagePath')) {
            if ($def.PSObject.Properties[$prop]) { $def.PSObject.Properties.Remove($prop) }
        }
        $def | Add-Member -NotePropertyName backgroundImage            -NotePropertyValue $wallpaper -Force
        $def | Add-Member -NotePropertyName backgroundImageOpacity     -NotePropertyValue $finalBgOp -Force
        $def | Add-Member -NotePropertyName backgroundImageAlignment   -NotePropertyValue $active.terminal.backgroundImageAlignment -Force
        $def | Add-Member -NotePropertyName backgroundImageStretchMode -NotePropertyValue 'uniformToFill' -Force
        $def | Add-Member -NotePropertyName unfocusedAppearance -NotePropertyValue ([PSCustomObject]@{
            opacity                = $active.terminal.unfocusedOpacity
            backgroundImageOpacity = $active.terminal.unfocusedBgOpacity
        }) -Force
    }

    $wt | ConvertTo-Json -Depth 12 | Set-Content $wtPath -Encoding utf8NoBOM
}

function Switch-KisekiTheme {
    <# .SYNOPSIS Switch active anime theme live (WT + HLSL Live Shader + Oh My Posh + Fastfetch + PSReadLine + FZF + EZA + Bat) #>
    param(
        [Parameter(Position = 0)]
        [ArgumentCompleter({
            param($cmd, $param, $word)
            Get-ChildItem "$HOME\.config\kiseki-terminal\themes" -Directory -ErrorAction SilentlyContinue |
                Select-Object -ExpandProperty Name |
                Where-Object { $_ -like "$word*" }
        })]
        [string]$Name,
        [double]$BgOpacity = -1,
        [switch]$Static,
        [switch]$Quiet
    )

    if (-not $Name) {
        Get-KisekiThemes
        return
    }

    $targetDir = Join-Path $script:KisekiRoot "themes\$Name"
    if (-not (Test-Path (Join-Path $targetDir 'theme.json'))) {
        Write-Host "✖ Unknown theme '$Name'. Available themes:" -ForegroundColor Red
        Get-KisekiThemes
        return
    }

    Set-Content -Path $script:KisekiCurrentFile -Value $Name -Encoding utf8NoBOM -NoNewline
    if ($Static) {
        Set-Content -Path $script:KisekiLiveFile -Value 'off' -Encoding utf8NoBOM -NoNewline
    }
    Sync-KisekiWindowsTerminal -ThemeId $Name -BgOpacity $BgOpacity
    Apply-KisekiShellTheme -ThemeId $Name

    if (-not $Quiet) {
        $meta = Get-KisekiThemeData -ThemeId $Name
        $modeTag = if (Get-KisekiLiveMode) { 'LIVE 60FPS' } else { 'STATIC' }
        Write-Host "✦ Switched to [$($meta.name)] ($modeTag) — $($meta.subtitle)" -ForegroundColor Cyan
        if (Get-Command fastfetch -ErrorAction SilentlyContinue) {
            fastfetch --config $script:KisekiFastfetchConfig
        }
    }
}

function Toggle-KisekiLive {
    <# .SYNOPSIS Toggle 60fps GPU-animated live background shaders ON/OFF (hot-reloads Windows Terminal) #>
    param(
        [ValidateSet('toggle', 'on', 'off')][string]$State = 'toggle'
    )
    $current = Get-KisekiLiveMode
    $next = switch ($State) {
        'on'     { $true }
        'off'    { $false }
        default  { -not $current }
    }
    Set-Content -Path $script:KisekiLiveFile -Value ($(if ($next) { 'on' } else { 'off' })) -Encoding utf8NoBOM -NoNewline
    $active = Get-KisekiActiveThemeId
    Sync-KisekiWindowsTerminal -ThemeId $active -LiveMode $next
    if ($next) {
        Write-Host "✦ Live Dynamic Background: ON (60fps HLSL GPU shader for '$active')" -ForegroundColor Magenta
    } else {
        Write-Host "✦ Live Dynamic Background: OFF (static zero-GPU mode for '$active')" -ForegroundColor Cyan
    }
}

function Get-KisekiThemes {
    <# .SYNOPSIS List all installed Kiseki anime terminal themes #>
    $active = Get-KisekiActiveThemeId
    $modeTag = if (Get-KisekiLiveMode) { 'LIVE 60FPS' } else { 'STATIC' }
    Write-Host "`n★ Kiseki Anime Terminal Themes [$modeTag]  (switch: theme <id> | toggle motion: live)`n" -ForegroundColor Cyan
    Get-ChildItem (Join-Path $script:KisekiRoot 'themes') -Directory | Sort-Object Name | ForEach-Object {
        $meta = Get-Content (Join-Path $_.FullName 'theme.json') -Raw -Encoding utf8 | ConvertFrom-Json
        $mark = if ($meta.id -eq $active) { '▶' } else { ' ' }
        $color = if ($meta.id -eq $active) { 'Magenta' } else { 'Gray' }
        Write-Host ("  {0} {1,-16} {2,-26} {3}" -f $mark, $meta.id, "[$($meta.style)]", $meta.subtitle) -ForegroundColor $color
    }
    Write-Host ""
}

function Toggle-RetroCRT {
    <# .SYNOPSIS Toggle static subtle CRT phosphor bloom & scanlines in Windows Terminal #>
    $wtPath = Get-WindowsTerminalSettingsPath
    $json = Get-Content $wtPath -Raw -Encoding utf8 | ConvertFrom-Json
    $shader = Join-Path $script:KisekiRoot 'shaders\retro-crt-subtle.hlsl'
    $prop = 'experimental.pixelShaderPath'
    $imgProp = 'experimental.pixelShaderImagePath'
    $active = Get-KisekiActiveThemeId
    if ($json.profiles.defaults.PSObject.Properties[$prop] -and $json.profiles.defaults.$prop -eq $shader) {
        Sync-KisekiWindowsTerminal -ThemeId $active
        Write-Host "✦ Retro CRT Shader: OFF (restored theme mode)" -ForegroundColor Cyan
    } else {
        # Ensure static wallpaper is mounted underneath CRT shader cleanly
        Sync-KisekiWindowsTerminal -ThemeId $active -LiveMode $false
        $json = Get-Content $wtPath -Raw -Encoding utf8 | ConvertFrom-Json
        if ($json.profiles.defaults.PSObject.Properties[$imgProp]) { $json.profiles.defaults.PSObject.Properties.Remove($imgProp) }
        $json.profiles.defaults | Add-Member -NotePropertyName $prop -NotePropertyValue $shader -Force
        $json | ConvertTo-Json -Depth 12 | Set-Content $wtPath -Encoding utf8NoBOM
        Write-Host "✦ Retro CRT Shader: ON (subtle phosphor bloom + scanlines)" -ForegroundColor Magenta
    }
}

function Set-TerminalFont {
    <# .SYNOPSIS Switch terminal font live: 8bit | code | maple | modern #>
    param(
        [Parameter(Mandatory)][ValidateSet('8bit', 'code', 'maple', 'modern')][string]$Style,
        [double]$Size = 11.0
    )
    $map = @{
        '8bit'   = 'DepartureMono Nerd Font, Fusion Pixel 12px Mono zh-Hans, Fusion Pixel 12px Mono ja, JetBrainsMono NF'
        'code'   = 'Pixel Code, Fusion Pixel 12px Mono zh-Hans, Fusion Pixel 12px Mono ja, JetBrainsMono NF'
        'maple'  = 'Maple Mono NF CN, JetBrainsMono NF'
        'modern' = 'JetBrainsMono NF, Maple Mono NF CN'
    }
    $wtPath = Get-WindowsTerminalSettingsPath
    $json = Get-Content $wtPath -Raw -Encoding utf8 | ConvertFrom-Json
    $json.profiles.defaults.font.face = $map[$Style]
    $json.profiles.defaults.font.size = $Size
    $json | ConvertTo-Json -Depth 12 | Set-Content $wtPath -Encoding utf8NoBOM
    Write-Host "✦ Terminal Font -> $Style ($Size pt)" -ForegroundColor Cyan
}

function Set-TerminalBg {
    <# .SYNOPSIS Adjust static background opacity or turn off wallpaper #>
    param(
        [ValidateRange(0.0, 1.0)][double]$Opacity = 0.40,
        [switch]$Off
    )
    $active = Get-KisekiActiveThemeId
    if ($Off) {
        Set-Content -Path $script:KisekiLiveFile -Value 'off' -Encoding utf8NoBOM -NoNewline
        Sync-KisekiWindowsTerminal -ThemeId $active -LiveMode $false
        $wtPath = Get-WindowsTerminalSettingsPath
        $json = Get-Content $wtPath -Raw -Encoding utf8 | ConvertFrom-Json
        $json.profiles.defaults.PSObject.Properties.Remove('backgroundImage')
        $json | ConvertTo-Json -Depth 12 | Set-Content $wtPath -Encoding utf8NoBOM
        Write-Host "✦ Terminal Background: OFF" -ForegroundColor Magenta
    } else {
        Set-Content -Path $script:KisekiLiveFile -Value 'off' -Encoding utf8NoBOM -NoNewline
        Sync-KisekiWindowsTerminal -ThemeId $active -BgOpacity $Opacity -LiveMode $false
        Write-Host "✦ Terminal Static Background Opacity -> $Opacity (run 'live' to re-enable 60fps motion)" -ForegroundColor Magenta
    }
}

Set-Alias theme    Switch-KisekiTheme -Scope Global
Set-Alias themes   Get-KisekiThemes   -Scope Global
Set-Alias live     Toggle-KisekiLive  -Scope Global
Set-Alias motion   Toggle-KisekiLive  -Scope Global
Set-Alias crt      Toggle-RetroCRT    -Scope Global
Set-Alias termfont Set-TerminalFont   -Scope Global
Set-Alias termbg   Set-TerminalBg     -Scope Global
