# Kiseki Terminal v2 — Community Theme Authoring Guide

本文档说明如何从零创建、校验、调试并在本地安装一套符合 **Kiseki Terminal v2** 规范的主题包。

> **关于社区市场（Kiseki Gallery）**：Kiseki Gallery 随 **`v2.0.0`** 同步上线（完整架构见 [RFC-0006](./rfc/0006-community-theme-gallery.md)）。首次上架只需向独立索引仓 `nerv-base/kiseki-themes` 提交 **1 个独立 YAML 文件 PR**（`data/themes/<id>.yml`），不设仓库创建时长门槛；合入后你只需在自己仓库发 Tag/Release 或推送默认分支，市场定时探测会自动校验并更新，**后续发版永远无需再提 PR**。用户可通过官网 `#gallery`、终端 TUI（`kiseki` 按 `Tab`）或 `kiseki theme install <id>` 一键安装。

---

## 1. 快速上手（30 秒创建并验证）

```bash
# 1. 生成一套开箱即过检的暗色主题脚手架（加 --with-shader 会同时生成 GLSL + HLSL 模板）
kiseki theme init tokyo-midnight \
  --name "Tokyo Midnight" \
  --author "Your Name" \
  --appearance dark \
  --with-shader

# 如需浅色主题，使用 --appearance light
# kiseki theme init sakura-daylight --appearance light

# 2. 执行完整校验（目录结构 + Schema + naga Shader 编译与复杂度预算 + WCAG 2.1 对比度）
kiseki theme validate ./tokyo-midnight

# 3. 安装到本地社区主题目录并启用
kiseki theme install ./tokyo-midnight
kiseki use tokyo-midnight
```

更新主题内容后，重新安装覆盖只需加 `--force`：

```bash
kiseki theme validate ./tokyo-midnight && kiseki theme install ./tokyo-midnight --force
kiseki use tokyo-midnight
```

---

## 2. 主题包目录规范（白名单沙箱）

为确保第三方主题绝对安全，Kiseki 采用**严格白名单目录结构**（RFC-0004 §4.2）。任何不在白名单内的文件、符号链接或可执行脚本都会被 `kiseki theme validate` 直接拒绝：

```text
my-theme/
├── manifest.json         # 必填：身份元数据、明暗属性、声明层与来源证明
├── tokens.json           # 必填：5 层语义 Design Tokens（仅 L1 palette 必填）
├── LICENSE               # 必填：主题包开源许可证文本（如 MIT / Apache-2.0）
├── CREDITS.md            # 必填：壁纸、Shader 与字体来源/授权声明
├── assets/
│   ├── logo.txt          # 可选：Fastfetch ASCII/TrueColor Logo（<= 32 KiB，仅允许 SGR 转义）
│   └── wallpaper.png     # 可选：壁纸图片（.png / .jpg / .jpeg / .webp，宽高 <= 8192px）
├── previews/
│   └── preview.png       # 可选：展示预览图
└── shaders/
    ├── live.glsl         # 可选：Ghostty 参数化 GLSL 模板（<= 256 KiB）
    └── live.hlsl         # 可选：Windows Terminal 参数化 HLSL 模板（<= 256 KiB）
```

### 安全与体积硬限制

| 规则 | 限制 |
| :--- | :--- |
| 文件总数 / 总解包体积 | 最多 `64` 个文件，总大小 `<= 30 MiB` |
| 严禁符号链接与特殊文件 | 任意 symlink、FIFO、设备节点立即拒收 |
| 严禁可执行脚本 | 拒绝 `.sh` / `.zsh` / `.bash` / `.fish` / `.ps1` / `.bat` / `.cmd` / `.py` / `.lua` / `.js` / `.exe` 等 |
| Logo 终端安全审计 | `assets/logo.txt` 仅允许标准 SGR 颜色转义（`\x1b[...m`），禁止 OSC/CSI 光标劫持或终端命令注入 |
| 图片安全限制 | 仅允许 `.png`/`.jpg`/`.jpeg`/`.webp`，单边最大 `8192 px` |

---

## 3. 5 层语义 Token 体系（`tokens.json`）

完整 JSON Schema 见 [schemas/manifest.v1.schema.json](../schemas/manifest.v1.schema.json) 与 [schemas/tokens.v1.schema.json](../schemas/tokens.v1.schema.json)（详细设计见 [RFC-0002](./rfc/0002-theme-schema-and-tokens.md)）。

| 层级 | 键名 | 是否必填 | 作用 |
| :--- | :--- | :--- | :--- |
| **L1** | `palette` | **必填** | `surfaces`（8 级暗/亮阶）、`text`、`accents`、`semantic`、`interaction`、`ansi`（16 色） |
| **L2** | `syntax` / `diff` / `ui` | 可选 | 19 项代码语法色、Diff 色、窗口/标签/状态栏色；省略时由引擎从 L1 确定性推导 |
| **L3** | `atmosphere` | 可选 | 窗口透明度/模糊/内边距、壁纸路径/对齐/安全区（`codeSafeZone`）、动态 Shader 模板与参数 |
| **L4** | `typography` | 可选 | 推荐字体栈（使用 Font Catalog ID 如 `jetbrains-mono-nf`）、字号、行高、连字 |
| **L5** | `overrides` | 可选 | 声明式终端光标样式、Prompt 预设（`tactical-bar` / `shrine-ribbon` / `cinematic-line` / `pixel-frame`）、fzf 符标、Fastfetch 标题与字段名 |

> **零代码执行约束**：L5 `overrides.prompt` 仅支持声明式颜色与文字徽章，严禁注入 Oh My Posh `command` 段或 Starship `[custom.*]` 命令。

### WCAG 2.1 对比度门禁

`kiseki theme validate` 会自动计算关键配色对的 WCAG 2.1 对比度：
- 主文本 `text.primary` / 选区 / 光标文本等核心组合必须达到 **AA 最低门禁**（阻断级错误）；
- ANSI 彩色与语法色若偏低会给出警告提示；
- 当用户切换到 `kiseki profile accessibility` 时，引擎会自动沿 OKLCH 亮度轴将所有前景色提升至 **WCAG AAA (7.0:1)** 并关闭壁纸与 Shader。

---

## 4. 参数化 Shader 开发指南（GLSL / HLSL）

每个主题只需提供 **1 份 `shaders/live.glsl`**（Ghostty）与 **1 份 `shaders/live.hlsl`**（Windows Terminal），切勿硬编码主题颜色或为 `eco/balanced/ultra` 写三份副本。

编译期 `kiseki-shader` 会自动向模板顶部注入：

- 颜色宏：`KISEKI_BASE_BG`、`KISEKI_ACCENT_PRIMARY`、`KISEKI_ACCENT_SECONDARY`、`KISEKI_ACCENT_TERTIARY`、`KISEKI_TEXT_PRIMARY`
- 档位宏：`KISEKI_QUALITY_LEVEL`（`1` = Eco，`2` = Balanced，`3` = Ultra）
- 自定义参数宏：`tokens.json` 里 `atmosphere.shader.params` 中的 `camelCase` 键自动转为 `KISEKI_PARAM_<SCREAMING_SNAKE>`（例如 `pulseSpeed` → `KISEKI_PARAM_PULSE_SPEED`）
- 辅助函数：
  - `kiseki_bg_mask(term, uv)`：区分纯背景区域与文字字形，防止光效糊掉代码
  - `kiseki_narrow_factor(res)`：在窄分屏时平滑淡出右侧立绘与特效

### `naga` 校验与 TDR 安全红线

`kiseki theme validate` 会在 `eco` / `balanced` / `ultra` 三个档位下调用内嵌 `naga` 静态编译并检查：
1. **禁止反向 `smoothstep(a, b, x)`**（必须保证 `a < b`，避免跨 GPU 未定义行为）；
2. **禁止裸除法除 `alpha`**（反预乘必须写 `texel.rgb / max(texel.a, 0.001)`）；
3. **禁止动态/无限循环**，纹理采样与循环次数不得超过各档位 TDR 预算；
4. Ghostty 模板不要自己写 `#version` 或 `void main()`，直接实现 `void mainImage(out vec4 fragColor, in vec2 fragCoord)`。

参考官方主题的完整实现：[themes/nerv-eva/shaders/live.glsl](../themes/nerv-eva/shaders/live.glsl) 与 [themes/kyoto-sakura/shaders/live.glsl](../themes/kyoto-sakura/shaders/live.glsl)。

---

## 5. 版权、来源证明与字体规则

1. **必填 `LICENSE` 与 `CREDITS.md`**：所有主题包都必须包含清晰的许可证和素材署名；`manifest.json` 需填写 `provenance`（`wallpaperOrigin`: `original` | `cc0` | `cc-by` | `licensed` | `none`，`shaderOrigin`: `original` | `mit-adapted` | `none`）。
2. **禁止侵权素材**：严禁在主题包中打包未经授权的商用插画、动漫截图或无明确许可的第三方壁纸。
3. **严禁在主题包中塞入字体文件**：`.ttf` / `.otf` / `.woff2` 不在主题包允许扩展名之列。请在 `tokens.json` 的 `typography.families` 中引用 Font Catalog ID（`jetbrains-mono-nf`、`maple-mono-nf-cn`、`departure-mono-nf`、`fusion-pixel-12-zh`）或通用字体族名，引擎会自动按本机已装字体回退匹配。

---

## 6. 调试与跨宿主验证清单

在分享你的主题包之前，建议完整跑一遍以下检查：

```bash
# 1. 全量静态校验（输出 JSON 便于查看具体对比度与 Shader 预算）
kiseki theme validate ./my-theme --json

# 2. 本地安装并测试 5 档渲染配置
kiseki theme install ./my-theme --force
kiseki use my-theme --profile balanced
kiseki profile eco
kiseki profile ultra
kiseki profile static
kiseki profile accessibility

# 3. 混搭测试（确认你的 L1/L3 层可以被其他主题干净组合）
kiseki compose --palette my-theme --atmosphere starlight-8bit
kiseki compose --reset

# 4. 运行全链路诊断
kiseki doctor
```
