# Kiseki Terminal v2 — Architecture & Engineering RFCs

> **Status**: Draft (Baseline Specification for `v2` branch)  
> **Scope**: Universal Immersive Theming Engine for Terminal Ecosystems (Rust Core)  
> **Baseline Version**: Branching from frozen `v1.1.0`

本目录包含 Kiseki Terminal v2 重构的正式技术规范（RFC）与机器可读 Schema，作为 `v2` 分支搭建 Rust Cargo Workspace 与实现垂直切片的唯一权威依据。

---

## RFC 索引

| 编号 | 文档 | 核心内容 |
| :--- | :--- | :--- |
| **RFC-0001** | [0001-architecture-and-workspace.md](./0001-architecture-and-workspace.md) | 总体架构、Rust Workspace Crate 划分、CLI 命令树、状态与作用域模型、`<1ms` 零 fork 多窗口同步、字体策略、`cargo-dist` 分发与 `v1 -> v2` 自动迁移 |
| **RFC-0002** | [0002-theme-schema-and-tokens.md](./0002-theme-schema-and-tokens.md) | 5 层可组合 Theme Schema（`manifest.json` + `tokens.json`）、Semantic Tokens 体系、L1→L2 确定性推导、WCAG/APCA 对比度校验、明暗配对、混搭合成与 `nerv-eva` 完整示例 |
| **RFC-0003** | [0003-adapter-contract-and-capabilities.md](./0003-adapter-contract-and-capabilities.md) | Adapter 契约与 Capability Model（Rust Trait 定义）、25+ 应用三层集成矩阵、Dotfiles/Symlink 与 `render-only` 兼容、Windows Terminal JSONC 保留注释局部 Patch、iTerm2 父 Profile 继承、分目标事务日志与回滚 |
| **RFC-0004** | [0004-shader-pipeline-and-security.md](./0004-shader-pipeline-and-security.md) | 参数化单模板 Shader 管线（1 GLSL + 1 HLSL + 编译期宏）、Rendering Profile、窄分屏长宽比自适应、Ghostty vs WT 渲染边界、内嵌 `naga` 静态校验与复杂度预算、零代码执行白名单沙箱、版权与字体授权规范 |
| **RFC-0005** | [0005-dependency-and-asset-policy.md](./0005-dependency-and-asset-policy.md) | 外部依赖供给边界（`kiseki` 本体零网络、零包管理器调用）、按渠道分流（Homebrew `depends_on` / 安装器下载到私有 vendor 目录 / 纯文本资产编译进二进制）、依赖清单单一来源与 `deps.tsv` 分发格式、OMP 真实二进制渲染验证、字体因 OFL 再分发义务而一律安装时下载、npm 与 Scoop 渠道移除 |

### 配套机器可读 Schema

- [schemas/manifest.v1.schema.json](../../schemas/manifest.v1.schema.json) — Theme Package Manifest（JSON Schema Draft 2020-12）
- [schemas/tokens.v1.schema.json](../../schemas/tokens.v1.schema.json) — 5 层 Semantic Tokens 与安全声明式覆盖（JSON Schema Draft 2020-12）

---

## 核心架构决策（ADR 001–014 已冻结 · ADR-015 提案中）

| ADR | 维度 | 已定案结论 |
| :--- | :--- | :--- |
| **ADR-001** | 产品定位 | 双核并重：第一版同时打磨「沉浸式壁纸/60fps Shader/字体栈引擎」与「20+ 终端生态应用的通用 Semantic Tokens 体系」 |
| **ADR-002** | 核心技术栈 | **Rust 单栈**：`clap` + `Ratatui` + 内嵌 `naga` + `windows-rs`/macOS 原生 API，保留未来 `wgpu` 预览路径 |
| **ADR-003** | 演进策略 | 冻结 `v1.1.0`（`main` 保持稳定），在 `v2` 分支直接启动 Rust 重构 |
| **ADR-004** | 应用适配分层 | Tier 1（终端/Shell/Prompt/Fastfetch）一等自动热切；Tier 2（编辑器/Multiplexer/CLI/OpenCode）编译产物 + 显式 opt-in 挂载；Tier 3（Claude Code/Codex/htop 等）16 色 ANSI 透传 |
| **ADR-005** | CLI 命名空间 | 统一主命令 `kiseki`，零全局无前缀 shim；旧 `theme`/`themes` 仅作为 `kiseki init --aliases` 可选别名 |
| **ADR-006** | Shell 与 Prompt | 彻底移除 `cat`/`ls`/`grep` 别名、`zoxide`、`PSReadLine` 键位改写与强卸 Starship；Prompt 检测到用户自建配置时显式询问是否允许覆盖（可选仅注入配色 / 备份后覆盖 / 跳过） |
| **ADR-007** | 字体策略 | 安装向导与首次切主题主动提示下载缺失的官方推荐字体；切主题默认应用主题字体栈（自动匹配本机字体 + CJK/Nerd Font 回退，支持用户手动覆盖风格/字号/锁定） |
| **ADR-008** | Dotfiles 与事务 | 尊重 Symlink 不破坏链接；外部 `include` 优先；遇 Nix/Home Manager 等只读配置自动降级 `render-only`（提供全局 `--render-only`）；无 include 宿主（WT）做保留注释 JSONC 局部 Patch + 分目标回滚日志 |
| **ADR-009** | Theme Schema | 5 层可组合模型（L1 必填，L2–L5 可选）+ 原生 `appearance: dark \| light` 与明暗配对 + 支持按层混搭覆盖 |
| **ADR-010** | Shader 渲染策略 | 每主题 1 份 GLSL + 1 份 HLSL 参数化模板 + 编译期宏生成 `Eco / Balanced / Ultra` + 内嵌 `naga` 静态校验；Ghostty `Balanced` 默认保留完整视觉特效（优化 `bgLayerMask` 保护文字，低干扰切 `Eco`/`Static`/`Accessibility`）；窄分屏靠 Shader 长宽比自适应淡出立绘 |
| **ADR-011** | 作用域与同步 | 全局主题为主 + Shell `precmd` 零 fork 读取 generation 自动同步多窗口；`kiseki pick` 临时预览零持久副作用且异常可恢复；支持终端提供可选 `--session` |
| **ADR-012** | 安全红线 | 严格零代码执行白名单沙箱：强类型与正则防配置注入；禁止第三方包携带原始 Lua/Shell/Vimscript；Prompt 严禁 OMP `command` 与 Starship `[custom.*]`；Shader 过 `naga` 复杂度预算；提供 `kiseki recover --safe` |
| **ADR-013** | 打包分发与更新 | 单仓 `cargo-dist` 双通道分发（Homebrew/Scoop/Winget + 一键脚本）；4 个官方主题随发行版内置离线可用 + `kiseki theme install/update`；独立安装走 SHA256 事务自更新（严禁回退 `main`） |
| **ADR-014** | 迁移与版权 | `kiseki init` 内置一键自动迁移（快照备份、干净卸载旧钩子与 8 个 shim、迁入偏好，补齐 Windows 完整卸载）；引擎 MIT + 主题包强制 `LICENSE`/`CREDITS.md` 来源元数据 + 严禁再分发第三方受限字体 |
| **ADR-015**（提案） | 依赖供给与资产边界 | `kiseki` 本体永不执行包管理器、永不发起网络请求；依赖按渠道原生机制供给（Homebrew `depends_on` / 安装器下载至私有 vendor 目录 / 体积允许的纯文本资产编译进二进制）；字体因 OFL 再分发义务一律安装时从上游下载；版本确定性由 CI 滚动的 `deps.tsv` 与 OMP 真实二进制渲染验证共同保证（见 [RFC-0005](./0005-dependency-and-asset-policy.md)） |

---

## 实施阶段总览（验证优先，拒绝瀑布流）

1. **Milestone 1 — 垂直切片（Vertical Slice）**：搭建 Rust Workspace，仅用最异构的组合（**Windows Terminal + Ghostty + PowerShell + Zsh + `nerv-eva`**）跑通 `Schema -> Compiler -> naga Shader Validation -> JSONC Patch / Include -> Journal & Rollback -> Zero-Fork Multi-Window Sync -> Doctor`。
2. **Milestone 2 — Tier 1 全量 + 官方四主题 + 交互 TUI**：补齐 Kitty、WezTerm、iTerm2（Parent Profile 继承）、Alacritty、Bash、Fish、Oh My Posh、Starship、Fastfetch；迁入全部 4 个官方主题并打磨参数化 Shader；落地 `Ratatui` 交互选择器、`kiseki init`（含 Prompt 询问、字体引导、v1 自动迁移）、SHA256 自更新与完整卸载。
3. **Milestone 3 — Tier 2 全生态应用 + Theme SDK**：实现 Neovim、Vim、Helix、tmux、Zellij、OpenCode、lazygit、fzf、bat（生成原生 `.tmTheme`）、eza、delta、btop 的编译模板与 opt-in 集成；落地 `kiseki theme init / validate / pack / preview`。
4. **Milestone 4 — 跨平台质量验收与 `v2.0.0` 发布**：CI 自动化矩阵（macOS/Windows/Linux）+ `naga` / WCAG 门禁 + `cargo-dist` 全渠道发布（Homebrew / Scoop / Winget）。
5. **Milestone 5（v2.1+）— Kiseki Gallery**：GitHub 驱动的静态 Registry、自动化 PR 校验（Schema + `naga` + 对比度 + 许可证审计）与静态展示站。
