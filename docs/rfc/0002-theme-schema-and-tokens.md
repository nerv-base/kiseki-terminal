# RFC-0002: 5-Layer Composable Theme Specification & Semantic Tokens

- **Status**: Draft
- **Schema Version**: `1`（`schemas/manifest.v1.schema.json`, `schemas/tokens.v1.schema.json`）
- **Related**: [RFC-0001](./0001-architecture-and-workspace.md), [RFC-0003](./0003-adapter-contract-and-capabilities.md), [RFC-0004](./0004-shader-pipeline-and-security.md)

---

## 1. 设计哲学：5 层可组合设计系统

为了同时满足「**极简社区纯配色主题 5 分钟可上手**」与「**官方旗舰主题全维度深度定制 + 用户跨主题混搭**」，Kiseki v2 将主题数据模型拆分为 **5 个正交层级**：

| 层级 | 名称 | 是否必填 | 职责范围 |
| :--- | :--- | :---: | :--- |
| **L1** | `palette`（Core Palette） | **必填** | `appearance (dark\|light)`、层级背景面（Surface）、文字层级、主强调色、语义状态色、16 色 ANSI、光标与选区 |
| **L2** | `syntax` & `ui`（Syntax & Chrome） | 可选 | 代码语法高亮（19 个语义槽）、Diff 高亮、窗口边框/标签栏/状态栏/面板色；**缺省时由编译器从 L1 自动推导** |
| **L3** | `atmosphere`（Wallpaper, Blur & Shader） | 可选 | 窗口透明度/模糊/内边距、分区壁纸与代码安全区元数据、参数化 GLSL/HLSL Shader 声明及默认渲染档位 |
| **L4** | `typography`（Font Stack） | 可选 | 语义字体栈（等宽主体 + Nerd Font + CJK 回退）、参考字号、行高、字重、连字与官方字体目录 ID 引用 |
| **L5** | `overrides`（Safe Declarative Overrides） | 可选 | 受严格白名单沙箱约束的应用专属视觉精调（Prompt 装饰段/符号、FZF 图标边框、Fastfetch 标语/徽章、光标形状等，**严禁可执行代码**） |

### 为什么这能同时解决扩展性与混搭？
- **零门槛社区生态**：只想贡献一套精妙配色的作者只需填写 L1（约 30 个 hex 颜色），Theme Compiler 即可通过 OKLCH 感知色空间确定性推导出完整 L2，立即覆盖全部 20+ 终端与应用。
- **按层混搭（Composition）**：`kiseki compose --palette nerv-eva --atmosphere shinkai-rain --typography starlight-8bit` 可以直接把不同主题的对应层拼接成一份 `ResolvedTheme`，再交给同一套 Compiler 生成全部配置。
- **原生明暗配对（Light/Dark）**：每个主题在 `manifest.json` 显式声明 `appearance: "dark" | "light"`，并可通过 `pairedTheme` 指向对应的日间/夜间主题，支持未来跟随 macOS/Windows 系统外观一键切换。

---

## 2. 主题包标准目录结构

```text
nerv-eva/
├── manifest.json               # 包身份、版本、appearance、支持层级、资产索引、许可证元数据（必填）
├── tokens.json                 # L1–L5 声明式视觉 Token（必填）
├── assets/                     # 静态视觉资源（仅当启用 L3/L5 时存在）
│   ├── wallpaper.png           # 主壁纸（推荐 16:9 / 2560x1440+，左侧代码可读区暗化处理）
│   ├── wallpaper-portrait.png  # 可选：竖屏/窄屏专用构图
│   └── logo.txt                # 可选：Fastfetch 纯文本/ANSI 徽章
├── shaders/                    # 参数化单模板 Shader（仅当启用 L3 动态效果时存在）
│   ├── live.glsl               # Ghostty 参数化 GLSL 模板（经宏编译产出 Eco/Balanced/Ultra）
│   └── live.hlsl               # Windows Terminal 参数化 HLSL 模板
├── previews/                   # 预览图（用于 Gallery / 文档展示）
│   └── preview.png
├── LICENSE                     # 必填：主题包及原创资产的许可证全文
└── CREDITS.md                  # 必填：壁纸/配色灵感/字体上游出处与授权声明
```

> **安全注意**：目录中**不再存在** `overrides/terminals/wezterm.lua` 或原始 `prompt.omp.json` 等可含任意代码执行的自由文件。所有允许的声明式精调统一收口在经过 JSON Schema 严格校验的 `tokens.json#overrides`（L5）内（详见 [RFC-0004](./0004-shader-pipeline-and-security.md)）。

---

## 3. Token 体系详细规范

### 3.1 L1: Core Palette（必填）

受 Catppuccin 的清晰层级与 Base24 的 ANSI 映射启发，L1 分为 6 个子组，所有颜色值必须严格符合 `#RRGGBB`（6 位十六进制，禁止含换行或 alpha 混写）：

1. **`surfaces`（背景层级，由深到浅 / 浅色主题由浅到深）**
   - `crust`：最深外框/Tab Bar 底色
   - `mantle`：次级面板/非焦点 Tab/侧栏底色
   - `base`：主终端/编辑器画布背景色（对应原 `scheme.background`）
   - `surface0`：轻度抬升面（活跃行、浮动窗口背景、当前焦点 Tab）
   - `surface1`：中度抬升面（输入框、高亮选中块）
   - `surface2`：边框、分割线、滚动条滑块
   - `overlay0` / `overlay1`：弱化提示文字、行号、占位符、不可见字符

2. **`text`（文字层级）**
   - `primary`：主正文前景（对应原 `scheme.foreground`）
   - `secondary`：次要正文/子标题/非关键元信息
   - `muted`：注释、时间戳、低优先级提示
   - `inverse`：用于亮色强调背景（如光标字符、Badge）上的反色文字

3. **`accents`（主题辨识度核心色）**
   - `primary`：主题第一视觉主色（如 `nerv-eva` 的 `#F06A22` 橙、`kyoto-sakura` 的 `#FF9EC7` 樱粉）
   - `secondary`：第二辅色（如 `nerv-eva` 的 `#D8262E` 绯红）
   - `tertiary`：第三点缀色（如 `nerv-eva` 的 `#62C6F2` 战术蓝）

4. **`semantic`（状态语义色）**
   - `error`、`warning`、`success`、`info`、`hint`

5. **`interaction`（光标与选区）**
   - `cursor`、`cursorText`
   - `selectionBg`、`selectionFg`
   - `matchBg`、`matchFg`（搜索匹配高亮，供 fzf / 编辑器 / delta 共用）

6. **`ansi`（标准 16 色终端调色板）**
   - `black`、`red`、`green`、`yellow`、`blue`、`magenta`、`cyan`、`white`
   - `brightBlack`、`brightRed`、`brightGreen`、`brightYellow`、`brightBlue`、`brightMagenta`、`brightCyan`、`brightWhite`

---

### 3.2 L2: Syntax & UI（可选，缺省时从 L1 确定性推导）

#### `syntax`（19 个通用语法槽，面向 Neovim/Vim/Helix/bat/zsh-syntax-highlighting/PSReadLine）
`keyword`、`operator`、`function`、`method`、`type`、`variable`、`parameter`、`property`、`string`、`escape`、`number`、`boolean`、`constant`、`comment`、`punctuation`、`tag`、`attribute`、`macro`、`invalid`

#### `diff`（面向 delta / lazygit / Neovim / Helix）
`addFg`、`addBg`、`deleteFg`、`deleteBg`、`changeFg`、`changeBg`、`inlineAddBg`、`inlineDeleteBg`

#### `ui`（面向终端窗口铬件、tmux、Zellij、btop、OpenCode）
`windowFrame`、`windowFrameUnfocused`、`tabBarBg`、`tabBarUnfocusedBg`、`tabActiveBg`、`tabActiveFg`、`tabInactiveBg`、`tabInactiveFg`、`statuslineBg`、`statuslineFg`、`border`、`borderFocused`、`panelBg`

#### L1 → L2 确定性推导规则（当主题未提供 L2 字段时）
Compiler 在 OKLCH 色彩空间执行纯函数推导（示例）：
- `syntax.keyword` ← `accents.secondary`（或 `ansi.magenta`）
- `syntax.function` ← `accents.primary`（或 `ansi.blue`）
- `syntax.string` ← `ansi.green`
- `syntax.number` / `boolean` ← `ansi.yellow`
- `syntax.type` ← `ansi.cyan`
- `syntax.comment` ← `text.muted`（并自动钳位确保与 `surfaces.base` 对比度 ≥ 3.0）
- `diff.addBg` ← `mix_oklch(surfaces.base, semantic.success, 0.16)`
- `diff.deleteBg` ← `mix_oklch(surfaces.base, semantic.error, 0.16)`
- `ui.tabBarBg` ← `surfaces.crust`；`ui.tabActiveBg` ← `surfaces.surface0`；`ui.tabInactiveBg` ← `surfaces.mantle`

---

### 3.3 L3: Atmosphere（可选，沉浸式氛围核心层）

```json
"atmosphere": {
  "window": {
    "opacity": 0.95,
    "unfocusedOpacity": 0.88,
    "blur": true,
    "blurRadius": 20,
    "useMica": false,
    "padding": { "x": 18, "y": 14 }
  },
  "wallpaper": {
    "path": "assets/wallpaper.png",
    "opacity": 0.78,
    "unfocusedOpacity": 0.45,
    "alignment": "bottom-right",
    "fit": "cover",
    "codeSafeZone": {
      "side": "left",
      "startX": 0.0,
      "endX": 0.56
    },
    "variants": {
      "portrait": "assets/wallpaper-portrait.png"
    }
  },
  "shader": {
    "glsl": "shaders/live.glsl",
    "hlsl": "shaders/live.hlsl",
    "defaultProfile": "balanced",
    "params": {
      "accentPrimary": "#F06A22",
      "accentSecondary": "#D8262E",
      "scanPeriodSec": 7.5,
      "glitchIntervalSec": 6.0,
      "narrowFadeAspect": 1.15
    }
  }
}
```

- **`codeSafeZone`**：明确声明壁纸的文字阅读安全区与立绘区边界，Shader 预处理自动将其编译为 `KISEKI_SAFE_ZONE_END_X` 等常量，**替代 v1 中散落在各 shader 里的硬编码魔数**。
- **`narrowFadeAspect`**：当窗口宽高比（`width / height`）小于该阈值（例如窄分屏）时，Shader 自动平滑降低立绘区特效与背景亮度，零常驻进程解决窄窗口构图冲突。

---

### 3.4 L4: Typography（可选，跨平台字体栈）

```json
"typography": {
  "style": "modern",
  "families": [
    "jetbrains-mono-nf",
    "maple-mono-nf-cn"
  ],
  "sizePt": 11.0,
  "lineHeight": 1.26,
  "weight": "normal",
  "ligatures": true
}
```

- `families` 既支持引用 Core 内置 `font-catalog.json` 的标准化 Font ID（如 `"departure-mono-nf"`、`"fusion-pixel-12-zh"`、`"maple-mono-nf-cn"`，由 Core 自动解析为 Windows DirectWrite / macOS CoreText / PostScript / Linux Fontconfig 的精确字体名），也支持填入自由字体族名字符串作为回退。
- `sizePt` 以 Windows 96 DPI 为基准点；macOS（72 DPI CoreText）按平台缩放系数（默认 `× 1.25` 并对齐 `0.5pt`）自动换算，允许用户在 `config.toml` 微调全局 `dpiScale`。

---

### 3.5 L5: Safe Declarative Overrides（可选，白名单沙箱）

所有 L5 覆盖均**严禁包含可执行脚本、命令模板或任意配置文件片段**，仅接受结构化视觉元数据：

- **`terminal`**：`cursorShape`（`block | block-hollow | bar | underline`）、`cursorBlink`（`bool`）、`macosTitlebarStyle`（`transparent | tabs | native`）。
- **`prompt`**：
  - `preset`：引用内置安全模板骨架（如 `"tactical-bar"`、`"minimal-pill"`、`"pixel-frame"`、`"shrine-ribbon"`）；
  - `badgeText`：纯显示徽标（严格正则限制长度 ≤ 24、禁止控制字符与转义序列，如 `"02"`、`"★"`）；
  - `promptSymbol` / `errorSymbol`（如 `"▸"`、`"❀"`）；
  - `consoleTitlePrefix`（如 `"UNIT-02"`）；
  - `segmentColors`：各内置安全段（`path`、`git`、`lang`、`time`、`status`、`duration`）的前景/背景色覆盖。
  - **重要**：编译器将 `tokens.json#overrides.prompt` 分别编译为 **Oh My Posh JSON** 和 **Starship TOML**，从而一个主题同时天然支持两大 Prompt 引擎，且 **0 任意命令执行风险**！
- **`fzf`**：`border`（`sharp | rounded | bold | none`）、`prompt`、`pointer`、`marker`（均限制短安全字符）。
- **`fastfetch`**：`logoPath`（相对包内纯文本路径）、`headerTitle`、`accentSwatchColors`、`fieldLabels`（仅允许重命名静态标签文字，如 `CPU -> MAGI`、`Memory -> LCL`，**不允许添加 `command` 模块**）。

---

## 4. 可读性与对比度门禁（WCAG 2.1 + APCA）

`kiseki theme validate` 与 CI 门禁对每个主题自动运行对比度检查：

| 检查项 | 最低门槛（Error） | 推荐标准（Warning） |
| :--- | :---: | :---: |
| `text.primary` vs `surfaces.base` | WCAG ≥ **7.0:1**（AAA） | APCA Lc ≥ 75 |
| `text.secondary` vs `surfaces.base` | WCAG ≥ **4.5:1**（AA） | APCA Lc ≥ 60 |
| `text.muted` / `syntax.comment` vs `surfaces.base` | WCAG ≥ **3.0:1** | ≥ 3.8:1 |
| `ansi.{red,green,yellow,blue,magenta,cyan}` vs `surfaces.base` | WCAG ≥ **4.0:1** | ≥ 4.5:1 |
| `interaction.selectionFg` vs `interaction.selectionBg` | WCAG ≥ **4.5:1** | ≥ 6.0:1 |
| `interaction.cursorText` vs `interaction.cursor` | WCAG ≥ **4.5:1** | — |
| `Accessibility` 渲染档位 | 强制关闭壁纸/Shader/透明，并将不达 7.0:1 的语义色在 OKLCH 亮度轴自动拉升至 AAA | — |

---

## 5. 混搭合成解析优先级（Resolution Order）

当 Compiler 构建最终 `ResolvedTheme` 时，按以下严格顺序合并（后者覆盖前者）：

1. **Base Theme**：`state.activeTheme` 完整 5 层数据（L2 缺失项由 L1 自动推导）。
2. **Layer Composition**：`state.composition.{palette, syntax, atmosphere, typography, overrides}` 指定的外部主题对应层（若替换了 `palette` 且未指定独立 `syntax`，则重新从新 `palette` 推导缺失的 `syntax`）。
3. **Rendering Profile Transform**：应用 `static / eco / balanced / ultra / accessibility` 变换规则（例如 `static` 移除 Shader、`accessibility` 禁用壁纸并自动提升对比度）。
4. **User Explicit Overrides**：`state.rendering.bgOpacityOverride`、`state.rendering.crt`、`state.typography.{locked, styleOverride, sizeOverridePt}`。
5. **Capability Negotiation**：由各目标 Adapter 根据当前终端/应用的实际能力集合完成最终降级与映射（详见 [RFC-0003](./0003-adapter-contract-and-capabilities.md)）。

---

## 6. 完整官方主题示例：`nerv-eva`（v2 格式）

### `themes/nerv-eva/manifest.json`

```json
{
  "$schema": "../../schemas/manifest.v1.schema.json",
  "schemaVersion": 1,
  "id": "nerv-eva",
  "name": "NERV UNIT-02",
  "subtitle": "特務機関 NERV 弐号機 · 战术蓝图暗红橙",
  "version": "2.0.0",
  "appearance": "dark",
  "pairedTheme": null,
  "tags": ["anime", "mecha", "tactical", "dark", "live-shader"],
  "authors": [
    { "name": "nerv-base", "url": "https://github.com/nerv-base" }
  ],
  "license": "MIT",
  "creditsFile": "CREDITS.md",
  "layers": ["palette", "syntax", "ui", "atmosphere", "typography", "overrides"],
  "minEngineVersion": "2.0.0",
  "preview": "previews/preview.png"
}
```

### `themes/nerv-eva/tokens.json`（核心节选）

```json
{
  "$schema": "../../schemas/tokens.v1.schema.json",
  "schemaVersion": 1,
  "palette": {
    "surfaces": {
      "crust": "#070506",
      "mantle": "#0E0B0C",
      "base": "#0B090A",
      "surface0": "#171214",
      "surface1": "#241418",
      "surface2": "#55383B",
      "overlay0": "#6F6462",
      "overlay1": "#A59A93"
    },
    "text": {
      "primary": "#ECE4DA",
      "secondary": "#DDD3C9",
      "muted": "#8C7D78",
      "inverse": "#0B090A"
    },
    "accents": {
      "primary": "#F06A22",
      "secondary": "#D8262E",
      "tertiary": "#62C6F2"
    },
    "semantic": {
      "error": "#F0413C",
      "warning": "#F5C96A",
      "success": "#3FE08A",
      "info": "#62C6F2",
      "hint": "#FF8A3D"
    },
    "interaction": {
      "cursor": "#F06A22",
      "cursorText": "#0B090A",
      "selectionBg": "#4E1218",
      "selectionFg": "#FFF7F2",
      "matchBg": "#4A2410",
      "matchFg": "#FFC08F"
    },
    "ansi": {
      "black": "#171214",
      "red": "#D8262E",
      "green": "#3FE08A",
      "yellow": "#F06A22",
      "blue": "#62C6F2",
      "magenta": "#E05A6D",
      "cyan": "#FF8A3D",
      "white": "#DDD3C9",
      "brightBlack": "#7D706B",
      "brightRed": "#F0413C",
      "brightGreen": "#86F7BB",
      "brightYellow": "#F5C96A",
      "brightBlue": "#8FD8FA",
      "brightMagenta": "#FFA58F",
      "brightCyan": "#FFC08F",
      "brightWhite": "#FFF7F2"
    }
  },
  "syntax": {
    "keyword": "#FF5A52",
    "operator": "#F06A22",
    "function": "#FFC08F",
    "method": "#FFC08F",
    "type": "#F0B53A",
    "variable": "#ECE4DA",
    "parameter": "#F5C96A",
    "property": "#62C6F2",
    "string": "#7FDCA4",
    "escape": "#86F7BB",
    "number": "#FF9A52",
    "boolean": "#FF9A52",
    "constant": "#FFA58F",
    "comment": "#8C7D78",
    "punctuation": "#A59A93",
    "tag": "#D8262E",
    "attribute": "#F5C96A",
    "macro": "#FF8A3D",
    "invalid": "#F0413C"
  },
  "ui": {
    "windowFrame": "#55383B",
    "windowFrameUnfocused": "#24191B",
    "tabBarBg": "#070506",
    "tabBarUnfocusedBg": "#050405",
    "tabActiveBg": "#171214",
    "tabActiveFg": "#FFF7F2",
    "tabInactiveBg": "#0E0B0C",
    "tabInactiveFg": "#7D706B",
    "statuslineBg": "#171214",
    "statuslineFg": "#ECE4DA",
    "border": "#55383B",
    "borderFocused": "#F06A22",
    "panelBg": "#120D0E"
  },
  "atmosphere": {
    "window": {
      "opacity": 0.95,
      "unfocusedOpacity": 0.88,
      "blur": true,
      "blurRadius": 20,
      "useMica": false,
      "padding": { "x": 18, "y": 14 }
    },
    "wallpaper": {
      "path": "assets/wallpaper.png",
      "opacity": 0.78,
      "unfocusedOpacity": 0.45,
      "alignment": "bottom-right",
      "fit": "cover",
      "codeSafeZone": { "side": "left", "startX": 0.0, "endX": 0.56 }
    },
    "shader": {
      "glsl": "shaders/live.glsl",
      "hlsl": "shaders/live.hlsl",
      "defaultProfile": "balanced",
      "params": {
        "scanPeriodSec": 7.5,
        "glitchIntervalSec": 6.0,
        "narrowFadeAspect": 1.15
      }
    }
  },
  "typography": {
    "style": "modern",
    "families": ["jetbrains-mono-nf", "maple-mono-nf-cn"],
    "sizePt": 11.0,
    "lineHeight": 1.26,
    "weight": "normal",
    "ligatures": true
  },
  "overrides": {
    "terminal": {
      "cursorShape": "block",
      "cursorBlink": false,
      "macosTitlebarStyle": "transparent"
    },
    "prompt": {
      "preset": "tactical-bar",
      "badgeText": "02",
      "okLabel": "[弐号機]",
      "errorLabel": "[精神汚染]",
      "promptSymbol": "▸",
      "consoleTitlePrefix": "UNIT-02"
    },
    "fzf": {
      "border": "sharp",
      "prompt": "02▸ ",
      "pointer": "▸",
      "marker": "■"
    },
    "fastfetch": {
      "logoPath": "assets/logo.txt",
      "headerTitle": "EVANGELION UNIT-02 // 弐号機",
      "fieldLabels": {
        "title": "PILOT",
        "os": "SYS",
        "shell": "SHELL",
        "terminal": "PLUG",
        "cpu": "MAGI",
        "memory": "LCL",
        "uptime": "POWER"
      }
    }
  }
}
```

> 注：v2 Schema 统一采用标准 ANSI 命名 `brightMagenta`（迁移工具会自动兼容 v1 的 `brightPurple`）。具体校验约束见 [schemas/manifest.v1.schema.json](../../schemas/manifest.v1.schema.json) 与 [schemas/tokens.v1.schema.json](../../schemas/tokens.v1.schema.json)。
