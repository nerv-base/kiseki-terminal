# Contributing to Kiseki Terminal v2

感谢你关注并愿意参与 **Kiseki Terminal** 的共建！无论你是提交 Bug 反馈、补充新的终端/工具适配器、打磨 Shader，还是创作自己的主题包，我们都非常欢迎。

---

## 核心工程红线（提交 PR 前请先确认）

在写任何代码之前，请务必遵守项目的架构底线（详见 [docs/rfc/README.md](./docs/rfc/README.md)）：

1. **绝不劫持宿主行为**：Kiseki 只接管**视觉层**（配色、字体、壁纸、Shader、Prompt 视觉、Fastfetch、命令行语法高亮色板）。绝不写入 `alias ls/cat/grep/cd`、键位绑定（`bindkey` / `keybind` / `Set-PSReadLineKeyHandler`）、`shell-integration`、`command` 或 `working-directory`。
2. **核心切换路径零网络、永不调用包管理器**：常规 `use / apply / status / doctor` 必须 100% 离线，绝不在运行时调用 `brew` / `apt` / `winget`；所有只读 HTTPS 社区市场请求严格隔离在独立的 `kiseki-gallery` crate 中（见 [RFC-0005](./docs/rfc/0005-dependency-and-asset-policy.md) 与 [RFC-0006](./docs/rfc/0006-community-theme-gallery.md)）。
3. **所有文件副作用必须走 `Platform` Trait 与 `kiseki-tx`**：Adapter 只能是纯规划器（返回声明式 `FileOperation`），绝不直接调用 `std::fs::write`，以确保 Symlink 保留、只读自动降级 `render-only`、`--dry-run` 零落盘与事务回滚对所有适配器统一生效。
4. **严格零脚本沙箱**：主题包禁止任何可执行脚本（`.sh`/`.ps1`/`.py`/`.lua`/`.js`）、符号链接、Prompt 可执行 `command` 段与未经 `naga` 校验的 Shader。

---

## 本地开发环境

### 依赖工具

- **Rust**：`1.85+`（含 `clippy`）
- **Node.js**：`22+`（用于校验发布脚本 `scripts/release/*.mjs`）
- **Bun**（可选，仅修改官网 `site/` 时需要）

### Workspace 结构（13 Crates）

| Crate | 职责 |
| :--- | :--- |
| `kiseki-types` | 领域原语：`Color`（sRGB/OKLCH/WCAG）、`ThemeId`、`RenderingProfile`、`CapabilityFlags`、`EnvContext` |
| `kiseki-schema` | `manifest.json` / `tokens.json` 强类型模型、L1→L2 派生、5 层混搭解析、WCAG 对比度校验 |
| `kiseki-shader` | 参数化 GLSL/HLSL 宏预处理、内置 `naga` 静态编译与 TDR 复杂度预算校验 |
| `kiseki-compiler` | 纯函数编译器：将 `ResolvedTheme` 确定性编译为各终端、Shell、Prompt、Fastfetch 与 Tier 2 产物 |
| `kiseki-platform` | `Platform` Trait、`SystemPlatform` / `MockPlatform`、路径解析、Symlink 安全读写、文件锁、字体与依赖探测 |
| `kiseki-adapters` | Tier 1（终端/Shell/Prompt/Fastfetch）与 Tier 2（opt-in 生态工具）声明式 `Adapter` 实现与注册表 |
| `kiseki-tx` | 6 阶段事务引擎：锁、校验、Journal 快照、按目标提交与回滚、`doctor` 诊断 |
| `kiseki-state` | `config.toml`、`state.json`、`state.gen` 与崩溃可恢复的预览状态 |
| `kiseki-pkg` | 官方主题内嵌加载、社区主题包安全沙箱校验、脚手架生成（`theme init`）与 v1 迁移 |
| `kiseki-gallery` | 独立同步社区市场客户端（`ureq + rustls`、ETag/TTL 缓存、防穿越 `.tar.gz` 沙箱解包、安装与更新） |
| `kiseki-tui` | Ratatui 交互式主题选择器（含 `Tab` 懒加载 `Gallery` 市场页）与初始化向导（`kiseki init`） |
| `kiseki-cli` | `kiseki` 主 CLI 入口、人机友好输出与 `--json` 协议 |
| `kiseki-bench` | 引擎微基准测试 |

### 常用校验命令

```bash
# 运行全部单元测试与集成测试
cargo test --workspace

# 静态检查（CI 强制 -D warnings）
cargo clippy --workspace --all-targets -- -D warnings

# 校验安装脚本与依赖清单脚本
sh -n scripts/install.sh
node --check scripts/release/render-deps.mjs
node --check scripts/release/render-manifests.mjs

# 构建并检查官网（如果改动了 site/）
cd site && bun install --frozen-lockfile && bun run build
```

---

## 如何新增一个终端或工具适配器（Adapter）

1. **在 `kiseki-compiler` 增加纯函数编译模块**：
   - 在 `crates/kiseki-compiler/src/artifact.rs` 注册产物相对路径常量。
   - 编写确定性渲染函数与静态 `lint()`（确保绝不输出非视觉键或危险命令），并在 `crates/kiseki-compiler/src/lib.rs` 中接入 `compile()`。
2. **在 `kiseki-adapters` 实现 `Adapter` Trait**：
   - 明确分层：`Tier1Managed`（桌面终端 / Shell / Prompt / Fastfetch）或 `Tier2OptInArtifact`（编辑器 / Multiplexer / CLI / 监控器，仅在显式 `--only` 或 `enabledAdapters` 时写入宿主 drop-in 目录，默认只输出 `generated/`）。
   - `detect()` 中避免慢速子进程调用，确保 `kiseki status` 保持毫秒级响应。
   - `plan()` 必须正确处理 `--render-only`、只读文件/只读符号链接降级与 SSH 会话抑制，同时实现对称的 `plan_unhook()`。
   - 在 `crates/kiseki-adapters/src/registry.rs` 的 `AdapterRegistry::standard()` 中注册。
3. **补齐自动化测试**：
   - 使用 `MockPlatform` 覆盖 `detect`、`plan`（普通文件 / 只读降级 / SSH）、`validate`、`reload` 与 `plan_unhook`。
   - 如有新 `FileOperation`，需同步处理 `crates/kiseki-tx/src/lib.rs` 的 `apply_host` 与 `resolve_target` 并验证 `rollback`。
4. **同步文档**：更新 [README.md](./README.md) 与 `site/src/data/docs.ts` 中的支持矩阵。

---

## 如何创作与分享主题包

请阅读专门的主题包开发指南：[docs/COMMUNITY_THEMES.md](./docs/COMMUNITY_THEMES.md)。

最快上手方式：

```bash
kiseki theme init my-theme --with-shader
kiseki theme validate ./my-theme
kiseki theme install ./my-theme
kiseki use my-theme
```

> 注：线上社区主题市场（Kiseki Gallery）将在 `v2.1+` 规划与落地，当前阶段欢迎先通过 GitHub 仓库或本地目录分享和安装主题包。

---

## PR 提交流程

1. 从 `v2` 分支切出特性分支（如 `feat/adapter-foot`、`fix/kitty-tint`）。
2. 提交信息建议遵循 Conventional Commits（`feat(...)` / `fix(...)` / `docs(...)` / `perf(...)`）。
3. 确保本地 `cargo test --workspace` 与 `cargo clippy --workspace --all-targets -- -D warnings` 全部通过。
4. 发起 PR 时勾选 [Pull Request 模板](./.github/PULL_REQUEST_TEMPLATE.md) 中的核对项。
