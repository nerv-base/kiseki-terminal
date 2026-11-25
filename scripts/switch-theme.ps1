# ============================================================
# ★ Kiseki Terminal — Live Anime Theme Engine for Windows Terminal & PowerShell
# ============================================================

$script:KisekiRoot = "$HOME\.config\kiseki-terminal"
$script:KisekiCurrentFile = Join-Path $script:KisekiRoot 'current-theme.txt'

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
    <# .SYNOPSIS Register all Kiseki themes into Windows Terminal settings.json and activate one #>
    param(
        [Parameter(Mandatory)][string]$ThemeId,
        [double]$BgOpacity = -1
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
    $active = Get-KisekiThemeData -ThemeId $ThemeId
    $wallpaper = Join-Path $script:KisekiRoot "themes\$ThemeId\wallpaper.png"
    $finalBgOp = if ($BgOpacity -ge 0) { $BgOpacity } else { $active.terminal.backgroundImageOpacity }

    $wt.theme = $active.uiTheme.name
    $def = $wt.profiles.defaults
    $def | Add-Member -NotePropertyName colorScheme              -NotePropertyValue $active.scheme.name -Force
    $def | Add-Member -NotePropertyName cursorShape              -NotePropertyValue $active.terminal.cursorShape -Force
    $def | Add-Member -NotePropertyName opacity                  -NotePropertyValue $active.terminal.opacity -Force
    $def | Add-Member -NotePropertyName useAcrylic               -NotePropertyValue $active.terminal.useAcrylic -Force
    $def | Add-Member -NotePropertyName padding                  -NotePropertyValue $active.terminal.padding -Force
    $def | Add-Member -NotePropertyName backgroundImage          -NotePropertyValue $wallpaper -Force
    $def | Add-Member -NotePropertyName backgroundImageOpacity   -NotePropertyValue $finalBgOp -Force
    $def | Add-Member -NotePropertyName backgroundImageAlignment -NotePropertyValue $active.terminal.backgroundImageAlignment -Force
    $def | Add-Member -NotePropertyName backgroundImageStretchMode -NotePropertyValue 'uniformToFill' -Force
    $def | Add-Member -NotePropertyName intenseTextStyle         -NotePropertyValue 'all' -Force
    $def | Add-Member -NotePropertyName scrollbarState           -NotePropertyValue 'hidden' -Force
    $def | Add-Member -NotePropertyName bellStyle                -NotePropertyValue 'none' -Force
    $def | Add-Member -NotePropertyName unfocusedAppearance      -NotePropertyValue ([PSCustomObject]@{
        opacity                = $active.terminal.unfocusedOpacity
        backgroundImageOpacity = $active.terminal.unfocusedBgOpacity
    }) -Force
    $def | Add-Member -NotePropertyName font -NotePropertyValue ([PSCustomObject]@{
        face       = $active.font.face
        size       = $active.font.size
        cellHeight = $active.font.cellHeight
        weight     = $active.font.weight
    }) -Force

    $wt | ConvertTo-Json -Depth 12 | Set-Content $wtPath -Encoding utf8NoBOM
}

function Switch-KisekiTheme {
    <# .SYNOPSIS Switch active anime theme live (WT + Oh My Posh + Fastfetch + PSReadLine + FZF + EZA + Bat) #>
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
    Sync-KisekiWindowsTerminal -ThemeId $Name -BgOpacity $BgOpacity
    Apply-KisekiShellTheme -ThemeId $Name

    if (-not $Quiet) {
        $meta = Get-KisekiThemeData -ThemeId $Name
        Write-Host "✦ Switched to [$($meta.name)] — $($meta.subtitle)" -ForegroundColor Cyan
        if (Get-Command fastfetch -ErrorAction SilentlyContinue) {
            fastfetch --config $script:KisekiFastfetchConfig
        }
    }
}

function Get-KisekiThemes {
    <# .SYNOPSIS List all installed Kiseki anime terminal themes #>
    $active = Get-KisekiActiveThemeId
    Write-Host "`n★ Kiseki Anime Terminal Themes (switch with: theme <id>)`n" -ForegroundColor Cyan
    Get-ChildItem (Join-Path $script:KisekiRoot 'themes') -Directory | Sort-Object Name | ForEach-Object {
        $meta = Get-Content (Join-Path $_.FullName 'theme.json') -Raw -Encoding utf8 | ConvertFrom-Json
        $mark = if ($meta.id -eq $active) { '▶' } else { ' ' }
        $color = if ($meta.id -eq $active) { 'Magenta' } else { 'Gray' }
        Write-Host ("  {0} {1,-16} {2,-16} {3}" -f $mark, $meta.id, "[$($meta.style)]", $meta.subtitle) -ForegroundColor $color
    }
    Write-Host ""
}

function Toggle-RetroCRT {
    <# .SYNOPSIS Toggle subtle CRT phosphor bloom & scanlines in Windows Terminal #>
    $wtPath = Get-WindowsTerminalSettingsPath
    $json = Get-Content $wtPath -Raw -Encoding utf8 | ConvertFrom-Json
    $shader = Join-Path $script:KisekiRoot 'shaders\retro-crt-subtle.hlsl'
    $prop = 'experimental.pixelShaderPath'
    if ($json.profiles.defaults.PSObject.Properties[$prop] -and $json.profiles.defaults.$prop) {
        $json.profiles.defaults.PSObject.Properties.Remove($prop)
        Write-Host "✦ CRT Shader: OFF (razor-sharp mode)" -ForegroundColor Cyan
    } else {
        $json.profiles.defaults | Add-Member -NotePropertyName $prop -NotePropertyValue $shader -Force
        Write-Host "✦ CRT Shader: ON (subtle phosphor bloom + scanlines)" -ForegroundColor Magenta
    }
    $json | ConvertTo-Json -Depth 12 | Set-Content $wtPath -Encoding utf8NoBOM
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
    <# .SYNOPSIS Adjust background opacity or turn off wallpaper #>
    param(
        [ValidateRange(0.0, 1.0)][double]$Opacity = 0.40,
        [switch]$Off
    )
    $wtPath = Get-WindowsTerminalSettingsPath
    $json = Get-Content $wtPath -Raw -Encoding utf8 | ConvertFrom-Json
    if ($Off) {
        $json.profiles.defaults.PSObject.Properties.Remove('backgroundImage')
        Write-Host "✦ Terminal Background: OFF" -ForegroundColor Magenta
    } else {
        $active = Get-KisekiActiveThemeId
        $json.profiles.defaults | Add-Member -NotePropertyName backgroundImage -NotePropertyValue (Join-Path $script:KisekiRoot "themes\$active\wallpaper.png") -Force
        $json.profiles.defaults | Add-Member -NotePropertyName backgroundImageOpacity -NotePropertyValue $Opacity -Force
        Write-Host "✦ Terminal Background Opacity -> $Opacity" -ForegroundColor Magenta
    }
    $json | ConvertTo-Json -Depth 12 | Set-Content $wtPath -Encoding utf8NoBOM
}

Set-Alias theme    Switch-KisekiTheme -Scope Global
Set-Alias themes   Get-KisekiThemes   -Scope Global
Set-Alias crt      Toggle-RetroCRT    -Scope Global
Set-Alias termfont Set-TerminalFont   -Scope Global
Set-Alias termbg   Set-TerminalBg     -Scope Global
