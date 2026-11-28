<p align="center">
  <img src="./assets/hero-v2.png" alt="Kiseki Terminal v2 — Universal Anime Terminal Theming Engine" width="100%" />
</p>

<p align="center">
  <a href="https://kisekiterm.dev"><img src="https://img.shields.io/badge/Docs-kisekiterm.dev-66f0ff?style=flat-square" alt="Website" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/License-MIT-ff82b8?style=flat-square" alt="License: MIT" /></a>
  <img src="https://img.shields.io/badge/Engine-Rust_Single_Binary-f06a22?style=flat-square" alt="Rust Engine" />
  <img src="https://img.shields.io/badge/Shaders-GLSL_%2F_HLSL_naga_Validated-b48efe?style=flat-square" alt="naga Validated Shaders" />
  <img src="https://img.shields.io/badge/WCAG_2.1-Verified-3fe08a?style=flat-square" alt="WCAG Verified" />
</p>

> **Kiseki Terminal v2** — 面向终端生态的统一视觉与沉浸式主题引擎（Rust 单二进制，零运行时脚本依赖）。  
> 5 层语义设计 Token · 按层混搭（`compose`）· 参数化 GLSL/HLSL 动态 Shader（内置 `naga` 校验）· 非侵入式事务挂载与一键回滚 · 社区主题包安全沙箱校验。

---

## 核心特性（v2 架构）

- **单二进制离线开箱即用**：内置全部 4 套官方主题包（壁纸 / TrueColor Logo / GLSL+HLSL Shader / 5 层 Token），无需 Python 或外部运行时。
- **零侵入挂载 & 事务可回滚**：宿主配置仅通过外部 `include` / 标记块 / 无损 JSONC 叶子补丁 / Parent Profile 继承挂载，每次变更写入事务日志，支持 `kiseki rollback` 一键还原与 `kiseki recover --safe` 紧急逃生。
- **绝不劫持终端行为**：只接管视觉层（配色 / 字体 / 壁纸 / Shader / Prompt / Fastfetch / 命令行语法高亮色板），绝不篡改 `ls`/`cat`/`cd` 别名、键位绑定或用户 Shell 行为；提供 `--render-only` 纯产物模式适配 Nix / Home Manager / Dotfiles。
- **零 fork 热同步**：Zsh / Bash / Fish / PowerShell 钩子在无变更热路径仅读取 `state.gen` 整数比较，零子进程开销；切换主题后终端（Ghostty `SIGUSR2` / Kitty `SIGUSR1` / WT·WezTerm·iTerm2·Alacritty 自动监听）、Oh My Posh Prompt、Fastfetch 与命令语法高亮即时联动。
- **强安全社区主题包**：纯声明式 JSON + `naga` Shader 静态编译与 TDR 复杂度预算 + WCAG 2.1 对比度门禁 + 白名单扩展名 / 拒绝符号链接与任何可执行脚本；内置 `kiseki theme init` 一键生成过检脚手架。

---

## 支持宿主与生态矩阵

| 层级 | 类别 | 支持目标 | 集成机制 |
| :--- | :--- | :--- | :--- |
| **Tier 1（自动挂载）** | 终端模拟器 | **Ghostty** · **Windows Terminal** · **Kitty** · **WezTerm** · **iTerm2** · **Alacritty** · **Apple Terminal**（按需） | 外部 `include` / JSONC 保留注释补丁 / `pcall(dofile)` / Parent Profile 继承（Apple Terminal 仅在当前会话或 `--only` 时同步） |
| **Tier 1（自动挂载）** | Shell | **Zsh** · **Bash** · **Fish** · **PowerShell 5.1 / 7** | 单块标记锚点 + 零 fork `state.gen` 热同步钩子 |
| **Tier 1（自动生成）** | Prompt & 信息卡 | **Oh My Posh** · **Starship**（配色片段） · **Fastfetch** | 零命令段声明式配置，检测到用户自建 Prompt 时显式保护 |
| **Tier 2（编译产物 + `--only` Opt-In）** | 编辑器 / 复用器 / 工具 | **Neovim** · **Vim** · **Helix** · **tmux** · **Zellij** · **OpenCode** · **bat**（原生 `.tmTheme`） · **git-delta** · **lazygit** · **btop**（外加 **fzf** / **eza** 环境变量） | 每次切主题自动输出到 `generated/`；传 `--only <id>` 或配置 `enabledAdapters` 时才写入宿主 `themes/`/`colors/` 或 `tmux.conf` |

---

## 官方主题一览

| 主题 ID | 名称 | 风格 | Prompt 预设 |
| :--- | :--- | :--- | :--- |
| `nerv-eva` | **NERV UNIT-02** | 特務機関 NERV 弐号機 · 战术蓝图暗红橙 · MAGI 扫描线 + AT-Field 六边形 | `tactical-bar` |
| `kyoto-sakura` | **Kyoto Sakura** | 夜樱绘卷 · 和风月夜神社 · 垂枝樱瓣 + 萤火虫 + 灯笼摇曳 + 镜池微澜 | `shrine-ribbon` |
| `shinkai-rain` | **Shinkai Rain** | 言叶之雨 · 新海诚东京雨夜 · 视差雨丝 + 积水涟漪 + 暖金散景 | `cinematic-line` |
| `starlight-8bit` | **Starlight 8-Bit** | 星屑 8-Bit · 复古像素夜空 · 像素星海闪烁 + 定时流星 | `pixel-frame` |

![NERV UNIT-02 Preview](./previews/nerv-eva.png)
![Kyoto Sakura Preview](./previews/kyoto-sakura.png)
![Shinkai Rain Preview](./previews/shinkai-rain.png)
![Starlight 8-Bit Preview](./previews/starlight-8bit.png)

---

## 安装与快速开始

### 通过包管理器安装（正式版可用）

```bash
# Homebrew（macOS / Linux，v2.0.0 正式版发布后自动推送至 Tap）
brew install nerv-base/tap/kiseki

# Cargo Binstall（Rust）
cargo binstall --git https://github.com/nerv-base/kiseki-terminal kiseki-cli
```

### 独立一键安装脚本（含 SHA-256 校验，测试版 & 正式版均可用）

```bash
# macOS / Linux
curl -fsSL https://kisekiterm.dev/install.sh | sh
```

```powershell
# Windows PowerShell
irm https://kisekiterm.dev/install.ps1 | iex
```

### 从源码构建（需要 Rust 1.85+）

```bash
git clone -b v2 https://github.com/nerv-base/kiseki-terminal.git
cd kiseki-terminal
cargo build --release -p kiseki-cli

# 软链到 PATH（例如 ~/.local/bin）
mkdir -p ~/.local/bin
ln -sf "$PWD/target/release/kiseki" ~/.local/bin/kiseki

# 一键初始化向导（自动探测环境、兼容 v1 迁移并安全挂载）
kiseki init
```

### 主题依赖与字体

Homebrew 会安装 Oh My Posh、Fastfetch、Zsh 语法高亮和 Zsh 自动建议。独立安装脚本从同一版本发布的 `deps.tsv` 读取资产，校验 SHA-256 后将 Oh My Posh 和 Fastfetch 装到 Kiseki 的私有 `vendor/bin`。Shell 和 `kiseki fetch` 优先使用系统工具。Kiseki 在生成配置时写入两个内置 Zsh 插件，供系统插件缺失时使用。

`cargo binstall` 和源码构建不会自动安装外部依赖或字体。运行 `kiseki deps` 查看主题能力、版本和修复方式。Linux musl ARM64 当前没有上游 Fastfetch 二进制，已有的系统版本仍可使用。

独立安装脚本默认安装 JetBrains Mono Nerd Font、Departure Mono Nerd Font、Maple Mono NF CN 和 Fusion Pixel 12px，当前下载量合计约 212 MiB。JetBrains Mono 按文件下载，其余三族从 ZIP 选取主题所需字形。macOS 和 Linux 解压 ZIP 需要 `unzip`。已有字体时，可将上面的 `| sh` 改成 `| sh -s -- --skip-fonts`，或在 PowerShell 通过 `[scriptblock]::Create` 调用安装脚本并传入 `-SkipFonts`。

安装目录为 macOS 的 `~/Library/Fonts`、Linux 的 `~/.local/share/fonts/kiseki` 和 Windows 的 `%LOCALAPPDATA%\Microsoft\Windows\Fonts`。运行 `kiseki fonts status` 检查当前主题字体，或运行 `kiseki fonts install` 查看重新运行安装脚本的方法。

---

## 常用命令

| 命令 | 说明 |
| :--- | :--- |
| `kiseki init` | 交互式初始化向导（支持 `--non-interactive` / `--render-only`） |
| `kiseki pick` | 方向键交互式主题选择器（按 `Tab` 懒加载逛社区市场，回车一键安装热切） |
| `kiseki use <id>` | 直接切换指定主题（`nerv-eva` / `kyoto-sakura` / `shinkai-rain` / `starlight-8bit` 或社区主题；支持 `--only <adapters>` 挂载 Tier 2 工具） |
| `kiseki compose` | 按层混搭多个主题（`--palette` / `--syntax` / `--atmosphere` / `--typography` / `--overrides` / `--prompt` / `--reset`） |
| `kiseki profile <tier>` | 切换渲染档位：`static` / `eco` / `balanced` / `ultra` / `accessibility` |
| `kiseki live [on\|off]` | 快速开关动态 Shader（`balanced` ↔ `static`） |
| `kiseki bg <0..1\|off\|reset>` | 调整壁纸透明度、关闭或恢复主题默认 |
| `kiseki font` | 覆盖终端字体栈、字号或锁定字体不被主题覆盖（`--lock` / `--unlock`） |
| `kiseki status` | 查看当前主题、环境、适配器、产物与依赖摘要（支持 `--json`） |
| `kiseki deps` | 查看主题能力、工具来源、版本与修复方式（支持 `--json`） |
| `kiseki fonts status\|install` | 检查主题字体安装状态与重装指引 |
| `kiseki doctor` | 全链路诊断（路径权限、字体、依赖摘要、Shader 与宿主配置验证等） |
| `kiseki rollback [tx-id]` | 回滚到上一次或指定历史事务快照 |
| `kiseki recover --safe` | 安全逃生：强制静态模式、关闭 Shader、损坏自动修复 |
| `kiseki gallery search\|info\|refresh\|sources` | 搜索社区主题市场（支持 `--sort trending\|downloads\|stars\|updated\|newest`）、查看详情与索引源 |
| `kiseki theme init\|list\|info\|validate\|install\|update\|remove` | 脚手架生成、校验、从本地/归档/市场短 ID/`github:owner/repo` 安装及一键更新主题包 |
| `kiseki shell-init <zsh\|bash\|fish\|pwsh>` | 输出零 fork Shell 初始化同步脚本 |
| `kiseki completions <shell>` | 输出 Bash / Zsh / Fish / PowerShell / Elvish 自动补全脚本 |
| `kiseki fetch` | 使用当前主题配置展示 Fastfetch 系统信息卡 |
| `kiseki migrate [--apply]` | 探测并可逆迁移 Kiseki v1 旧版安装 |
| `kiseki uninstall [--purge]` | 干净撤销所有宿主挂载（加 `--purge` 清除本地数据） |

---

## 贡献与社区主题

- **外部贡献指南**：[CONTRIBUTING.md](./CONTRIBUTING.md)（开发环境、13-Crate 架构、如何新增终端/工具适配器与 PR 清单）
- **社区主题包开发指南**：[docs/COMMUNITY_THEMES.md](./docs/COMMUNITY_THEMES.md)（`kiseki theme init` 脚手架、5 层 Token、参数化 Shader 与安全/版权规范）
- **架构 RFC**：[docs/rfc/README.md](./docs/rfc/README.md) · [0001-architecture-and-workspace.md](./docs/rfc/0001-architecture-and-workspace.md) · [0002-theme-schema-and-tokens.md](./docs/rfc/0002-theme-schema-and-tokens.md) · [0003-adapter-contract-and-capabilities.md](./docs/rfc/0003-adapter-contract-and-capabilities.md) · [0004-shader-pipeline-and-security.md](./docs/rfc/0004-shader-pipeline-and-security.md) · [0005-dependency-and-asset-policy.md](./docs/rfc/0005-dependency-and-asset-policy.md) · [0006-community-theme-gallery.md](./docs/rfc/0006-community-theme-gallery.md)
- **JSON Schemas**：[schemas/manifest.v1.schema.json](./schemas/manifest.v1.schema.json) · [schemas/tokens.v1.schema.json](./schemas/tokens.v1.schema.json)

---

## License

MIT © Kiseki Terminal Contributors · [https://kisekiterm.dev](https://kisekiterm.dev)
