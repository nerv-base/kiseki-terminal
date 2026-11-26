# RFC-0005: Dependency Provisioning & Asset Policy

- **Status**: Draft
- **Applies To**: `crates/kiseki-platform`, `crates/kiseki-cli`, `crates/kiseki-compiler`, `crates/kiseki-tui`, `scripts/install.sh`, `scripts/install.ps1`, `scripts/release`
- **Related**: [RFC-0001](./0001-architecture-and-workspace.md), [RFC-0003](./0003-adapter-contract-and-capabilities.md), [RFC-0004](./0004-shader-pipeline-and-security.md)
- **Issue**: [#6](https://github.com/nerv-base/kiseki-terminal/issues/6)

---

## 1. 问题

v2 的主题数据有相当一部分**只能经由外部工具才能被用户看见**，而这些工具从未被任何安装路径保证安装，且缺失时全部静默降级。

现状实测（`v2` 分支）：

| 事实 | 位置 |
| :--- | :--- |
| 两个独立安装器只下载 `kiseki` 二进制，零第三方工具、零字体 | `scripts/install.sh:1-8`、`scripts/install.ps1:1-12` |
| `kiseki init` 只生成配置与 hook，不安装任何外部工具 | `crates/kiseki-cli/src/lib.rs:1105` |
| OMP 缺失时 `env.zsh` 仅 `export STARSHIP_CONFIG`，而 Starship 从未被 init → 用户得到最朴素的 prompt | `crates/kiseki-compiler/src/shell.rs:140-168` |
| `zsh-syntax-highlighting` 只探测 4 个已装路径并静默跳过；**从未被任何脚本安装** | `crates/kiseki-compiler/src/shell.rs:180-190` |
| 4 个硬编码探测路径漏掉 Linuxbrew（`/home/linuxbrew/.linuxbrew/share/...`） | 同上 |
| `kiseki fonts install` 为 Milestone 2 stub，直接返回错误 | `crates/kiseki-cli/src/lib.rs:1848` |
| wizard 的 `install_fonts` 开关无任何消费者，文案却声称"缺失时提示安装" | `crates/kiseki-tui/src/wizard.rs:53,133,192,489` |

### 1.1 每个组件承载的主题数据

| 组件 | 消费的主题数据 | 缺失后果 |
| :--- | :--- | :--- |
| Oh My Posh | `overrides.prompt`（`preset` / `segmentColors` / `badgeText` / `okLabel` / `errorLabel` / `promptSymbol`）+ palette / syntax | 主题人格整体丢失，prompt 裸奔 |
| `zsh-syntax-highlighting` | `syntax` 令牌块 | **24 条 `ZSH_HIGHLIGHT_STYLES`** 全部空转，`syntax` 成为死数据 |
| `zsh-autosuggestions` | `syntax.InlinePrediction` | 1 条预测文本配色失效 |
| fastfetch | `overrides.fastfetch`（`logoPath` / `headerTitle` / `fieldLabels`） | 首屏画报丢失 |
| eza / bat / fzf / zoxide | palette | 退回默认配色（不破坏功能） |

v2 主题包本身**不携带** `prompt.omp.json` / `fastfetch.jsonc`——二者由 `kiseki use` 从 `tokens.json` 编译生成。因此这些工具属于**引擎级资产、全主题共用**，不是单个主题的附属物。这直接决定了第 6、7 节"内置还是外装"的边界。

---

## 2. 责任边界

**核心原则：`kiseki` 本体不执行任何包管理器，也不发起任何 HTTP 请求。**

依赖供给按渠道分流，每个渠道使用其原生机制：

| 渠道 | 供给机制 | 覆盖组件 |
| :--- | :--- | :--- |
| Homebrew | formula `depends_on` | OMP、fastfetch、两个 zsh 插件 |
| `install.sh` / `install.ps1` | 从本仓库 release 下载到 `$KISEKI_HOME/vendor/bin` | OMP、fastfetch + 4 族字体 |
| 编译进二进制 | `include_str!` | 两个 zsh 插件 |
| cargo binstall | **不做任何处理**，仅诊断提示 | — |
| 源码构建 | **不做任何处理**，仅诊断提示 | — |

后两者被视为有能力自行解决的用户。这不构成静默降级：第 4 节的诊断与能力清单会在 `init` / `doctor` / `status` / `deps` 四处明确报告缺失项，且内置的 zsh 插件保证这两个渠道**仍然拥有语法高亮与自动建议**。

### 2.1 为什么不由 `kiseki` 自己安装

`deps install` 在渠道分流后**没有剩余用户**：

- Homebrew / 安装器渠道已由各自机制覆盖
- binstall / 源码构建渠道明确不处理

唯一残留场景是"vendor 副本下载失败或被误删 → 重试"，而重试需要网络。`scripts/install.sh` 已经是幂等的且自带 SHA-256 校验，在 Rust 侧复刻一套下载逻辑属于重复实现，还会破坏"零网络"这一既有架构属性（当前 workspace 不含任何 HTTP 客户端依赖）。

---

## 3. 依赖清单的单一来源

清单分两层，避免"人写的身份"与"机器验证的版本"互相污染：

### 3.1 人写层：`crates/kiseki-platform/src/deps.rs`

静态 Rust 表，定义工具**身份**而非具体版本：

```rust
pub struct ToolSpec {
    /// 稳定 id，例如 "oh-my-posh"。
    pub id: &'static str,
    /// 探测方式。
    pub probe: ProbeKind,
    /// Homebrew 包名（None = 该渠道不覆盖）。
    pub brew_package: Option<&'static str>,
    /// 已验收的版本范围（semver）。
    pub version_range: VersionRange,
    /// 该工具失效时，用户看到的能力损失描述。
    pub capability: &'static str,
    /// 修复提示（安装器重跑命令 / brew 包名）。
    pub remedy: &'static str,
}

pub enum ProbeKind {
    /// PATH 上的可执行文件（OMP、fastfetch、eza、bat、fzf、zoxide）。
    OnPath,
    /// 非可执行资产：一组已知路径（两个 zsh 插件）。
    KnownPaths(&'static [&'static str]),
    /// 非可执行资产 + 由 kiseki 自己铺设的 vendor 兜底路径。
    KnownPathsOrVendored(&'static [&'static str]),
}
```

字体表并列存放（族 id → 上游 release 资产正则 → 落盘文件名 → 各平台目标目录），见第 7 节。

所有探测只通过 `Platform` trait（`which` / `exists` / `run` / `env_var`）完成，因此可由 `FakePlatform::set_command_output` 完整驱动，无需真实安装任何工具即可测试。

### 3.2 机器生成层：`dist/deps.tsv`

发布时由 `scripts/release/render-deps.mjs` 按 3.1 中钉住的版本号向上游取资产并校验，产出 TSV 清单供安装器消费：

```
kind    platform    url    sha256    dest    mode
tool    darwin-arm64    https://…/posh-darwin-arm64    <sha>    vendor/bin/oh-my-posh    0755
font    darwin-arm64    https://…/JetBrainsMono.zip    <sha>    fonts/jetbrains-mono-nf  0644
```

**为何是 TSV 而非 JSON**：`scripts/install.sh` 是 POSIX `sh`（`#!/usr/bin/env sh`），既无数组也无 JSON 解析器。`awk` + `IFS=$'\t'` 读取 TSV 是唯一不引入额外依赖的可靠方案。`install.ps1` 侧同样可用 `-split "`t"` 处理，无需 `ConvertFrom-Json`。

`kiseki deps --json` 的 JSON 输出由 Rust 直接序列化，不走这份文件，二者无漂移风险。

---

## 4. 诊断与呈现

| 入口 | 行为 |
| :--- | :--- |
| `kiseki deps` | 唯一权威明细。表格 + `--json`，每行 `工具 / 状态 / 来源(system｜vendored｜missing) / 版本 / 修复命令` |
| `kiseki doctor` | 仅一条聚合 check `deps.summary`（明细不进入诊断流，避免淹没既有检查项） |
| `kiseki status` | 一行聚合摘要 |
| `kiseki init` 摘要 | 追加能力清单，与 `kiseki deps` 共用同一渲染函数 |
| `scripts/install.sh` / `.ps1` 末尾 | 同一份能力清单（shell 侧实现，字段与 Rust 侧对齐） |

### 4.1 不提供 fallback prompt

曾考虑的"内置 minimal 风格兜底 prompt"被**否决**。理由：在渠道分流后，无 OMP 的人群只剩 binstall 与源码构建用户；为这部分用户维护第二套视觉（zsh + pwsh 两份）不划算，且每改一次预设都要同步兜底版本。改为"明确报告缺失 + 给出修复命令"。

### 4.2 删除死的 `install_fonts` 开关

`kiseki-tui` 的 `install_fonts` 从 wizard 一路传递到 `InitWizardChoices`，**没有任何消费者**，文案却声称"缺失时提示安装"。这与本 RFC"禁止静默降级"的原则直接冲突——同一产品内不能同时存在"如实报告"与"假装会做"两种行为。字体安装本身由安装器承担（第 7 节），`kiseki init` 内不再保留任何字体开关。

---

## 5. 版本兼容契约

### 5.1 OMP：以模板 API 版本判定，而非工具版本

`doctor` **不**直接比较 `oh-my-posh --version`。真正的兼容边界是我们生成的 `prompt.omp.json` 的 `"version": 3` 字段——OMP 的 minor 版本更新不改变该字段，而该字段变化才意味着渲染必然跑偏。

因此：

- `deps.tsv` 钉住一个已验证的具体版本（安装器装它）
- `doctor` 检查"已装 OMP 是否支持我们生成的模板 API 版本"，仅在**跨 major**时告警
- Homebrew 渠道无法指定 `depends_on` 的版本（机制上不可行），上述判定正好覆盖它，不会产生"brew 一升级就天天告警"的噪声

### 5.2 fastfetch：严格范围锁

fastfetch 的 jsonc 配置格式在 major 之间会真实破坏，因此 `deps.tsv` 钉死版本，`doctor` 对系统已装版本做范围检查并在越界时告警。

### 5.3 维护承诺

每次 OMP / fastfetch 发版需要滚动 `deps.tsv`。由 CI 定时任务承担：检测上游新版本 → 更新钉住的版本常量 → 跑一次"渲染 4 个 prompt 预设并快照比对"的冒烟测试 → 开 PR。

---

## 6. 内置资产策略（两个 zsh 插件）

| 组件 | 许可证 | 源码体积 |
| :--- | :--- | :--- |
| `zsh-syntax-highlighting` | BSD-3-Clause | 约 100KB |
| `zsh-autosuggestions` | MIT | 约 84KB |

约 190KB 纯 zsh 源码，用 `include_str!` 编入二进制，在生成阶段幂等铺盘到 `$KISEKI_HOME/vendor/share/zsh/`（内容 hash 不变则不重写）。

### 6.1 探测顺序：系统优先，vendor 兜底

```
现有 4 个系统路径 → 新增 Linuxbrew 路径 → vendor 副本
```

保留 `env.zsh` 现有的 `(( ! ${+functions[_zsh_highlight]} ))` 守卫：用户已在 `.zshrc` 加载过系统插件时直接跳过。理由：

- 与第 2 节的渠道哲学一致——有包管理器的渠道把版本交给包管理器
- vendor 优先会造成版本倒挂：用户 `brew upgrade` 更新了插件，kiseki 仍使用冻结的旧副本且用户无感
- 实现上是"在现有路径列表后追加一项"，比推翻守卫更小

热路径开销不变：只增加一次 `[[ -r ... ]]` 判断。

### 6.2 许可证合规

BSD-3-Clause 要求在**二进制分发**时复现版权声明。因此 vendor 目录必须落一份 `LICENSE.zsh-syntax-highlighting`、`LICENSE.zsh-autosuggestions` 与 `NOTICE`（记录 vendored commit sha），三份均编入二进制并随铺盘写入用户目录。

---

## 7. 字体策略

### 7.1 官方 4 族

4 个官方主题的 `typography.families` 并集正好是 4 个族，其中 `jetbrains-mono-nf` 被全部主题引用：

| 族 | 引用者 | 上游 zip 粗估 |
| :--- | :--- | :--- |
| `jetbrains-mono-nf` | 全部 4 个主题 | ≈30MB（仅取 3 个 ttf） |
| `maple-mono-nf-cn` | nerv-eva / kyoto-sakura / shinkai-rain | 含全 CJK，≈40–50MB |
| `departure-mono-nf` | starlight-8bit | ≈2MB |
| `fusion-pixel-12-zh` | starlight-8bit | ≈15MB |

安装器全量预装这 4 族（对齐 v1 `install.sh` 的行为）：下载总量 60–100MB，落盘约 20MB。

### 7.2 必须是"安装时下载"，不能进发行包

这 4 族均为 **OFL**。OFL 的义务是"再分发时附带许可证文本"：

- 将字体 vendor 进 release 归档 = 构成再分发 → 触发 OFL 义务
- 在用户机器上从上游 release 下载安装 = **不构成再分发** → 义务不触发

因此字体一律走安装器下载。这不是体积取舍，是许可证约束决定的形态。

### 7.3 平台落点

| 平台 | 目标目录 | 额外动作 |
| :--- | :--- | :--- |
| macOS | `~/Library/Fonts` | — |
| Linux | `~/.local/share/fonts/kiseki` | `fc-cache -f` |
| Windows | `%LOCALAPPDATA%\Microsoft\Windows\Fonts` | `AddFontResource` + `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts` 注册表项 |

### 7.4 `kiseki fonts install` 的去向

保留 `kiseki fonts status`（只读诊断）；`fonts install` 子命令改为打印"重跑安装器"的修复路径，与 `kiseki deps` 的修复入口统一，不再保留会抛错的 stub。

---

## 8. 安装器契约

两个安装器在依赖与字体安装阶段遵循同一组语义：

1. **单段失败不中断**：每个资产的下载 + 校验独立包裹（`sh` 侧在 `set -eu` 下显式 `if ! ...; then FAILED+=...; fi`），失败项记入清单
2. **退出码 0**：仅当 `kiseki` 本体安装失败时才非 0。依赖缺失属于可降级状态，主题与语法高亮仍然可用
3. **末尾能力清单**：逐项标注 `✓ 已就绪` / `✗ 缺失 → 修复命令`，字段与 `kiseki deps` 对齐
4. **幂等**：重复运行不产生重复文件、不破坏已有安装
5. **不进 `PATH`**：vendor 二进制落在 `$KISEKI_HOME/vendor/bin`，由 `env.zsh` / `env.ps1` 以绝对路径调用。避免与用户已有的 `oh-my-posh` / `fastfetch` 冲突，也让 `kiseki uninstall` 能只删除自己那份

---

## 9. 渠道收敛

### 9.1 移除 npm 与 Scoop

- **npm**：平台包只能携带 `kiseki` 单文件，无法表达"还需要 OMP / fastfetch"，是本表中唯一无解的渠道；且 README 的"零 postinstall"承诺也禁止在安装期补装
- **Scoop**：删除后 Windows 只剩 `install.ps1`，因此该脚本必须承担全部依赖与字体安装职责

移除后的渠道集合：Homebrew、`install.sh`、`install.ps1`、cargo binstall、源码构建。其中 Homebrew 是唯一仍存在的包管理器渠道，而它恰好是唯一支持原生依赖声明的。

### 9.2 Homebrew 使用 `depends_on`

`oh-my-posh`、`fastfetch`、`zsh-syntax-highlighting`、`zsh-autosuggestions` 均存在于 **homebrew-core**，故 formula 直接声明 `depends_on` 即可，无需自建 tap。这比由安装器下载更优：用户获得由包管理器托管的版本，可随 `brew upgrade` 更新。

同时必须删除 formula caveats 中现有的 `brew install oh-my-posh fastfetch zsh-syntax-highlighting` 提示——`depends_on` 已自动安装，保留即自相矛盾。

---

## 10. 测试与验收

| 范围 | 验收项 |
| :--- | :--- |
| 单元（`FakePlatform`） | 每个 `ProbeKind` 的探测分支；`system` / `vendored` / `missing` 三态；版本范围边界 |
| 一致性 | `deps.rs` 表 ↔ `dist/deps.tsv` 的 CI 校验测试（工具集合、平台覆盖、`dest` 路径规范） |
| `env.zsh` 生成 | 若系统路径不存在则引用 vendor 副本；若系统路径存在则不引用 vendor；Linuxbrew 路径出现在候选列表 |
| 渲染冒烟 | 4 个 prompt 预设生成结果的快照比对（CI 滚版本时必跑） |
| 渠道 | Homebrew（macOS arm/intel、Linux）、`install.sh`（macOS arm/intel、Linux glibc/musl）、`install.ps1`（Win x64/arm64）、binstall、源码构建，各自按第 8 节契约验收 |
| 断网重跑 | `install.sh` 断网运行：主题与语法高亮仍可用、退出码 0、能力清单标红并给出修复命令 |
| macOS 公证 | release 包在干净机器上首次运行 vendor 二进制无 Gatekeeper 拦截（`curl` 不写 `com.apple.quarantine`，但此结论须真机验证，不接受推断） |

---

## 11. 明确不做

- 自研 prompt 渲染 / 替代 Oh My Posh
- 自研系统信息卡 / 替代 fastfetch
- 依赖清单声明式化（theme manifest 增加 `requires` + `schemaVersion: 2`）
- cargo binstall / 源码构建的自动依赖安装
- 恢复 npm / Scoop 渠道
- 在 `kiseki` 本体内引入 HTTP 客户端

---

## 12. 已知代价

- 二进制增长约 190KB（两个 zsh 插件源码）
- "一键安装"需多下载 60–100MB（4 族字体）
- OMP / fastfetch 的版本由 CI 定时滚动，安全修复需跟随一次 kiseki 发版
- Homebrew 渠道无法锁定 OMP 版本，只能靠第 5.1 节的模板 API 版本判定兜底
- 删除 npm / Scoop 会缩小分发面

---

## 13. 新增 ADR

| ADR | 维度 | 结论 |
| :--- | :--- | :--- |
| **ADR-015**（提案） | 依赖供给与资产边界 | `kiseki` 本体永不执行包管理器、永不发起网络请求。依赖按渠道原生机制供给（Homebrew `depends_on` / 安装器下载到私有 vendor 目录 / 体积允许的纯文本资产编译进二进制）。字体因 OFL 再分发义务而一律在安装时从上游下载。版本确定性由 CI 滚动的 `deps.tsv` 与"模板 API 版本"契约共同保证。 |
