//! Human-readable CLI output, layout, colors, and copywriting.
//!
//! Zero Nerd Font dependency: all indicators, rules, swatches and mini-logos
//! use standard Unicode characters with graceful degradation under `NO_COLOR`,
//! `TERM=dumb`, `--quiet`, or non-TTY stdout.

use std::io::IsTerminal;

use kiseki_adapters::DetectionReport;
use kiseki_compiler::ArtifactMap;
use kiseki_pkg::{MigrationPlan, MigrationReport, ThemeEntry, ThemeSource, ThemeValidation};
use kiseki_platform::{FontStatus, KisekiPaths};
use kiseki_schema::{ContrastReport, ResolvedTheme};
use kiseki_state::{Config, State};
use kiseki_tui::{render_mini_cli_logo, ThemeSwatch};
use kiseki_tx::{DoctorCheckStatus, DoctorReport, RollbackReport, TargetStatus, TransactionReport};
use kiseki_types::{Diagnostic, DiagnosticSeverity, EnvContext, PromptPolicy, RenderingProfile};

/// CLI presentation renderer.
#[derive(Debug, Clone, Copy)]
pub struct Ui {
    color: bool,
    quiet: bool,
}

impl Ui {
    /// Detects color support and honors `NO_COLOR` / `TERM=dumb` / `--quiet`.
    pub fn new(quiet: bool) -> Self {
        let color = !quiet
            && std::io::stdout().is_terminal()
            && std::env::var_os("NO_COLOR").is_none()
            && std::env::var("TERM").map(|term| term != "dumb").unwrap_or(true);
        Self { color, quiet }
    }

    pub fn quiet(&self) -> bool {
        self.quiet
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn paint_rgb(&self, rgb: (u8, u8, u8), text: &str, bold: bool) -> String {
        if self.color {
            let prefix = if bold { "1;" } else { "" };
            format!("\x1b[{prefix}38;2;{};{};{}m{text}\x1b[0m", rgb.0, rgb.1, rgb.2)
        } else {
            text.to_string()
        }
    }

    fn brand(&self, text: &str) -> String {
        self.paint_rgb((255, 130, 184), text, true)
    }

    fn accent(&self, text: &str) -> String {
        self.paint_rgb((102, 240, 255), text, true)
    }

    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    fn green(&self, text: &str) -> String {
        self.paint("1;32", text)
    }

    fn yellow(&self, text: &str) -> String {
        self.paint("1;33", text)
    }

    fn red(&self, text: &str) -> String {
        self.paint("1;31", text)
    }

    fn cyan(&self, text: &str) -> String {
        self.paint("1;36", text)
    }

    fn swatch_strip(&self, swatch: &ThemeSwatch) -> String {
        if !self.color {
            return String::new();
        }
        format!(
            "{}{}{}{}{}",
            self.paint_rgb(swatch.accent, "██", false),
            self.paint_rgb(swatch.secondary, "██", false),
            self.paint_rgb(swatch.tertiary, "██", false),
            self.paint_rgb(swatch.star, "██", false),
            self.paint_rgb(swatch.dot_green, "██", false),
        )
    }

    /// Compact 8-bit window logo + brand header shown by `init` and `status`.
    pub fn banner(&self, swatch: Option<&ThemeSwatch>) {
        if self.quiet {
            return;
        }
        let s = swatch.copied().unwrap_or_default();
        let logo = render_mini_cli_logo(&s, self.color);
        let strip = self.swatch_strip(&s);
        let url_line = if strip.is_empty() {
            self.dim("https://kisekiterm.dev")
        } else {
            format!("{strip}  {}", self.dim("https://kisekiterm.dev"))
        };
        let right = [
            String::new(),
            format!(
                "{}  {}",
                self.paint_rgb(s.accent, "KISEKI TERMINAL", true),
                self.dim(concat!("v", env!("CARGO_PKG_VERSION")))
            ),
            self.dim("终端生态统一视觉引擎 · Universal Theming Engine"),
            url_line,
            String::new(),
        ];
        println!();
        for (l, r) in logo.iter().zip(right.iter()) {
            if r.is_empty() {
                println!("  {l}");
            } else {
                println!("  {l}   {r}");
            }
        }
        println!();
    }

    pub fn header(&self, title: &str) {
        if self.quiet {
            return;
        }
        println!();
        println!("{} {}", self.accent("▸"), self.cyan(title));
        println!("  {}", self.dim(&"─".repeat(48)));
    }

    pub fn success(&self, message: &str) {
        if !self.quiet {
            println!("{} {}", self.green("✓"), message);
        }
    }

    pub fn info(&self, message: &str) {
        if !self.quiet {
            println!("{} {}", self.cyan("·"), message);
        }
    }

    pub fn warn(&self, message: &str) {
        if !self.quiet {
            eprintln!("{} {}", self.yellow("!"), message);
        }
    }

    pub fn error(&self, message: &str) {
        eprintln!("{} {}", self.red("✗"), message);
    }

    pub fn kv(&self, key: &str, value: &str) {
        if self.quiet {
            return;
        }
        let padded = pad_display_width(key, 16);
        println!("  {} {}", self.dim(&padded), value);
    }

    pub fn bullet(&self, text: &str) {
        if self.quiet {
            return;
        }
        println!("  {} {}", self.dim("•"), text);
    }

    pub fn code_line(&self, text: &str) {
        if self.quiet {
            return;
        }
        println!("  {}", self.dim(text));
    }

    // ---------------------------------------------------------------- use
    pub fn use_report(&self, report: &TransactionReport, theme: &ResolvedTheme, dry_run: bool) {
        if self.quiet {
            return;
        }
        let swatch = swatch_from_resolved(theme);
        let verb = if dry_run { "预览变更（未写入）" } else { "已切换至" };
        println!();
        println!(
            "{} {} {} {}  {}",
            self.green("✓"),
            verb,
            self.paint_rgb(swatch.accent, &theme.name, true),
            self.dim(&format!(
                "({} · {} · gen {})",
                theme.id,
                profile_label(theme.profile),
                report.state.generation
            )),
            self.swatch_strip(&swatch)
        );
        self.print_targets(report, None);
        self.print_reloads(report);
        let warnings = report
            .diagnostics
            .items
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            .count();
        if warnings > 0 {
            self.warn(&format!("有 {warnings} 条提醒，运行 `kiseki doctor` 查看详情"));
        }
        if !dry_run {
            println!(
                "  {}",
                self.dim(&format!(
                    "回滚快照: {}/{}（随时可 `kiseki rollback`）",
                    shorten_home(&report.journal_dir),
                    report.tx_id
                ))
            );
        }
    }

    // ------------------------------------------------------------- status
    #[allow(clippy::too_many_arguments)]
    pub fn status(
        &self,
        state: Option<&State>,
        config: &Config,
        env: &EnvContext,
        theme: Option<&ResolvedTheme>,
        artifacts: Option<&ArtifactMap>,
        adapters: &[(String, DetectionReport)],
        paths: &KisekiPaths,
    ) {
        let swatch = theme.map(swatch_from_resolved);
        self.banner(swatch.as_ref());

        self.header("当前状态");
        match state {
            Some(state) => {
                if let Some(theme) = theme {
                    self.kv(
                        "主题",
                        &format!(
                            "{} {}",
                            self.brand(&theme.name),
                            self.dim(&format!("({}, {})", state.active_theme, appearance_label(theme.appearance)))
                        ),
                    );
                } else {
                    self.kv("主题", state.active_theme.as_str());
                }
                self.kv("渲染档位", profile_label(state.rendering.profile));
                self.kv(
                    "CRT 叠加",
                    if state.rendering.crt { "开启" } else { "关闭" },
                );
                self.kv(
                    "壁纸透明度",
                    &state
                        .rendering
                        .bg_opacity_override
                        .map(|value| format!("{value:.2} (手动覆盖)"))
                        .unwrap_or_else(|| "跟随主题默认".to_string()),
                );
                self.kv("同步版本", &format!("gen {}", state.generation));
                if state.has_composition() {
                    self.kv(
                        "按层混搭",
                        &format!(
                            "palette={} · atmosphere={} · typography={}",
                            state.composition.palette.as_ref().map(|id| id.as_str()).unwrap_or("base"),
                            state.composition.atmosphere.as_ref().map(|id| id.as_str()).unwrap_or("base"),
                            state.composition.typography.as_ref().map(|id| id.as_str()).unwrap_or("base"),
                        ),
                    );
                }
            }
            None => {
                self.kv("主题", "尚未初始化 — 运行 `kiseki init` 一键开始");
            }
        }

        self.header("运行环境");
        self.kv("系统 / 终端", &format!(
            "{} · {}",
            env.os.as_str(),
            env.current_terminal.as_deref().unwrap_or("未识别")
        ));
        let mut flags = Vec::new();
        if env.is_ssh {
            flags.push("SSH");
        }
        if env.is_wsl {
            flags.push("WSL");
        }
        if let Some(mux) = env.multiplexer {
            flags.push(mux.as_str());
        }
        if !flags.is_empty() {
            self.kv("会话上下文", &flags.join(" · "));
        }
        self.kv("系统外观", appearance_label(env.system_appearance));
        self.kv(
            "写入模式",
            if config.render_only || env.render_only {
                "render-only（仅输出 generated/）"
            } else {
                "托管模式（外部 include + 安全软链）"
            },
        );
        self.kv("Prompt 策略", prompt_policy_label(config.prompt_policy));

        self.header("终端与适配器");
        if adapters.is_empty() {
            self.bullet("未检测到已安装宿主");
        }
        for (id, detection) in adapters {
            let (mark, state_txt) = if detection.installed {
                (self.green("✓"), "已就绪")
            } else {
                (self.dim("·"), "未安装")
            };
            println!(
                "  {mark} {:<18} {:<10} {}",
                id,
                state_txt,
                self.dim(detection.path_kind.label())
            );
            for note in &detection.notes {
                println!("      {}", self.dim(note));
            }
        }

        if let Some(artifacts) = artifacts {
            self.header("已编译产物");
            for path in artifacts.paths() {
                println!("  {} generated/{}", self.dim("·"), path);
            }
        }
        println!();
        println!(
            "  {}",
            self.dim(&format!("配置目录: {}", paths.config_dir().display()))
        );
    }

    // ------------------------------------------------------------- doctor
    pub fn doctor(&self, report: &DoctorReport) {
        self.header("环境诊断 · DOCTOR");
        for check in &report.checks {
            let (marker, label) = match check.status {
                DoctorCheckStatus::Ok => (self.green("✓"), "正常"),
                DoctorCheckStatus::Info => (self.cyan("·"), "信息"),
                DoctorCheckStatus::Warning => (self.yellow("!"), "提醒"),
                DoctorCheckStatus::Error => (self.red("✗"), "错误"),
            };
            println!("  {marker} {:<28} {}", check.name, self.dim(label));
            println!("      {}", check.message);
            if let Some(hint) = &check.hint {
                println!("      {} {}", self.dim("建议:"), self.dim(hint));
            }
        }
        println!();
        println!(
            "  汇总: {} 正常 · {} 信息 · {} 提醒 · {} 错误",
            report.count(DoctorCheckStatus::Ok),
            report.count(DoctorCheckStatus::Info),
            report.count(DoctorCheckStatus::Warning),
            report.count(DoctorCheckStatus::Error),
        );
    }

    // -------------------------------------------------------------- themes
    pub fn theme_list(&self, entries: &[ThemeEntry], active: Option<&str>) {
        self.header("已安装主题");
        if entries.is_empty() {
            self.bullet("暂无可用主题");
            return;
        }
        for entry in entries {
            let is_active = active == Some(entry.id.as_str());
            let marker = if is_active {
                self.green("●")
            } else {
                self.dim("○")
            };
            let source = match entry.source {
                ThemeSource::Bundled => "内置官方",
                ThemeSource::Official => "官方",
                ThemeSource::Community => "社区包",
                ThemeSource::UserDirectory => "本地目录",
            };
            let padded_id = format!("{:<20}", entry.id.as_str());
            let id_col = if is_active {
                self.brand(&padded_id)
            } else {
                padded_id
            };
            println!(
                "  {marker} {id_col} {:<10} {}",
                entry.version,
                self.dim(source)
            );
            println!(
                "      {}  {}",
                entry.name,
                self.dim(entry.tagline.as_deref().unwrap_or(""))
            );
        }
        println!();
        println!(
            "  {}",
            self.dim("提示: 运行 `kiseki pick` 交互切换，或 `kiseki use <id>` 直接应用")
        );
    }

    pub fn theme_info(&self, theme: &ResolvedTheme, contrast: &ContrastReport, root: &std::path::Path) {
        let swatch = swatch_from_resolved(theme);
        self.header(&format!("{}  {}", theme.name, self.swatch_strip(&swatch)));
        self.kv("主题 ID", theme.id.as_str());
        self.kv("明暗外观", appearance_label(theme.appearance));
        self.kv("渲染档位", profile_label(theme.profile));
        self.kv("主题目录", &root.display().to_string());
        self.kv(
            "包含层级",
            &[
                "L1 palette",
                "L2 syntax/ui",
                "L3 atmosphere",
                "L4 typography",
                if theme.overrides.is_some() { "L5 overrides" } else { "" },
            ]
            .into_iter()
            .filter(|layer| !layer.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
        );
        self.kv("动态 Shader", if theme.shader_enabled { "已启用" } else { "关闭" });
        self.kv("壁纸资源", theme.wallpaper_path().unwrap_or("无"));

        self.header("WCAG 可读性对比度");
        for check in &contrast.checks {
            let marker = if check.passes_minimum() {
                self.green("✓")
            } else {
                self.red("✗")
            };
            println!(
                "  {marker} {:<28} {:>6.2}:1  {}",
                check.name,
                check.ratio,
                self.dim(&format!("最低 {:.1}:1", check.minimum))
            );
        }
        println!();
        if contrast.is_compliant() {
            self.success("WCAG 必检色彩组合全部达标");
        } else {
            self.error("存在未达标的对比度组合，发布前请先修复");
        }
    }

    pub fn validation(&self, path: &std::path::Path, validation: &ThemeValidation) {
        self.header("主题包安全与规范校验");
        self.kv("路径", &path.display().to_string());
        self.kv(
            "包体规模",
            &format!(
                "{} 个文件 · {:.1} KiB",
                validation.file_count,
                validation.total_bytes as f64 / 1024.0
            ),
        );
        if let Some(contrast) = &validation.contrast {
            self.kv(
                "WCAG 检查",
                if contrast.is_compliant() { "通过" } else { "未通过" },
            );
        }
        let mut diagnostics: Vec<&Diagnostic> = validation.diagnostics.iter().collect();
        diagnostics.sort_by_key(|diagnostic| diagnostic.severity);
        if diagnostics.is_empty() {
            self.success("未发现安全或规范问题");
        }
        for diagnostic in diagnostics {
            let marker = match diagnostic.severity {
                DiagnosticSeverity::Error => self.red("✗"),
                DiagnosticSeverity::Warning => self.yellow("!"),
                DiagnosticSeverity::Info => self.cyan("·"),
            };
            println!("  {marker} {:<36} {}", diagnostic.code, diagnostic.message);
            if let Some(hint) = &diagnostic.hint {
                println!("      {} {}", self.dim("建议:"), self.dim(hint));
            }
        }
        if !validation.shader_reports.is_empty() {
            self.header("Shader 静态校验（naga）");
            for summary in validation.shader_summaries() {
                println!("  {} {}", self.green("✓"), summary);
            }
        }
        println!();
        if validation.is_valid() {
            self.success("主题包校验通过，符合 Kiseki v2 零代码执行安全规范");
        } else {
            self.error("主题包存在阻断性错误，已拒绝通过");
        }
    }

    // --------------------------------------------------------------- fonts
    pub fn fonts(&self, statuses: &[FontStatus], theme_families: &[String]) {
        self.header("字体栈探测状态");
        self.kv("主题推荐栈", &theme_families.join(" → "));
        for status in statuses {
            let marker = if status.installed {
                self.green("✓")
            } else {
                self.yellow("!")
            };
            let origin = status
                .catalog_id
                .as_deref()
                .map(|id| format!("目录:{id}"))
                .unwrap_or_else(|| "字面族名".to_string());
            println!(
                "  {marker} {:<24} {:<26} {}",
                status.requested,
                status.resolved_family,
                self.dim(&origin)
            );
            if let Some(path) = &status.path {
                println!("      {}", self.dim(&path.display().to_string()));
            }
        }
        if statuses.iter().any(|status| !status.installed) {
            println!();
            self.warn("部分推荐字体尚未安装（将在 Milestone 2 支持 `kiseki fonts install` 校验下载）");
            println!(
                "  {}",
                self.dim("当前将自动回退到本机已安装的等宽与 CJK 字体，不影响正常使用")
            );
        }
    }

    // ------------------------------------------------------------ journals
    pub fn journals(&self, summaries: &[kiseki_tx::JournalSummary]) {
        self.header("事务快照日志");
        if summaries.is_empty() {
            self.bullet("暂无历史变更记录");
            return;
        }
        for summary in summaries {
            println!(
                "  {} {:<24} {:<10} {}",
                self.cyan("·"),
                summary.tx_id,
                summary.profile,
                self.dim(&summary.command)
            );
            println!(
                "      {}",
                self.dim(&format!(
                    "主题={} · 文件数={} · 路径={}",
                    summary.theme,
                    summary.entries,
                    summary.path.display()
                ))
            );
        }
        println!();
        println!(
            "  {}",
            self.dim("提示: 运行 `kiseki rollback <tx-id>` 恢复到任意快照")
        );
    }

    pub fn rollback(&self, report: &RollbackReport) {
        self.header("事务回滚完成");
        self.kv("回滚目标", &report.tx_id);
        self.kv("已恢复文件", &format!("{} 个", report.restored.len()));
        self.kv("当前主题", report.state.active_theme.as_str());
        for path in &report.restored {
            self.bullet(&path.display().to_string());
        }
        self.success("已精准还原至变更前状态");
    }

    // ----------------------------------------------------------- migration
    pub fn migration_plan(&self, plan: &MigrationPlan) {
        self.header("v1 → v2 自动迁移计划");
        for line in plan.summary() {
            self.bullet(&line);
        }
        println!();
        self.info("尚未修改任何文件；运行 `kiseki migrate --apply` 执行备份并迁移");
    }

    pub fn migration_report(&self, report: &MigrationReport) {
        self.header("v1 → v2 迁移完成");
        self.kv("完整备份", &report.backup_dir.display().to_string());
        self.kv("清理旧钩子", &format!("{} 处", report.removed_marker_blocks.len()));
        self.kv("清理旧 shim", &format!("{} 个", report.deleted_shims.len()));
        for path in &report.removed_marker_blocks {
            self.bullet(&path.display().to_string());
        }
        for path in &report.deleted_shims {
            self.bullet(&format!("已移除旧命令 shim: {}", path.display()));
        }
        for note in &report.notes {
            self.info(note);
        }
        if let Some(state) = &report.state {
            self.success(&format!(
                "已导入原有偏好：{} · {}",
                state.active_theme,
                profile_label(state.rendering.profile)
            ));
        }
    }

    // ----------------------------------------------------------- uninstall
    pub fn uninstall(&self, report: &TransactionReport, purged: bool, incomplete: bool) {
        self.header(if incomplete { "卸载未完全完成" } else { "卸载完成" });
        for target in &report.targets {
            let (marker, status_label) = target_badge(self, target.status);
            println!("  {marker} {:<32} {}", target.adapter, self.dim(status_label));
        }
        if incomplete {
            self.warn("部分宿主配置未能撤销，已保留 generated/、journal/ 与 wt-baseline.json 以便修复权限后重试 `kiseki uninstall`");
        } else {
            self.success(if purged {
                "已干净移除所有宿主挂载、生成产物、本地主题与日志"
            } else {
                "已干净撤销所有宿主挂载；个人配置与主题包已保留（加 `--purge` 可彻底清除）"
            });
        }
    }

    // ---------------------------------------------------------------- init
    #[allow(clippy::too_many_arguments)]
    pub fn init_summary(
        &self,
        config: &Config,
        state: &State,
        theme: Option<&ResolvedTheme>,
        report: Option<&TransactionReport>,
        paths: &KisekiPaths,
        on_path: bool,
        current_exe: Option<&std::path::Path>,
    ) {
        let swatch = theme.map(swatch_from_resolved);
        self.banner(swatch.as_ref());
        self.header("初始化完成");
        self.kv(
            "首发主题",
            &theme
                .map(|t| format!("{} ({})", t.name, state.active_theme))
                .unwrap_or_else(|| state.active_theme.as_str().to_string()),
        );
        self.kv("渲染档位", profile_label(state.rendering.profile));
        self.kv("Prompt 策略", prompt_policy_label(config.prompt_policy));
        self.kv(
            "写入模式",
            if config.render_only {
                "render-only（不修改宿主配置）"
            } else {
                "非侵入托管"
            },
        );
        self.kv(
            "短别名",
            if config.shell_aliases {
                "已开启 (theme / themes)"
            } else {
                "关闭（统一使用 kiseki）"
            },
        );
        if let Some(report) = report {
            self.kv("同步版本", &format!("gen {}", report.state.generation));
            self.print_targets(report, Some(&paths.generated_dir()));
            self.print_reloads(report);
        }
        println!();
        if !on_path {
            self.warn("当前 `kiseki` 尚未加入 PATH（所以直接输入 `kiseki` 会提示 command not found）");
            if let Some(exe) = current_exe {
                println!(
                    "    {}  {}",
                    self.dim("安装到 ~/.local/bin:"),
                    self.accent(&format!("ln -sf \"{}\" ~/.local/bin/kiseki", exe.display()))
                );
            }
            println!();
        }
        self.success("准备就绪！常用指令：");
        println!(
            "    {}  {}",
            self.accent("kiseki pick"),
            self.dim("交互式切换主题")
        );
        println!(
            "    {}  {}",
            self.accent("kiseki doctor"),
            self.dim("检查字体、Shader 与终端冲突")
        );
        println!(
            "    {}  {}",
            self.accent("kiseki rollback"),
            self.dim("一键回滚到上一次快照")
        );
        if !config.render_only {
            println!(
                "  {}",
                self.dim("提示：当前终端执行 `source ~/.zshrc` 或新开标签页后，Prompt 与环境变量立即生效")
            );
        }
    }

    pub fn print_reloads(&self, report: &TransactionReport) {
        for (adapter, detail) in &report.reload {
            // Suppress verbose steady-state auto-reload notes for passive adapters.
            let passive = detail.contains("on the next prompt")
                || detail.contains("on the next shell prompt")
                || detail.contains("reads the generated jsonc");
            if passive {
                continue;
            }
            let pretty = match detail.as_str() {
                "sent SIGUSR2 to ghostty" => "已发送 SIGUSR2 实时热重载",
                "no running ghostty process found" => "未检测到运行中的 Ghostty 进程（下次启动生效）",
                other => other,
            };
            println!("  {} {:<16} {}", self.cyan("↻"), adapter, self.dim(pretty));
        }
    }

    fn print_targets(&self, report: &TransactionReport, generated_dir: Option<&std::path::Path>) {
        let mut generated_ok = 0usize;
        let mut generated_failed = Vec::new();
        let mut host_targets = Vec::new();

        for target in &report.targets {
            let is_generated = target.adapter == "generated"
                || generated_dir.is_some_and(|dir| target.target.starts_with(dir))
                || target
                    .target
                    .components()
                    .any(|comp| comp.as_os_str() == "generated");
            if is_generated {
                if matches!(target.status, TargetStatus::Applied | TargetStatus::DryRun) {
                    generated_ok += 1;
                } else {
                    generated_failed.push(target);
                }
            } else {
                host_targets.push(target);
            }
        }

        if generated_ok > 0 {
            let label = generated_dir
                .map(shorten_home)
                .unwrap_or_else(|| "generated/".to_string());
            println!(
                "  {} {:<16} {}  {}",
                self.green("✓"),
                "generated",
                self.dim(&format!("{label} ({generated_ok} 项产物)")),
                self.dim(if report.dry_run { "预览 (dry-run)" } else { "已生成" })
            );
        }
        for target in generated_failed.into_iter().chain(host_targets) {
            let (marker, status_label) = target_badge(self, target.status);
            let path = shorten_home(&target.target);
            println!(
                "  {marker} {:<16} {}  {}",
                target.adapter,
                self.dim(&path),
                self.dim(status_label)
            );
            for note in &target.notes {
                if !note.is_empty() && target.status != TargetStatus::Applied {
                    println!("      {}", self.dim(note));
                }
            }
        }
    }

    pub fn recover(&self, state: &State) {
        self.header("安全逃生恢复");
        self.kv("当前主题", state.active_theme.as_str());
        self.kv("渲染档位", "Static（已强制关闭所有 Shader）");
        self.kv("CRT 叠加", "关闭");
        self.kv("壁纸透明度", "恢复默认");
        self.success("已切回纯静态安全模式，并清理残留预览状态");
    }

    // --------------------------------------------------------------- misc
    pub fn render_profile(&self, profile: RenderingProfile) {
        self.success(&format!("渲染档位已切换为 {}", profile_label(profile)));
    }

    pub fn compose(&self, state: &State) {
        self.header("按层混搭配置");
        self.kv("基础主题", state.active_theme.as_str());
        self.kv(
            "L1 配色",
            state.composition.palette.as_ref().map(|id| id.as_str()).unwrap_or("跟随基础主题"),
        );
        self.kv(
            "L2 语法",
            state.composition.syntax.as_ref().map(|id| id.as_str()).unwrap_or("自动推导"),
        );
        self.kv(
            "L3 氛围/Shader",
            state.composition.atmosphere.as_ref().map(|id| id.as_str()).unwrap_or("跟随基础主题"),
        );
        self.kv(
            "L4 字体",
            state.composition.typography.as_ref().map(|id| id.as_str()).unwrap_or("跟随基础主题"),
        );
        self.kv(
            "L5 覆盖项",
            state.composition.overrides.as_ref().map(|id| id.as_str()).unwrap_or("跟随基础主题"),
        );
        self.success("混搭组合已保存并应用");
    }

    pub fn print_plain(&self, text: &str) {
        if !self.quiet {
            println!("{text}");
        }
    }
}

/// Converts a resolved Kiseki theme into a compact TUI/CLI [`ThemeSwatch`].
pub fn swatch_from_resolved(theme: &ResolvedTheme) -> ThemeSwatch {
    let p = &theme.palette;
    ThemeSwatch::from_theme_colors(
        p.surfaces.base,
        p.surfaces.surface0,
        theme.ui.border,
        p.text.primary,
        p.text.muted,
        p.accents.primary,
        p.accents.secondary,
        p.accents.tertiary,
        p.ansi.bright_yellow,
        p.ansi.red,
        p.ansi.yellow,
        p.ansi.green,
    )
}

fn target_badge(ui: &Ui, status: TargetStatus) -> (String, &'static str) {
    match status {
        TargetStatus::Applied => (ui.green("✓"), "已应用"),
        TargetStatus::RenderOnly => (ui.yellow("~"), "仅渲染产物 (render-only)"),
        TargetStatus::Skipped => (ui.dim("·"), "跳过"),
        TargetStatus::DryRun => (ui.cyan("?"), "预览 (dry-run)"),
        TargetStatus::RolledBack => (ui.red("✗"), "已自动回滚"),
        TargetStatus::Failed => (ui.red("✗"), "失败"),
    }
}

/// Human label for a rendering profile.
pub fn profile_label(profile: RenderingProfile) -> &'static str {
    match profile {
        RenderingProfile::Static => "Static (静态)",
        RenderingProfile::Eco => "Eco (低功耗)",
        RenderingProfile::Balanced => "Balanced (推荐)",
        RenderingProfile::Ultra => "Ultra (全特效)",
        RenderingProfile::Accessibility => "Accessibility (高对比)",
    }
}

/// Human label for light/dark appearance.
pub fn appearance_label(appearance: kiseki_types::Appearance) -> &'static str {
    match appearance {
        kiseki_types::Appearance::Dark => "暗色 (dark)",
        kiseki_types::Appearance::Light => "浅色 (light)",
    }
}

/// Human label for a prompt policy.
pub fn prompt_policy_label(policy: PromptPolicy) -> &'static str {
    match policy {
        PromptPolicy::ManagedTemplate => "主题完整模板",
        PromptPolicy::PaletteOnly => "仅注入配色（保留原布局）",
        PromptPolicy::Disabled => "关闭",
    }
}

/// Human label for a diagnostic severity.
pub fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Info => "info",
    }
}

fn pad_display_width(text: &str, target: usize) -> String {
    let width: usize = text.chars().map(char_display_width).sum();
    if width >= target {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(target - width))
    }
}

fn shorten_home(path: &std::path::Path) -> String {
    let raw = path.display().to_string();
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() && raw.starts_with(&home) {
            return format!("~{}", &raw[home.len()..]);
        }
    }
    raw
}

fn char_display_width(ch: char) -> usize {
    let code = ch as u32;
    let wide = matches!(
        code,
        0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
    );
    if wide { 2 } else { 1 }
}
