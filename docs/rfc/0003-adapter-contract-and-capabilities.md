# RFC-0003: Adapter Contract, Capability Model & Transaction Engine

- **Status**: Draft
- **Applies To**: `crates/kiseki-adapters`, `crates/kiseki-tx`, `crates/kiseki-platform`
- **Related**: [RFC-0001](./0001-architecture-and-workspace.md), [RFC-0002](./0002-theme-schema-and-tokens.md), [RFC-0004](./0004-shader-pipeline-and-security.md)

---

## 1. 统一抽象原则

操作系统、终端模拟器、Shell、Prompt 与终端应用是正交维度：
- **新增主题无需改动任何 Adapter 或 Engine 代码**。
- **新增终端/应用只需实现 `Adapter` Trait 并在 `AdapterRegistry` 注册**，所有能力协商、Dotfiles 保护、快照回滚与 Doctor 诊断由 Core 统一托管。
- **宿主写入最小化原则**：能把产物隔离在 `<kiseki_config>/generated/` 里通过单行 `include` / `source` 引用的，绝不在切换主题时重复改写用户主配置。

---

## 2. Capability Model（能力模型）

每个 Adapter 在运行时根据探测到的宿主机环境、版本与权限返回精确的 `CapabilitySet`，取代 v1 里硬编码 `if cur_term == "ghostty"` 的脆弱分支：

```rust
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CapabilityFlags: u32 {
        // 色彩与视觉
        const PALETTE_16           = 1 << 0;
        const TRUECOLOR            = 1 << 1;
        const WINDOW_OPACITY       = 1 << 2;
        const WINDOW_BLUR          = 1 << 3;
        const WALLPAPER_STATIC     = 1 << 4;
        const WALLPAPER_ALIGNMENT  = 1 << 5;
        const FONT_STACK_FALLBACK  = 1 << 6;
        const FONT_SINGLE_ONLY     = 1 << 7;
        const UI_CHROME            = 1 << 8; // Tab / Titlebar / Frame

        // Shader 渲染物理模型（必须区分，避免误假设等效）
        const SHADER_LAYERED_BG    = 1 << 9;  // Windows Terminal: t0(text) + t1(wallpaper) 分离
        const SHADER_POSTPROCESS   = 1 << 10; // Ghostty: 单张已合成 iChannel0 后处理

        // 热重载与作用域
        const HOT_RELOAD_SIGNAL    = 1 << 11; // SIGUSR2 (Ghostty>=1.3) / SIGUSR1 (Kitty)
        const HOT_RELOAD_WATCH     = 1 << 12; // 文件变更自动监听 (WT / WezTerm / iTerm2 DynamicProfiles)
        const SESSION_LIVE_OSC     = 1 << 13; // 支持 --session 会话内临时切色 (iTerm2/Kitty/OSC)

        // 配置集成模式
        const SUPPORTS_INCLUDE     = 1 << 14; // 支持外部 config-file / include / dofile
        const REQUIRES_JSONC_PATCH = 1 << 15; // 需在宿主 JSONC 文件做局部受控 Patch (Windows Terminal)
        const PARENT_INHERITANCE   = 1 << 16; // 支持父 Profile 继承 (iTerm2 Dynamic Profile Parent)
    }
}
```

### 环境上下文（`EnvContext`）
`kiseki-platform` 探测并向所有 Adapter 注入统一环境上下文，修复 v1 中 SSH/WSL/tmux 检测不完善的问题：

```rust
pub struct EnvContext {
    pub os: OsKind,                        // MacOs, Windows, Linux
    pub current_terminal: Option<String>,  // ghostty, windows-terminal, iterm2, kitty, wezterm, alacritty, warp...
    pub is_ssh: bool,                      // SSH_CONNECTION / SSH_TTY / SSH_CLIENT
    pub is_wsl: bool,                      // WSL_DISTRO_NAME / /proc/version Microsoft
    pub multiplexer: Option<Multiplexer>,  // Tmux, Zellij, Screen
    pub power_source: PowerSource,         // Ac, Battery, Unknown (用于功耗提示/策略)
    pub system_appearance: Appearance,     // Dark, Light (macOS AppleInterfaceStyle / Windows AppsUseLightTheme)
    pub render_only: bool,                 // 全局 --render-only 或用户配置
}
```

**SSH / WSL / Multiplexer 协商规则**：
- **SSH 会话中**：默认禁用本地桌面终端文件写入与进程重载，仅允许按需应用本机 Shell/TUI 环境变量或通过安全 OSC 透传（明确不修改远端宿主机配置文件）。
- **WSL 环境中**：自动识别运行在 Linux 子系统内还是 Windows 宿主侧；在 WSL 内运行 `kiseki` 时，默认同步 Linux 侧 Shell/CLI/编辑器产物，并可通过配置桥接检测 `/mnt/c/...` 下的 Windows Terminal 配置（需显式授权）。
- **tmux / Zellij 中**：若发送 OSC 序列，自动检测 `allow-passthrough` 能力决定是否包裹 DCS passthrough 转义，否则跳过并给出 Doctor 提示，绝不向屏幕打印乱码。

---

## 3. Rust Adapter Contract（Trait 契约）

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    Terminal,
    Shell,
    Prompt,
    Editor,
    Multiplexer,
    AgentTui,
    DevTool,
    SystemMonitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationTier {
    /// Tier 1: 一等自动热切（首次 init 挂载，之后零侵入同步）
    Tier1Managed,
    /// Tier 2: 编译独立产物/环境变量，显式 opt-in 挂载（绝不暴力改写用户复杂配置）
    Tier2OptInArtifact,
    /// Tier 3: 无稳定外部主题接口，通过终端 16 色 ANSI 透传 + Doctor 配置指引
    Tier3AnsiPassthrough,
}

pub struct DetectionReport {
    pub installed: bool,
    pub version: Option<semver::Version>,
    pub config_path: Option<PathBuf>,
    pub path_kind: ConfigPathKind,         // Regular, WritableSymlink(target), ReadOnlySymlink(target), Missing
    pub active_in_session: bool,
    pub notes: Vec<String>,
}

pub trait Adapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn kind(&self) -> AdapterKind;
    fn tier(&self) -> IntegrationTier;

    /// 1. Detect: 探测安装状态、版本、配置路径与软链接/只读属性
    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport>;

    /// 2. Capabilities: 基于探测结果返回当前版本实际支持的能力
    fn capabilities(&self, detection: &DetectionReport, env: &EnvContext) -> CapabilityFlags;

    /// 3. Plan: 纯规划阶段，输出将要写入的 generated 文件、宿主锚点/Patch 与降级说明（零副作用）
    fn plan(
        &self,
        theme: &ResolvedTheme,
        detection: &DetectionReport,
        caps: CapabilityFlags,
        opts: &PlanOptions,
    ) -> Result<AdapterPlan>;

    /// 4. Validate: 静态校验待落盘产物（如 `ghostty +validate-config`、JSONC 语法、Lua 语法、冲突检测）
    fn validate(&self, plan: &AdapterPlan, platform: &dyn Platform) -> Result<ValidationReport>;

    /// 5. Reload & Verify: 产物与宿主补丁由 Transaction Manager 统一落盘后，触发热重载并汇报结果
    fn reload(
        &self,
        detection: &DetectionReport,
        caps: CapabilityFlags,
        platform: &dyn Platform,
    ) -> Result<ReloadOutcome>;

    /// 6. Session Preview (可选): 仅当支持 SESSION_LIVE_OSC 时实现，零落盘临时预览
    fn preview_session(
        &self,
        _theme: &ResolvedTheme,
        _platform: &dyn Platform,
    ) -> Result<bool> {
        Ok(false)
    }

    /// 7. Unhook Plan: 生成干净撤销宿主挂载/补丁的计划（供 uninstall 与 v1 迁移复用）
    fn plan_unhook(&self, detection: &DetectionReport) -> Result<AdapterPlan>;
}
```

### 关键设计亮点：把「落盘与快照」从 Adapter 中抽离给 `kiseki-tx`
Adapter **不直接调用 `std::fs::write`**！Adapter 在 `plan()` 中只返回声明式 `FileOperation` 列表：
- `WriteGenerated { path, content }`（写入 `<kiseki_config>/generated/...`）
- `UpsertMarkerBlock { target, begin, end, body }`（幂等插入/更新注释包裹的 include 锚点）
- `PatchJsoncKeys { target, managed_patches, owned_scheme_names }`（保留注释的 JSONC 精准 Patch）
- `WriteDynamicProfile { target, parent_name, visual_overrides }`（iTerm2 继承式 Dynamic Profile）
- `RemoveMarkerBlock { target, begin, end }` / `RestoreJsoncKeys { target, snapshot }`

这样做的巨大好处是：**Symlink 安全解析、只读降级 `render-only`、`--dry-run` 展示 diff、快照备份、文件锁、回滚 Journal 全部由 `kiseki-tx` 一处实现，所有 25+ Adapter 自动获得同等安全性！**

---

## 4. 全生态应用三层集成矩阵

### Tier 1 — 一等自动热切 Adapter

| 类别 | 目标 | 集成方式 | 热重载方式 | 特别注意事项 |
| :--- | :--- | :--- | :--- | :--- |
| **Terminal** | **Ghostty** | 宿主配置 1 行 `config-file = ?.../generated/ghostty.conf` | `SIGUSR2`（Linux / macOS ≥1.3）或 AppleScript 降级 | 校验用 `ghostty +validate-config`；禁止写入非视觉项如 `shell-integration-features` |
| **Terminal** | **Windows Terminal** | **JSONC 保留注释局部 Patch**（见 §5.1） | WT 原生文件监听秒级自动重载 | 检测具体子 Profile 是否覆盖 `colorScheme`/`font` 并在 `doctor` 报警；不再写 `scrollbarState`/`bellStyle` |
| **Terminal** | **Kitty** | `kitty.conf` 1 行 `include .../generated/kitty.conf` | `SIGUSR1`（或 `kitty @ set-colors` 用于 `--session`） | 支持多字重映射与 `background_tint` 计算 |
| **Terminal** | **WezTerm** | `wezterm.lua` 注入安全 `pcall(dofile, ...)` 锚点 | Touch 配置文件触发原生热重载 | **修复 v1 静默失败**：若用户 Lua 结构无法安全定位 `return <ident>`，明确上报 `ActionRequired` 并打印 2 行手动接入代码，绝不谎报成功 |
| **Terminal** | **iTerm2** | **Parent Profile 继承式** Dynamic Profile（见 §5.2） | iTerm2 原生监听 + OSC 1337 `SetProfile` | **修复 v1 破坏性切换**：绝不硬写 `Silence Bell`、`Unlimited Scrollback`、`Option Key Sends` |
| **Terminal** | **Alacritty** | `alacritty.toml` `general.import = [...]` | Alacritty 原生 `live_config_reload` | 仅配色/字体/透明度/内边距（无壁纸/Shader 能力自动降级） |
| **Shell** | **Zsh / Bash / Fish / PowerShell** | 1 行 `source`/点源加载 `<kiseki_config>/generated/env.*` + 零 fork `precmd` hook | 任意已开窗口下一次回车 `< 0.2ms` 自动同步 | **严禁覆盖 `cat`/`ls`/`grep` 别名、zoxide 或键位** |
| **Prompt** | **Oh My Posh / Starship** | 编译 `generated/prompt.omp.json` 与 `generated/starship-palette.toml` | 随 Shell `state.gen` 变更自动刷新（OMP 旋转 `POSH_SESSION_ID`） | **ADR-006**：`kiseki init` 检测到用户自建配置时显式询问「仅注入配色 / 备份后使用官方模板 / 跳过」 |
| **Monitor** | **Fastfetch** | 编译 `generated/fastfetch.jsonc`（动态解析 logo 绝对路径） | 立即生效 | 不硬编码 `~/.config/kiseki-terminal`；仅通过 `kiseki fetch` 触发，不抢占全局 `fetch` 命令名 |

> 注：macOS 原生 **Terminal.app** 降为可选低优先级 Adapter（**绝不在默认 `auto` 检测中无条件同步**，避免每次切主题拖慢 Ghostty/iTerm2 用户；仅当用户显式启用或当前就在 `Apple_Terminal` 中运行时才同步）。

### Tier 2 — 编译独立产物 + 显式 Opt-in 挂载（绝不暴力改写用户配置）

| 类别 | 目标 | 生成产物 | 推荐接入方式 |
| :--- | :--- | :--- | :--- |
| **Editor** | **Neovim** | `generated/nvim/kiseki.lua`（标准 `vim.api.nvim_set_hl` + Treesitter/LSP 组） | 提供 1 行 `dofile(...)` 或 lazy.nvim local spec，不修改用户 `init.lua` |
| **Editor** | **Vim** | `generated/vim/kiseki.vim`（`hi` 高亮组 + `termguicolors`） | 用户可选 `source` 到 `~/.vimrc` |
| **Editor** | **Helix** | `generated/helix/kiseki.toml`（标准 Helix theme，软链或复制到 `themes/`） | 用户在 `config.toml` 选 `theme = "kiseki"`，发 `SIGUSR1` 热重载 |
| **Multiplexer** | **tmux** | `generated/tmux.conf`（状态栏、边框、消息与复制模式配色） | 用户可选 `source-file` 并由 Adapter 触发 `tmux source-file` 热刷 |
| **Multiplexer** | **Zellij** | `generated/zellij.kdl`（标准 `themes { kiseki { ... } }` 块） | 写入 Zellij `themes/` 目录，用户配置 `theme "kiseki"` |
| **Agent TUI** | **OpenCode** | `generated/opencode.json`（链接/同步至 `~/.config/opencode/themes/kiseki.json`） | 用户在 OpenCode 选择 `kiseki` 主题 |
| **Dev Tools** | **bat** | `generated/bat/Kiseki.tmTheme`（从 L1/L2 语法 Token **直接编译原生 TextMate XML**！） | 通过 `BAT_THEME="Kiseki"` + `bat cache --build`（告别 v1 借用第三方主题） |
| **Dev Tools** | **fzf / eza / delta / lazygit** | `env.*` 环境变量 + `generated/delta.gitconfig` + `generated/lazygit.yml` | `fzf`/`eza` 随 Shell 自动生效；`delta`/`lazygit` 通过 opt-in include 生效 |
| **Monitor** | **btop** | `generated/btop/kiseki.theme` | 放入 btop `themes/` 目录，opt-in 选中 |

### Tier 3 — 16 色 ANSI 透传与文档指引（不写脆弱的专有 Patch）

- **Claude Code**：推荐配置主题为 `dark-ansi` / `light-ansi`，100% 无缝跟随 Kiseki 终端 16 色 ANSI 调色板，不因上游配置结构频繁变动而失效。
- **Codex CLI / 其他 Agent TUI / htop / Nano / Warp**：默认复用终端 ANSI 调色板；`kiseki doctor` 可检测 Warp（提示无 CLI 热切限制，支持 `--export-warp-yaml` 导出静态 YAML 供用户手选）与 Nano 环境限制。

---

## 5. 关键宿主的非侵入精修方案

### 5.1 Windows Terminal：JSONC 保留注释局部 Patch 规范
彻底解决 v1 的 P0 侵入问题：
1. **词法/CST 保留注释编辑**：使用 Rust JSONC CST 解析库（如 `jsonc-parser` CST 模式或 `serde_json` + 精确字节区间替换器 `TextEdit`），**100% 保留用户手写的 `//`、`/* */` 注释、空白缩进、键顺序与尾逗号**。
2. **托管字段白名单**：仅修改/管理以下纯视觉键：
   - 顶层 `schemes[]` 中名称带 `"Kiseki: "` 前缀的条目、`themes[]` 中带 `"Kiseki: "` 前缀的条目、以及顶层 `theme`；
   - `profiles.defaults` 下的 `colorScheme`、`font`（仅 `face`/`size`/`cellHeight`/`weight`）、`cursorShape`、`opacity`、`useAcrylic`、`padding`、`unfocusedAppearance`、`backgroundImage*`、`experimental.pixelShaderPath`、`experimental.pixelShaderImagePath`。
   - **严禁触碰**：`scrollbarState`、`bellStyle`、`intenseTextStyle`、`startingDirectory`、`commandline`、`keybindings`、`actions` 及用户自定义 `profiles.list` 行为项。
3. **精准卸载与回滚**：在第一次接管 WT 时，将上述托管键的原始值（含“原本不存在”标记）存入 `<kiseki_data>/journal/wt-baseline.json`；执行 `kiseki uninstall` 时精准还原每个托管键的原始值并移除 `"Kiseki: "` 条目，不破坏卸载前用户期间新增的其他设置。
4. **子 Profile 冲突探测**：`kiseki doctor` 扫描 `profiles.list[*]`，如果用户的 PowerShell / WSL profile 单独硬编码了 `colorScheme` 或 `backgroundImage`，直接指出具体 Profile 名与行号，避免“切了主题却没生效”的黑盒体验。

### 5.2 iTerm2：基于 Parent Profile 的零行为损失继承
彻底解决 v1 丢掉用户快捷键/Trigger/工作目录的问题：
1. `kiseki init` 读取用户当前默认 Profile 的 `Name`（记为 `original_parent_name`）与 `Guid`。
2. 生成 `DynamicProfiles/kiseki-terminal.json` 时，在 `Kiseki Active` 与各主题 Profile 中写入：
   ```json
   {
     "Name": "Kiseki Active",
     "Guid": "kiseki-v2-active",
     "Dynamic Profile Parent Name": "<original_parent_name>",
     "Tags": ["Kiseki"],
     "Background Color": { ... },
     "Foreground Color": { ... },
     "Normal Font": "...",
     "Background Image Location": "..."
   }
   ```
3. **只覆写纯视觉键**，彻底删除 `Silence Bell`、`Unlimited Scrollback`、`Option Key Sends` 等非视觉字段——用户的 Trigger、工作目录、Hotkey、登录命令、回滚行数 100% 自动继承自原 Profile！

---

## 6. Dotfiles / Symlink 兼容与分目标事务引擎（`kiseki-tx`）

### 6.1 路径分类与写入规则（ADR-008）

| 目标路径状态 | 引擎行为 |
| :--- | :--- |
| **普通文件（可写）** | 原子写入（同目录临时文件 + `fsync` + `rename`），写入前保存完整快照至 Journal |
| **可写 Symlink（如 Stow / chezmoi / dotfiles repo）** | 调用 `canonicalize()` 解析最终目标路径，在目标文件所在目录执行原子写入，**绝对保留原路径的符号链接本身不被替换为普通文件** |
| **只读文件或只读 Symlink（如 `/nix/store/...`）** | **不报错中断整体流程**；自动对该 Adapter 降级为 `RenderOnly`（产物仍正常编译输出到 `<kiseki_config>/generated/`），在摘要中打印 1 行可直接复制到 Nix/Home Manager 配置里的引用路径 |
| **全局 `--render-only` 模式** | 所有 Adapter 仅执行 `WriteGenerated`，零宿主文件修改，零进程信号发送 |

### 6.2 分目标隔离事务与回滚流程

```text
Acquire File Lock (.lock)
  │
  ▼
[1. Detect & Capabilities]  ── 并发探测已启用 Adapter
  │
  ▼
[2. Plan & Validate]        ── 全量在内存中编译产物 + naga 校验 Shader + 宿主配置静态检查
  │                            （任一 Fatal 校验失败则零落盘直接中止，保护现有环境）
  ▼
[3. Snapshot & Journal]     ── 创建 journal/<tx-id>/manifest.json + 逐文件备份原内容与 SHA256
  │
  ▼
[4. Per-Target Apply]       ── 按目标独立提交：
  │                            • 先写入 generated/*（自有目录）
  │                            • 再执行需要 Patch/Hook 的宿主目标
  │                            • 单目标失败时立即自动回滚该目标，不影响其余健康目标
  ▼
[5. Commit State & Gen]     ── 原子更新 state.json 并递增 state.gen（触发多窗口 Shell 自动同步）
  │
  ▼
[6. Reload & Report]        ── 发送非阻塞热重载信号，输出结构化逐目标结果摘要
```

### 6.3 紧急恢复命令（`kiseki recover --safe`）
当异常 Shader、损坏配置或第三方包导致终端闪退/黑屏/卡顿时，用户可在任意救援终端或 TTY 执行 `kiseki recover --safe`：
1. 强制将 `state.json` 切为 `profile = "static"`、`crt = false`、移除自定义 Shader 路径；
2. 校验并清理所有残留的 `preview/recovery.json` 锁状态；
3. 对全部已托管终端立刻输出纯静态、高对比度安全配置并触发重载；
4. 可附加 `--rollback` 直接一键还原到上一个已知健康的 Journal 快照。
