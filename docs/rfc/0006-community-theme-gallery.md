# RFC-0006: Kiseki Gallery — Community Theme Registry & Distribution

> **Status**: Implemented (Target: `v2.0.0`)  
> **Depends on**: RFC-0001, RFC-0002, RFC-0004, RFC-0005  
> **Amends**: ADR-015 (Network Boundary) · **Adds**: ADR-016

---

## 1. 背景与目标

`v2.0` 已完备本地主题开发闭环（`kiseki theme init` / `validate` / `install`）以及严格零代码执行白名单沙箱（RFC-0004）。参考 [awesome-dsh-plugin](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin)（支撑数千社区条目的纯静态插件市场架构），本 RFC 定义随 `v2.0.0` 同步上线的 **Kiseki Gallery（社区主题市场）** 技术方案：

1. **极简一次上架、后续发版零 PR**：
   - 独立索引仓采用「一主题一独立 YAML 文件」（`data/themes/<id>.yml`），README 与 `index.json` 全自动生成，并发 PR 永不冲突；
   - 不做全网 Topic 盲扫、不设仓库年龄等待门槛，新主题建好即可提 PR；
   - **上架之后所有版本迭代完全由作者在个人仓库自行维护**（发 Tag/Release 或推分支），定时探测流水线全自动拉取校验并刷新市场，作者无需再提任何 PR。
2. **去人工特权、靠多维客观指标排行 + 自动能力披露**：
   - **不设人工 `verified` 认证字段**，所有社区主题平等收录；
   - 通过 **下载次数（总下载 / 30 天下载）+ GitHub Star（总 Star / 7 天飙升）+ 最近活跃度** 多维度排序，同时直接披露 `kiseki theme validate` 自动产出的能力元数据（WCAG 等级、壁纸、Shader 档位）。
3. **机器强安全、内容极宽松、生产级高可用**：
   - 双阶段安全 PR 门禁（`pr-check` + `workflow_run` 的 `pr-gate`，绝不执行 fork PR 代码）；
   - `push` 秒级上线与定时全量 `probe` 拆分为双并发组，互不取消；支持 `target_repo` 单条目定向刷新；
   - 坏版本自动拦截并保留上一稳定版在线；失效截图自动降级，绝不炸毁全站构建；周度 `decay-scan` 汇总失效仓库。
4. **真机截图加速展示 + 清晰版权边界**：
   - 带壁纸/Shader 主题自动把 `previews/preview.png` 压成轻量 WebP 缩略图推 R2 加速，国内不裂图；无截图纯色主题由前端按 `tokens` 摘要实时渲染终端卡片兜底；
   - 不二次托管第三方高分辨率壁纸原图，配标准 Report / DMCA 下架通道。
5. **CLI & TUI 一等体验（守住核心零网络底线）**：
   - 支持平铺短 ID（`kiseki theme install tokyo-midnight`）、直装任意 GitHub 仓库（`github:owner/repo`）、一键升级（`kiseki theme update`）、多索引源（`gallery.sources`）与 TUI 内置市场页；
   - 日常 `use / apply / status / doctor` 路径继续保持 100% 离线零网络。

---

## 2. 注册表仓库与全自动同步架构（借鉴 `awesome-dsh-plugin`）

独立设立轻量索引仓库 `kiseki-themes`，不存放任何主题源码或高分辨率壁纸二进制。

### 2.1 一主题一文件（`data/themes/<id>.yml`）

作者首次上架只需提交 **1 个独立 YAML 文件** `data/themes/<id>.yml`（文件名必须与平铺短 ID 一致；`README.md` 与 `README.zh.md` 由 CI 全自动生成，严禁手改，确保任意并发 PR 零合并冲突）：

```yaml
id: tokyo-midnight
repo: alice/kiseki-tokyo-midnight
subpath: .
track: releases
description:
  en: Deep indigo city-pop palette with subtle live shader glow.
  zh: 深靛蓝都市夜景配色，带柔和动态流光 Shader。
```

| 字段 | 必填 | 说明 |
| :--- | :--- | :--- |
| `id` | 是 | 全局唯一平铺短 ID（`^[a-z0-9][a-z0-9-]{2,47}$`），须与 `data/themes/<id>.yml` 文件名及目标仓库 `manifest.json#id` 一致 |
| `repo` | 是 | 公开 GitHub 仓库 `owner/name`；`owner` 同时作为该短 ID 的归属凭证 |
| `subpath` | 否 | 主题包在仓库内的相对子路径，默认 `.`（支持 monorepo 单仓多主题） |
| `track` | 否 | 版本追踪方式：`releases`（默认，优先取最新 SemVer Release/Tag `v*`，若尚无 Release/Tag 则回退默认分支最新 commit）或 `branch`（始终追踪默认分支） |
| `description` | 否 | 简短中/英介绍（缺省时自动读取 `manifest.json#subtitle`） |

### 2.2 平铺短 ID 保护规则

- **保留名单**：内置 4 套官方主题 ID（`nerv-eva`、`kyoto-sakura`、`shinkai-rain`、`starlight-8bit`）及 `kiseki-*`、`official-*`、`default`、`system`、`custom` 禁止注册。
- **不设仓库年龄门槛**：只要仓库公开可达且通过完整 `kiseki theme validate`（证明为真实可用的主题包而非空壳仓库），创建当天即可提交上架。
- **防批量占位与所有权保护**：单个 PR 默认限新增 1 条 YAML（monorepo 多主题可由维护者放行）；后续修改已有条目（如更换仓库路径）仅允许原 `repo` 的 `owner` 或维护者提交。

### 2.3 双阶段安全 PR 门禁（`pr-check` + `pr-gate`）

严格遵循 `awesome-dsh-plugin` 的 fork 安全隔离模型，避免 `pull_request_target` 供应链漏洞：

1. **`pr-check.yml`（`pull_request`，只读无敏感 Token）**：
   - 校验仅改动了 `data/themes/*.yml`（未手改自动生成的 README）；
   - 校验 YAML 语法、文件名与 `id` 一致性、保留名单、单 PR 新增数量上限。
2. **`pr-gate.yml`（`workflow_run`，在 `pr-check` 完成后触发）**：
   - **只 checkout 主分支的可信代码与锁文件，绝不执行 PR 分支的任何脚本**；
   - 仅将 PR 中变更的 `data/themes/*.yml` 作为纯数据提取出来；
   - 使用仓库配额的 `GITHUB_TOKEN` 查询目标仓库、解析最新不可变 `commit` SHA、拉取主题包并运行官方发布版 `kiseki theme validate --json`；
   - 若配置了壁纸或 Shader 但缺失 `previews/preview.png`，输出友好提示（Warning，不阻断）；
   - 全绿后即可合入，内容审核极宽松，不设人工审美卡点。

### 2.4 `push` 秒级上线与定时 `probe` 双并发组解耦

吸取 `awesome-dsh-plugin` 在大规模条目下的生产经验，将“合并后立刻发布”与“定时全量刷新”彻底解耦：

- **两个独立 `concurrency` group**（`build-push` 与 `build-probe`）：合并 PR 触发的 `push` 构建绝不会 cancel 掉正在运行中的定时 `probe`。
- **独立分片 Cache**：`data/probe-state.json`、`data/stars.json`、`data/downloads.json`、`data/updates.json` 各自独立缓存（互不共享同一 `path` 列表），防止新增缓存项导致全量冷启动击穿 GitHub API 配额。
- **三种运行模式**：
  1. **`push`（合入新 PR 后秒发）**：不重扫全网旧仓库，仅对本次新增/变更的条目执行增量探测，复用了缓存中其余条目的指标，数秒内生成并发布新版 `index.json` 与站点，新上架主题立即可见。
  2. **`schedule`（定时全量同步，例如每 2 小时）**：
     - 使用 GitHub GraphQL 批量查询所有已收录仓库的最新 Release/Tag/Commit、Star 数、推送时间与归档状态（单次请求查 100 个仓库）；
     - 仅对 `commit` SHA 发生变化的仓库拉取不可变归档并运行 `kiseki theme validate --json`；
     - **校验通过** → 更新 `sha256`、`manifest` / `tokens` 预览摘要、自动能力字段、`updates.json` 更新日志、将新 `previews/preview.png` 转码为轻量 WebP 缩略图推 R2、无壁纸主题同步归档至 R2，发布新版 minified `index.json`；
     - **校验失败** → **原地保留上一个通过校验的稳定版本在线不动**，在 `sync-status.json` 记录失败诊断供作者自助排查，作者推新版本修复后下一轮自动恢复。
  3. **`workflow_dispatch` 定向刷新（`target_repo`）**：允许传入单个 `id` 或 `owner/repo`，只刷新该条目的版本、截图与元数据，无需等待全量队列。
- **每日趋势快照（每日 00:00 UTC）**：写入轻量 `snapshots/YYYY-MM-DD.json`（仅记录各条目 `stars` 与 `downloads`），用于计算 7 天 Star 增量与 30 天下载量。
- **周度腐化巡检（`decay-scan.yml`）**：每周扫描一次 404/私有、已归档或已删除 `manifest.json` 的仓库，汇总写入**同一个 Tracking Issue** 供维护者确认后再下架，绝不静默误删。

---

## 3. 多维度热门排行、能力披露与匿名下载统计

**不设人工 `verified` 认证字段**。市场通过客观指标排序，并像 `awesome-dsh-plugin` 的 `probe-capabilities` 一样透明披露自动探测结果。

### 3.1 排序指标体系

| 维度 | 字段 | 数据来源 | 作用 |
| :--- | :--- | :--- | :--- |
| **下载热度** | `downloads` / `downloads30d` | 极薄 Cloudflare Worker 302 匿名下载计数（见 §3.2） | 反映真实安装量（“最多安装”） |
| **社区认可** | `stars` / `stars7d` | GitHub GraphQL + 每日快照差值 | 反映社区口碑与近期上升趋势（“Star 飙升”） |
| **维护活跃度** | `updatedAt` / `pushedAt` | 仓库最新发版 / 推送时间 | 识别近期更新与活跃维护的主题 |
| **自动能力披露** | `hasWallpaper`、`hasShader`、`shaderProfiles`、`wcagGrade`、`appearance` | `kiseki theme validate --json` 静态扫描 | 透明展示主题组成与无障碍评级，供正交筛选 |

默认综合热门分（`Trending`）采用多维对数平滑公式：

$$\text{trendingScore} = 0.50 \cdot \ln(1 + \text{downloads}_{30\text{d}}) + 0.30 \cdot \ln(1 + \text{stars}_{7\text{d}} \cdot 4) + 0.15 \cdot \ln(1 + \text{stars}) + 0.05 \cdot \text{recency}$$

官网与 CLI 同时支持切换排序：**`Trending`（默认）/ `Most Downloaded` / `Most Starred` / `Recently Updated` / `Newest`**。

### 3.2 零壁纸托管的匿名下载统计（Cloudflare Worker 302 路由）

采用极薄下载计数入口 `https://dl.kisekiterm.dev/gallery/dl/<id>/<commit>`：

- **无壁纸主题**（`provenance.wallpaperOrigin == "none"` 且无 `assets/wallpaper.*`，通常 `< 50 KiB`）：匿名计数 `+1`（Cloudflare Analytics Engine / KV，不记录 IP 或个人标识）→ 直接返回 R2 镜像归档，国内秒装；
- **带壁纸主题**：匿名计数 `+1` → 返回 **`302 Found`** 重定向至作者 GitHub 不可变 Archive URL（**不中转壁纸流量、不托管高分辨率原图壁纸**）；
- **高可用兜底**：`index.json` 同时给出 `download.trackUrl` 与 `download.url`（GitHub 原始不可变链接）；若计数路由不可达，CLI 自动降级直连 `download.url` 并核验同一 `sha256`（原生尊重 `HTTPS_PROXY` 及用户自定义 `gallery.githubProxy`）。

---

## 4. 预览截图、容错降级与版权下架边界

### 4.1 市场预览策略（真机截图优先 + 实时配色兜底）

- **有 `previews/preview.png`**：同步流水线自动压缩为轻量 WebP 缩略图（限宽 `960px`、画质 `80`、上限 `150 KiB`），推送至 R2 `gallery/thumbs/<id>-<short_sha>.webp`，确保官网和国内访问秒开不裂图。
- **无截图或截图探测失效**：不阻断构建，自动降级由官网前端依据 `index.json` 的 `tokens` 摘要**实时渲染终端窗口、代码语法高亮与 Prompt 预览卡**。

### 4.2 版权声明与快速下架（Notice-and-Takedown）

- 所有主题均强制携带 `LICENSE`、`CREDITS.md` 与 `manifest.provenance`（由 `kiseki theme validate` 门禁保证）；
- 官网详情页清晰展示原作者仓库、`LICENSE`、`CREDITS.md` 摘要，并提供标准 **Report / DMCA Takedown** 入口；
- 收到权利人通知或违规举报后，将对应 `id` 或 `owner/repo` 移出 `data/themes/` 并写入 `denylist.json`，下一次构建自动清除其索引条目与 R2 缓存缩略图。

---

## 5. 发布的静态索引拆分与多源分发

参考 `awesome-dsh-plugin` 将主索引与更新日志拆分的做法，避免每次刷新拉取冗余大文本：

1. **主索引 `gallery/index.json`**（minified 压缩 JSON，同步输出 `index.json.sha256`）：只包含列表展示、排序筛选、色板预览与安装校验所需字段；
2. **更新日志 `gallery/updates.json`**（独立文件，按需加载）：存放各主题最近 Release Notes 与简短 commit 摘要，仅在用户查看详情或更新说明时拉取；
3. **多源 CDN 分发**：主源发布至 Cloudflare R2（`https://dl.kisekiterm.dev/gallery/index.json`）与国内 EdgeOne 镜像（`https://kiseki-dl.nerv-base.com/gallery/index.json`），同时保留 GitHub Pages（`https://nerv-base.github.io/kiseki-themes/index.json`）作为异地兜底。

`gallery/index.json` 条目示例：

```json
{
  "schemaVersion": 1,
  "generatedAt": "2026-12-01T12:00:00Z",
  "themes": [
    {
      "id": "tokyo-midnight",
      "name": "Tokyo Midnight",
      "subtitle": "Deep indigo city-pop palette with subtle live glow",
      "version": "1.2.0",
      "appearance": "dark",
      "tags": ["dark", "community", "live-shader"],
      "authors": [{ "name": "Alice" }],
      "license": "MIT",
      "repo": "alice/kiseki-tokyo-midnight",
      "subpath": ".",
      "commit": "9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b",
      "createdAt": "2026-11-15T08:00:00Z",
      "updatedAt": "2026-12-01T11:30:00Z",
      "capabilities": {
        "hasWallpaper": true,
        "hasShader": true,
        "shaderProfiles": ["eco", "balanced", "ultra"],
        "wcagGrade": "AA"
      },
      "metrics": {
        "stars": 184,
        "stars7d": 26,
        "downloads": 1420,
        "downloads30d": 612,
        "trendingScore": 4.86
      },
      "download": {
        "trackUrl": "https://dl.kisekiterm.dev/gallery/dl/tokyo-midnight/9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b",
        "url": "https://codeload.github.com/alice/kiseki-tokyo-midnight/tar.gz/9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b",
        "sha256": "<hex>",
        "bytes": 1843200,
        "stripPrefix": "kiseki-tokyo-midnight-9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b"
      },
      "preview": {
        "thumbUrl": "https://dl.kisekiterm.dev/gallery/thumbs/tokyo-midnight-9a8b7c6d.webp",
        "palette": {
          "base": "#131722",
          "surface0": "#1D2333",
          "textPrimary": "#E9EEF8",
          "accentPrimary": "#74C7EC",
          "accentSecondary": "#F5A9B8",
          "keyword": "#CBA6F7",
          "string": "#A6E3A1",
          "function": "#89B4FA"
        }
      }
    }
  ]
}
```

---

## 6. CLI / TUI 架构与 ADR-015 边界修订

### 6.1 独立网络 Crate 隔离（`crates/kiseki-gallery`）

对 **ADR-015** 作出明确边界修订（记为 **ADR-016**）：

1. **核心路径永不触网**：`kiseki-compiler`、`kiseki-shader`、`kiseki-adapters`、`kiseki-tx`、`kiseki-state` 以及常规命令（`use` / `apply` / `profile` / `compose` / `status` / `doctor` / `recover` / `uninstall`）**继续 100% 离线零网络**，绝不引入 `tokio`。
2. **极简同步 HTTP 栈**：新增独立 `crates/kiseki-gallery`，采用纯同步 **`ureq` + `rustls`**（二进制增幅约 `~1–1.5 MiB`），原生支持 `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY`。
3. **显式触发原则**：仅在用户显式执行 `kiseki gallery ...`、远程 `kiseki theme install`、`kiseki theme update` 或在 TUI 主动切换至 `Gallery` Tab 时发起只读 HTTPS 请求。
4. **可插拔多索引源（`gallery.sources`）与本地缓存**：`config.json` 默认配置官方主源（含兜底镜像），并允许用户添加第三方/团队私有 `index.json` 源；索引缓存在 `<kiseki_cache>/gallery/`（带 `ETag` 与默认 `15` 分钟 TTL），断网时可读本地缓存。

### 6.2 CLI 命令面扩展

```bash
# 搜索与浏览市场（支持多维排序与 --json 输出）
kiseki gallery search [query] [--sort trending|downloads|stars|updated|newest] [--dark | --light] [--shader] [--tag <tag>] [--json]
kiseki gallery info <id> [--json]
kiseki gallery refresh

# 统一安装入口：支持 本地目录 / 本地 .kiseki.tar.gz / Gallery 短 ID / 任意 GitHub 仓库
kiseki theme install ./my-theme
kiseki theme install ./tokyo-midnight.kiseki.tar.gz
kiseki theme install tokyo-midnight
kiseki theme install github:alice/kiseki-tokyo-midnight[@ref]

# 自动检测并升级已安装的社区主题（依据本地 .kiseki-origin.json）
kiseki theme update [id] [--all] [--dry-run]
```

### 6.3 本地安装纵深防御（永不盲信远端）

无论来自官方 Gallery、自定义索引源还是直接 `github:owner/repo`，落地前均强制经过 5 道防线：

1. **流式体积截断**：HTTPS 下载流严格在 `30 MiB`（`kiseki_pkg::MAX_BYTES`）处截断；
2. **强制完整性核验**：从索引源安装时强制核验下载字节与 `download.sha256` 完全一致；
3. **安全沙箱解包**：拒绝绝对路径、`..` 路径穿越、符号链接/硬链接、设备节点，并限制解压总文件数 `<= 64`、总解压大小 `<= 30 MiB`（防压缩炸弹）；
4. **本地二次全量校验**：对解包目录**再次执行本机 `kiseki_pkg::validate_theme_dir`**（含 `naga` Shader 三档编译与 WCAG 对比度门禁）；
5. **原子落盘与来源回执**：校验通过后原子替换 `<kiseki_config>/themes/<id>`，写入 `.kiseki-origin.json`（记录 `source`、`id`、`repo`、`version`、`commit`、`sha256`、`installedAt`），供 `kiseki theme update` 幂等升级。

### 6.4 TUI 市场视图

在 `kiseki`（Ratatui 交互式选择器）中新增 `Gallery` Tab：
- 默认打开本地主题页（零网络瞬间启动）；
- 按 `Tab` 切到 `Gallery` 时由后台线程加载索引（不阻塞 UI 帧），支持实时搜索、按热门/下载/Star/最新切换排序、查看色卡与能力标签，按回车一键拉包安装并进入安全临时预览。

---

## 7. 官网 `site/` Gallery 页面

在现有 [site/](../../site) 中新增纯静态 Gallery 页面，直接消费 R2 CDN 的 `gallery/index.json`（按需加载 `updates.json`）：

- 顶部搜索框 + 排序切换（`Trending` / `Most Downloaded` / `Most Starred` / `Recently Updated` / `Newest`）+ 正交筛选（`Dark` / `Light` / `Live Shader` / `Wallpaper` / `Tag`）；
- 卡片优先展示 R2 WebP 终端真机截图（无截图或失效时按 `tokens` 摘要实时渲染终端卡片），角落展示 Star 数与下载量；
- 详情抽屉：完整色板与自动能力披露（WCAG 评级、Shader 档位）、最近更新记录、`LICENSE` & `CREDITS.md` 摘要、原 GitHub 仓库链接、一键复制 `kiseki theme install <id>` 与标准 Report/DMCA 下架入口。

---

## 8. 交付与仓库落点（随 `v2.0.0` 一起上线）

| 组件 | 落点 | 状态 |
| :--- | :--- | :--- |
| **Rust 市场客户端** | `crates/kiseki-gallery/`（纯同步 `ureq + rustls`、ETag/TTL 缓存、安全 `.tar.gz` 沙箱解包、`gallery-origins.json`） | 已实现（`v2.0.0`） |
| **CLI 与 TUI 集成** | `crates/kiseki-cli`（`kiseki gallery search/info/refresh/sources`、`kiseki theme install <id\|github:...>`, `kiseki theme update`）+ `crates/kiseki-tui`（`Tab` 懒加载 `Gallery` Tab） | 已实现（`v2.0.0`） |
| **官网 Gallery 视图** | `site/src/components/GalleryHub.tsx` + `#gallery` 路由（截图优先 + 实时配色兜底 + 详情抽屉与下架链接） | 已实现（`v2.0.0`） |
| **独立索引仓套件** | `infra/kiseki-themes/`（推送至独立仓库 `nerv-base/kiseki-themes` 启用，含 6 个 GitHub Workflows、自动化脚本与单测） | 已生成就绪（`v2.0.0`） |
| **边缘 302 计数 Worker** | `infra/gallery-worker/`（绑定 `dl.kisekiterm.dev/gallery/*` 与 R2 `kiseki-dl`） | 已生成就绪（`v2.0.0`） |

---

## 9. 新增与修订 ADR

| ADR | 维度 | 结论 |
| :--- | :--- | :--- |
| **ADR-016**（提案，修订 ADR-015） | 社区主题市场与受限网络边界 | 参考 `awesome-dsh-plugin` 架构：采用独立索引仓（一主题一 `data/themes/<id>.yml`、首次上架提一次 PR 且不设仓库创建时长门槛、不盲扫 Topic、双阶段 `pr-check`/`pr-gate` 安全隔离）+ 后续作者自维护版本由定时 `probe` 全自动同步零 PR（`push` 与 `probe` 双并发组解耦、分片缓存、`target_repo` 定向刷新、周度 `decay-scan` 巡检）；采用平铺短 ID；不设人工 `verified`，通过下载量（Cloudflare Worker 302 匿名计数，不二次托管高分辨率原图壁纸）/ Star / 7 日飙升 / 活跃度多维客观排行并透明披露自动扫描能力；终端截图压成 WebP 缩略图走 R2 加速，无截图/失效时降级实时 `tokens` 卡片，主索引 minified 与 `updates.json` 拆分，支持多索引源与标准下架通道。修订 ADR-015：核心切换路径继续 100% 离线零网络，仅独立 `kiseki-gallery`（同步 `ureq + rustls`）在显式市场命令 / 远程安装（含 `github:owner/repo`）/ TUI Gallery Tab 时发起只读 HTTPS 请求，落地前强制核验 `sha256` 并执行本机二次沙箱校验。 |
