# ============================================================
# ★ Kiseki Terminal — Live Anime Theme Engine for Windows Terminal & PowerShell
# ============================================================

$script:KisekiRoot        = "$HOME\.config\kiseki-terminal"
$script:KisekiCurrentFile = Join-Path $script:KisekiRoot 'current-theme.txt'
$script:KisekiLiveFile    = Join-Path $script:KisekiRoot 'live-mode.txt'

function Write-KisekiUtf8NoBom {
    param([string]$Path, [string]$Content)
    $parent = Split-Path $Path -Parent
    if ($parent -and -not (Test-Path $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function Read-KisekiJsonFile {
    param([string]$Path)
    $raw = Get-Content $Path -Raw -Encoding utf8
    if ($PSVersionTable.PSEdition -ne 'Core') {
        # Windows PowerShell 5.1 ConvertFrom-Json rejects // comments & trailing commas found in WT settings.json
        $raw = [regex]::Replace($raw, '("([^"\\]|\\.)*")|//[^\r\n]*|/\*[\s\S]*?\*/', {
            param($m)
            if ($m.Groups[1].Success) { $m.Groups[1].Value } else { '' }
        })
        $raw = [regex]::Replace($raw, ',(\s*[\}\]])', '$1')
    }
    return ($raw | ConvertFrom-Json)
}

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

    $wt = Read-KisekiJsonFile -Path $wtPath
    if (-not $wt.PSObject.Properties['schemes']  -or $null -eq $wt.schemes)  { $wt | Add-Member -NotePropertyName schemes  -NotePropertyValue @() -Force }
    if (-not $wt.PSObject.Properties['themes']   -or $null -eq $wt.themes)   { $wt | Add-Member -NotePropertyName themes   -NotePropertyValue @() -Force }
    if (-not $wt.PSObject.Properties['profiles'] -or $null -eq $wt.profiles) { $wt | Add-Member -NotePropertyName profiles -NotePropertyValue ([PSCustomObject]@{ defaults = [PSCustomObject]@{} }) -Force }
    if (-not $wt.profiles.PSObject.Properties['defaults'] -or $null -eq $wt.profiles.defaults) { $wt.profiles | Add-Member -NotePropertyName defaults -NotePropertyValue ([PSCustomObject]@{}) -Force }

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

    $wt | Add-Member -NotePropertyName theme -NotePropertyValue $active.uiTheme.name -Force
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

    Write-KisekiUtf8NoBom -Path $wtPath -Content ($wt | ConvertTo-Json -Depth 12)
}

function Switch-KisekiTheme {
    <# .SYNOPSIS Switch active anime theme live, or run `theme update` to check/upgrade Kiseki Terminal #>
    [CmdletBinding(PositionalBinding = $false)]
    param(
        [Parameter(Position = 0)]
        [ArgumentCompleter({
            param($cmd, $param, $word)
            $items = @('update') + @(
                Get-ChildItem "$HOME\.config\kiseki-terminal\themes" -Directory -ErrorAction SilentlyContinue |
                    Select-Object -ExpandProperty Name
            )
            $items | Where-Object { $_ -like "$word*" }
        })]
        [string]$Name,
        [Parameter(Position = 1)]
        [ArgumentCompleter({
            param($cmd, $param, $word, $ast, $fakeBound)
            if ($fakeBound['Name'] -eq 'update') {
                @('check', 'force', 'edge', 'mute', 'unmute') | Where-Object { $_ -like "$word*" }
            }
        })]
        [string]$Action,
        [double]$BgOpacity = -1,
        [switch]$Static,
        [switch]$Quiet,
        [switch]$Check,
        [switch]$Force,
        [switch]$Edge,
        [switch]$Mute,
        [switch]$Unmute
    )

    if (-not $Name) {
        Get-KisekiThemes
        return
    }

    if ($Name -eq 'update') {
        $act = if ($Action) { $Action.ToLowerInvariant() } else { '' }
        Update-KisekiTerminal `
            -Check:($Check -or $act -eq 'check') `
            -Force:($Force -or $act -eq 'force') `
            -Edge:($Edge -or $act -eq 'edge') `
            -Mute:($Mute -or $act -eq 'mute') `
            -Unmute:($Unmute -or $act -eq 'unmute')
        return
    }

    $targetDir = Join-Path $script:KisekiRoot "themes\$Name"
    if (-not (Test-Path (Join-Path $targetDir 'theme.json'))) {
        Write-Host "✖ Unknown theme '$Name'. Available themes:" -ForegroundColor Red
        Get-KisekiThemes
        return
    }

    Write-KisekiUtf8NoBom -Path $script:KisekiCurrentFile -Content $Name
    if ($Static) {
        Write-KisekiUtf8NoBom -Path $script:KisekiLiveFile -Content 'off'
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
    Write-KisekiUtf8NoBom -Path $script:KisekiLiveFile -Content ($(if ($next) { 'on' } else { 'off' }))
    $active = Get-KisekiActiveThemeId
    Sync-KisekiWindowsTerminal -ThemeId $active -LiveMode $next
    if ($next) {
        Write-Host "✦ Live Dynamic Background: ON (60fps HLSL GPU shader for '$active')" -ForegroundColor Magenta
    } else {
        Write-Host "✦ Live Dynamic Background: OFF (static zero-GPU mode for '$active')" -ForegroundColor Cyan
    }
}

function ConvertTo-KisekiAnsiFg {
    param([string]$Hex)
    $h = ($Hex -replace '^#', '').Trim()
    if ($h.Length -ne 6) { return '' }
    $r = [Convert]::ToInt32($h.Substring(0, 2), 16)
    $g = [Convert]::ToInt32($h.Substring(2, 2), 16)
    $b = [Convert]::ToInt32($h.Substring(4, 2), 16)
    return "$([char]27)[38;2;${r};${g};${b}m"
}

function Get-KisekiThemeSwatch {
    param($Meta)
    $esc = [char]27
    $s = $Meta.scheme
    $cols = @($s.cursorColor, $s.red, $s.yellow, $s.green)
    $blocks = ($cols | ForEach-Object { "$(ConvertTo-KisekiAnsiFg $_)█" }) -join ''
    return "${blocks}${esc}[0m"
}

function Get-KisekiThemes {
    <# .SYNOPSIS Interactive arrow-key theme picker (Windows/macOS parity); pass -List for static output #>
    param([switch]$List)

    $dirs = @(Get-ChildItem (Join-Path $script:KisekiRoot 'themes') -Directory | Sort-Object Name)
    $items = @()
    foreach ($d in $dirs) {
        $mp = Join-Path $d.FullName 'theme.json'
        if (Test-Path $mp) {
            $items += (Get-Content $mp -Raw -Encoding utf8 | ConvertFrom-Json)
        }
    }
    if ($items.Count -eq 0) {
        Write-Host "✖ No themes found in $script:KisekiRoot\themes" -ForegroundColor Red
        return
    }

    $active = Get-KisekiActiveThemeId
    $modeTag = if (Get-KisekiLiveMode) { 'LIVE 60FPS' } else { 'STATIC' }
    $verTag = if (Get-Command Get-KisekiLocalVersionInfo -ErrorAction SilentlyContinue) {
        "v$((Get-KisekiLocalVersionInfo).version)"
    } else { '' }

    $canInteract = (-not $List) -and $Host.UI.SupportsVirtualTerminal -and (-not [Console]::IsInputRedirected) -and (-not [Console]::IsOutputRedirected)
    if (-not $canInteract) {
        Write-Host "`n★ Kiseki Anime Terminal Themes $verTag [$modeTag]  (switch: theme <id> | motion: live | update: theme update)`n" -ForegroundColor Cyan
        foreach ($meta in $items) {
            $mark  = if ($meta.id -eq $active) { '●' } else { ' ' }
            $color = if ($meta.id -eq $active) { 'Magenta' } else { 'Gray' }
            $sw    = Get-KisekiThemeSwatch $meta
            Write-Host ("  {0} {1,-16} {2}  [{3,-25}] {4}" -f $mark, $meta.id, $sw, $meta.style, $meta.subtitle) -ForegroundColor $color
        }
        Write-Host ""
        return
    }

    $esc           = [char]27
    $initialActive = $active
    $activeId      = $active
    $previewedId   = $null
    $selectedIdx   = 0
    for ($i = 0; $i -lt $items.Count; $i++) {
        if ($items[$i].id -eq $active) { $selectedIdx = $i; break }
    }

    $totalLines = $items.Count + 4
    $drawn      = $false
    $chosen     = $null

    $render = {
        $sb = [System.Text.StringBuilder]::new()
        if ($drawn) {
            [void]$sb.Append("${esc}[${totalLines}A`r")
        }
        $lines = @(
            "${esc}[1;36m★ Kiseki Anime Terminal Themes $verTag [$modeTag]${esc}[0m",
            "${esc}[90m  ↑/↓ (j/k) 选择  ·  Enter 切换  ·  Space 实时预览  ·  1-9 快捷  ·  q/Esc 退出${esc}[0m",
            ""
        )
        for ($i = 0; $i -lt $items.Count; $i++) {
            $meta = $items[$i]
            $sw   = Get-KisekiThemeSwatch $meta
            $dot  = if ($meta.id -eq $activeId) { "${esc}[32m●${esc}[0m" } else { ' ' }
            if ($i -eq $selectedIdx) {
                $accent = ConvertTo-KisekiAnsiFg $meta.scheme.cursorColor
                $lines += ("  ${accent}${esc}[1m❯${esc}[0m $dot ${accent}${esc}[1m{0,-16}${esc}[0m $sw  ${esc}[1;97m[{1,-25}]${esc}[0m ${esc}[97m{2}${esc}[0m" -f $meta.id, $meta.style, $meta.subtitle)
            } else {
                $lines += ("    $dot ${esc}[37m{0,-16}${esc}[0m $sw  ${esc}[90m[{1,-25}] {2}${esc}[0m" -f $meta.id, $meta.style, $meta.subtitle)
            }
        }
        if ($previewedId) {
            $lines += "  ${esc}[35m✦ 正在预览: $previewedId（按 Enter 确认，Esc 恢复 $initialActive）${esc}[0m"
        } else {
            $lines += ""
        }
        foreach ($ln in $lines) {
            [void]$sb.Append("${esc}[2K${ln}`r`n")
        }
        [Console]::Write($sb.ToString())
        $drawn = $true
    }

    try {
        [Console]::Write("${esc}[?25l")
        . $render
        while ($true) {
            $key = [Console]::ReadKey($true)
            $k = $key.Key
            $c = $key.KeyChar
            if ($k -eq [ConsoleKey]::UpArrow -or $c -eq 'k' -or $c -eq 'K') {
                $selectedIdx = ($selectedIdx - 1 + $items.Count) % $items.Count
                . $render
            } elseif ($k -eq [ConsoleKey]::DownArrow -or $c -eq 'j' -or $c -eq 'J') {
                $selectedIdx = ($selectedIdx + 1) % $items.Count
                . $render
            } elseif ($k -eq [ConsoleKey]::Spacebar) {
                $previewedId = $items[$selectedIdx].id
                $activeId    = $previewedId
                Write-KisekiUtf8NoBom -Path $script:KisekiCurrentFile -Content $previewedId
                Sync-KisekiWindowsTerminal -ThemeId $previewedId
                Apply-KisekiShellTheme -ThemeId $previewedId
                . $render
            } elseif ($k -eq [ConsoleKey]::Enter) {
                $chosen = $items[$selectedIdx].id
                break
            } elseif ($k -eq [ConsoleKey]::Escape -or $c -eq 'q' -or $c -eq 'Q') {
                if ($previewedId -and $previewedId -ne $initialActive) {
                    Write-KisekiUtf8NoBom -Path $script:KisekiCurrentFile -Content $initialActive
                    Sync-KisekiWindowsTerminal -ThemeId $initialActive
                    Apply-KisekiShellTheme -ThemeId $initialActive
                }
                $chosen = $null
                break
            } elseif ($c -ge '1' -and $c -le '9') {
                $num = ([int][char]$c) - ([int][char]'1')
                if ($num -ge 0 -and $num -lt $items.Count) {
                    $selectedIdx = $num
                    . $render
                }
            }
        }
    } finally {
        if ($drawn) {
            [Console]::Write("${esc}[${totalLines}A`r${esc}[J")
        }
        [Console]::Write("${esc}[?25h")
    }

    if ($chosen) {
        Switch-KisekiTheme -Name $chosen
    }
}

function Toggle-RetroCRT {
    <# .SYNOPSIS Toggle static subtle CRT phosphor bloom & scanlines in Windows Terminal #>
    $wtPath = Get-WindowsTerminalSettingsPath
    $json = Read-KisekiJsonFile -Path $wtPath
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
        $json = Read-KisekiJsonFile -Path $wtPath
        if ($json.profiles.defaults.PSObject.Properties[$imgProp]) { $json.profiles.defaults.PSObject.Properties.Remove($imgProp) }
        $json.profiles.defaults | Add-Member -NotePropertyName $prop -NotePropertyValue $shader -Force
        Write-KisekiUtf8NoBom -Path $wtPath -Content ($json | ConvertTo-Json -Depth 12)
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
    $json = Read-KisekiJsonFile -Path $wtPath
    if (-not $json.profiles.defaults.PSObject.Properties['font'] -or $null -eq $json.profiles.defaults.font) {
        $json.profiles.defaults | Add-Member -NotePropertyName font -NotePropertyValue ([PSCustomObject]@{}) -Force
    }
    $json.profiles.defaults.font | Add-Member -NotePropertyName face -NotePropertyValue $map[$Style] -Force
    $json.profiles.defaults.font | Add-Member -NotePropertyName size -NotePropertyValue $Size -Force
    Write-KisekiUtf8NoBom -Path $wtPath -Content ($json | ConvertTo-Json -Depth 12)
    Write-Host "✦ Terminal Font -> $Style ($Size pt)" -ForegroundColor Cyan
}

function Set-TerminalBg {
    <# .SYNOPSIS Adjust static background opacity or turn off wallpaper (`termbg 0.35`, `termbg off`, or `termbg -Off`) #>
    param(
        [Parameter(Position = 0)][string]$Opacity = '0.40',
        [switch]$Off
    )
    $active = Get-KisekiActiveThemeId
    if ($Off -or ($Opacity -and $Opacity.Trim().ToLowerInvariant() -eq 'off')) {
        Write-KisekiUtf8NoBom -Path $script:KisekiLiveFile -Content 'off'
        Sync-KisekiWindowsTerminal -ThemeId $active -LiveMode $false
        $wtPath = Get-WindowsTerminalSettingsPath
        $json = Read-KisekiJsonFile -Path $wtPath
        if ($json.profiles.defaults.PSObject.Properties['backgroundImage']) {
            $json.profiles.defaults.PSObject.Properties.Remove('backgroundImage')
        }
        Write-KisekiUtf8NoBom -Path $wtPath -Content ($json | ConvertTo-Json -Depth 12)
        Write-Host "✦ Terminal Background: OFF" -ForegroundColor Magenta
    } else {
        $val = 0.40
        if (-not [double]::TryParse($Opacity, [System.Globalization.NumberStyles]::Float, [System.Globalization.CultureInfo]::InvariantCulture, [ref]$val) -or $val -lt 0.0 -or $val -gt 1.0) {
            Write-Host "✖ Opacity must be a number between 0.0 and 1.0 (or 'off')." -ForegroundColor Red
            return
        }
        Write-KisekiUtf8NoBom -Path $script:KisekiLiveFile -Content 'off'
        Sync-KisekiWindowsTerminal -ThemeId $active -BgOpacity $val -LiveMode $false
        Write-Host "✦ Terminal Static Background Opacity -> $val (run 'live' to re-enable 60fps motion)" -ForegroundColor Magenta
    }
}

Set-Alias theme    Switch-KisekiTheme -Scope Global
Set-Alias themes   Get-KisekiThemes   -Scope Global
Set-Alias live     Toggle-KisekiLive  -Scope Global
Set-Alias motion   Toggle-KisekiLive  -Scope Global
Set-Alias crt      Toggle-RetroCRT    -Scope Global
Set-Alias termfont Set-TerminalFont   -Scope Global
Set-Alias termbg   Set-TerminalBg     -Scope Global

# Load zero-lag update notifier & `theme-update` self-updater
$script:KisekiUpdaterPath = Join-Path $script:KisekiRoot 'scripts\updater.ps1'
if (Test-Path $script:KisekiUpdaterPath) {
    . $script:KisekiUpdaterPath
}
