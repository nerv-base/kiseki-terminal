//! `kiseki` command-line interface.
//!
//! All human-readable output lives in [`ui`]; every command also has a
//! `--json` form for scripts. Interactive selection and the init wizard are
//! delegated to `kiseki-tui`.

pub mod ui;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use kiseki_adapters::{AdapterRegistry, DetectionReport, PlanOptions};
use kiseki_compiler::{compile, CompileInput, CompileOptions};
use kiseki_pkg::{ThemeEntry, ThemeLibrary, ThemeSource};
use kiseki_platform::{
    capture_env, default_font_dirs, detect, font_status, fonts::FontScanner, KisekiPaths, Platform,
    SystemPlatform,
};
use kiseki_schema::{Composition, ResolvedTheme, ThemePack, UserOverrides};
use kiseki_state::{AppearanceMode, Config, PreviewSession, State};
use kiseki_tui::{InitWizardChoices, InitWizardInput, PickerAction, PickerItem};
use kiseki_tx::{
    doctor, JournalSummary, TargetStatus, TransactionManager, TransactionOptions, TransactionReport,
};
use kiseki_types::{
    Appearance, Diagnostic, DiagnosticSeverity, EnvContext, KisekiError, PromptPolicy, RenderingProfile, Result,
    ThemeId,
};
use serde_json::json;

use ui::Ui;

/// Top-level CLI.
#[derive(Debug, Parser)]
#[command(
    name = "kiseki",
    version,
    about = "Kiseki Terminal — 终端生态统一视觉与沉浸式主题引擎 (https://kisekiterm.dev)",
    after_help = "快速开始:\n  kiseki init              一键初始化向导（选主题 + 回车即用）\n  kiseki pick              交互式主题选择器\n  kiseki use nerv-eva      切换至指定主题\n  kiseki doctor            诊断终端、字体、Shader 与配置冲突\n  kiseki rollback          回滚到上一次快照\n\n官网与文档: https://kisekiterm.dev",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    pub json: bool,
    /// Suppress human-readable output.
    #[arg(long, short, global = true)]
    pub quiet: bool,
    /// Compile artefacts only; never modify host config files.
    #[arg(long, global = true)]
    pub render_only: bool,
    /// Show the change plan without writing anything.
    #[arg(long, global = true)]
    pub dry_run: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Every subcommand.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Switch the active theme.
    Use(UseArgs),
    /// Interactive theme picker.
    Pick,
    /// Show the current theme, environment and adapter status.
    Status,
    /// Mix layers from several themes.
    Compose(ComposeArgs),
    /// Show or set the rendering profile.
    Profile {
        /// static | eco | balanced | ultra | accessibility
        profile: Option<String>,
    },
    /// Toggle dynamic shaders (balanced <-> static).
    Live { action: Option<String> },
    /// Toggle the CRT post-processing overlay.
    Crt { action: Option<String> },
    /// Override wallpaper opacity, or `off` / `reset`.
    Bg { value: String },
    /// Override the font stack or point size.
    Font(FontArgs),
    /// Initialise Kiseki (detect, migrate v1, mount adapters).
    Init(InitArgs),
    /// Diagnose the environment, configuration and adapters.
    Doctor(DoctorArgs),
    /// Roll back a transaction.
    Rollback(RollbackArgs),
    /// Emergency recovery: force static, no shader, clear previews.
    Recover(RecoverArgs),
    /// Inspect, validate and install theme packs.
    Theme(ThemeArgs),
    /// Check font availability.
    Fonts(FontsArgs),
    /// Detect and import a v1 installation.
    Migrate(MigrateArgs),
    /// Remove Kiseki's host integrations.
    Uninstall(UninstallArgs),
    /// Print the zero-fork shell integration hook.
    ShellInit {
        /// zsh | pwsh
        shell: String,
    },
    /// Print shell completions.
    Completions {
        /// bash | zsh | fish | powershell | elvish
        shell: String,
    },
    /// Run fastfetch with the generated theme config.
    Fetch,
}

#[derive(Debug, clap::Args)]
pub struct UseArgs {
    /// Theme ID (for example `nerv-eva`).
    pub theme: String,
    /// Rendering profile override.
    #[arg(long)]
    pub profile: Option<String>,
    /// `auto`, `dark` or `light`.
    #[arg(long)]
    pub appearance: Option<String>,
    /// Try a session-only switch (unsupported hosts fall back to global).
    #[arg(long)]
    pub session: bool,
    /// Limit to comma-separated adapter IDs.
    #[arg(long)]
    pub only: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct ComposeArgs {
    /// Palette layer source theme.
    #[arg(long)]
    pub palette: Option<String>,
    /// Syntax layer source theme.
    #[arg(long)]
    pub syntax: Option<String>,
    /// Atmosphere layer source theme.
    #[arg(long)]
    pub atmosphere: Option<String>,
    /// Typography layer source theme.
    #[arg(long)]
    pub typography: Option<String>,
    /// Overrides layer source theme.
    #[arg(long)]
    pub overrides: Option<String>,
    /// Prompt integration: a theme ID, `palette-only` or `none`.
    #[arg(long)]
    pub prompt: Option<String>,
    /// Clear every layer override.
    #[arg(long)]
    pub reset: bool,
}

#[derive(Debug, clap::Args)]
pub struct FontArgs {
    /// Font Catalog ID, family name, or `reset`.
    pub style: String,
    /// Point size override.
    pub size: Option<f64>,
    /// Prevent later theme switches from changing the stack.
    #[arg(long)]
    pub lock: bool,
    /// Allow theme switches to change the stack again.
    #[arg(long)]
    pub unlock: bool,
}

#[derive(Debug, clap::Args)]
pub struct InitArgs {
    /// Enable opt-in `theme` / `themes` aliases.
    #[arg(long)]
    pub aliases: bool,
    /// CI/script mode: safe defaults, never overwrite a prompt config.
    #[arg(long)]
    pub non_interactive: bool,
}

#[derive(Debug, clap::Args)]
pub struct DoctorArgs {
    /// Remove stale locks and recover crashed preview sessions.
    #[arg(long)]
    pub fix: bool,
}

#[derive(Debug, clap::Args)]
pub struct RollbackArgs {
    /// Transaction ID; defaults to the latest.
    pub tx_id: Option<String>,
    /// List recorded transactions instead of rolling back.
    #[arg(long)]
    pub list: bool,
}

#[derive(Debug, clap::Args)]
pub struct RecoverArgs {
    /// Force static mode (required for the safe path).
    #[arg(long)]
    pub safe: bool,
    /// Roll back the latest transaction before recovering.
    #[arg(long)]
    pub rollback: bool,
}

#[derive(Debug, clap::Args)]
pub struct ThemeArgs {
    #[command(subcommand)]
    pub command: ThemeCommand,
}

#[derive(Debug, Subcommand)]
pub enum ThemeCommand {
    /// List built-in, official and community themes.
    List,
    /// Show theme metadata and the contrast report.
    Info { theme: String },
    /// Validate a theme pack directory (Schema + security + naga + WCAG).
    Validate { path: PathBuf },
    /// Install a local theme pack directory.
    Install {
        path: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Remove a community theme.
    Remove { theme: String },
}

#[derive(Debug, clap::Args)]
pub struct FontsArgs {
    #[command(subcommand)]
    pub command: FontsCommand,
}

#[derive(Debug, Subcommand)]
pub enum FontsCommand {
    /// Show font availability and catalog resolution.
    Status,
    /// Download/install fonts (Milestone 2).
    Install {
        #[arg(default_value = "theme")]
        target: String,
    },
}

#[derive(Debug, clap::Args)]
pub struct MigrateArgs {
    /// Execute the plan (a full read-only backup is always taken first).
    #[arg(long)]
    pub apply: bool,
}

#[derive(Debug, clap::Args)]
pub struct UninstallArgs {
    /// Also remove installed themes and the transaction journal.
    #[arg(long)]
    pub purge: bool,
}

/// Runs the CLI, returning the process exit code.
pub fn main_entry() -> i32 {
    let cli = Cli::parse();
    let json = cli.json;
    let ui = Ui::new(cli.quiet);
    let (result, json_emitted) = run_tracked(cli, ui);
    match result {
        Ok(()) => 0,
        Err(error) => {
            if json {
                // A command that already emitted its (single) JSON document
                // must not get a second, empty-looking error document.
                if !json_emitted {
                    emit_json(json!({ "ok": false, "error": error.to_string() }));
                }
            } else {
                ui.error(&error.to_string());
            }
            1
        }
    }
}

/// Executes the parsed CLI.
pub fn run(cli: Cli, ui: Ui) -> Result<()> {
    run_tracked(cli, ui).0
}

fn run_tracked(cli: Cli, ui: Ui) -> (Result<()>, bool) {
    let command = cli.command.unwrap_or_else(|| {
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
            Command::Pick
        } else {
            Command::Status
        }
    });
    // Rescue commands must start even when `state.json` is unparseable.
    let rescue = matches!(command, Command::Rollback(_) | Command::Recover(_));
    let mut app = match App::new(cli.json, cli.render_only, cli.dry_run, ui, rescue) {
        Ok(app) => app,
        Err(error) => return (Err(error), false),
    };
    let result = match command {
        Command::Use(args) => app.use_theme(args),
        Command::Pick => app.pick(),
        Command::Status => app.status(),
        Command::Compose(args) => app.compose(args),
        Command::Profile { profile } => app.profile_command(profile),
        Command::Live { action } => app.live(action),
        Command::Crt { action } => app.crt(action),
        Command::Bg { value } => app.bg(value),
        Command::Font(args) => app.font(args),
        Command::Init(args) => app.init(args),
        Command::Doctor(args) => app.doctor(args),
        Command::Rollback(args) => app.rollback(args),
        Command::Recover(args) => app.recover(args),
        Command::Theme(args) => app.theme(args),
        Command::Fonts(args) => app.fonts(args),
        Command::Migrate(args) => app.migrate(args),
        Command::Uninstall(args) => app.uninstall(args),
        Command::ShellInit { shell } => app.shell_init(&shell),
        Command::Completions { shell } => app.completions(&shell),
        Command::Fetch => app.fetch(),
    };
    (result, app.json_emitted)
}

struct App {
    ui: Ui,
    json: bool,
    json_emitted: bool,
    render_only: bool,
    dry_run: bool,
    platform: SystemPlatform,
    paths: KisekiPaths,
    config: Config,
    state: Option<State>,
    registry: AdapterRegistry,
    library: ThemeLibrary,
    env: EnvContext,
}

impl App {
    fn new(json: bool, render_only: bool, dry_run: bool, ui: Ui, rescue: bool) -> Result<Self> {
        let platform = SystemPlatform::detect();
        let paths = platform.paths().clone();
        let config = match Config::load(&platform, &paths) {
            Ok(config) => config,
            Err(error) => {
                ui.warn(&format!("{error}; using defaults"));
                Config::default()
            }
        };
        let mut env = detect(&platform, &capture_env());
        env.render_only = render_only || config.render_only;
        let state = match State::load(&platform, &paths) {
            Ok(state) => state,
            // `rollback` / `recover --safe` exist precisely for a corrupted
            // state: warn and continue with no state rather than refusing.
            Err(error) if rescue => {
                ui.warn(&format!("{error}; continuing without state (rescue mode)"));
                None
            }
            Err(error) => return Err(error),
        };
        let library = ThemeLibrary::open(&platform, &config.theme_dirs);
        Ok(Self {
            ui,
            json,
            json_emitted: false,
            render_only: render_only || config.render_only,
            dry_run,
            platform,
            paths,
            config,
            state,
            registry: AdapterRegistry::milestone1(),
            library,
            env,
        })
    }

    fn env(&self) -> &EnvContext {
        &self.env
    }

    /// Persists `config.toml`, unless `--dry-run` promised zero mutations.
    fn save_config(&self) -> Result<()> {
        if self.dry_run {
            return Ok(());
        }
        self.config.save(&self.platform, &self.paths)
    }

    /// Emits the command's single JSON document and records that stdout is no
    /// longer empty, so `main_entry` never appends a duplicate error document.
    fn emit_json(&mut self, value: serde_json::Value) {
        emit_json(value);
        self.json_emitted = true;
    }

    // ------------------------------------------------------------- helpers
    fn theme_id(&self, raw: &str) -> Result<ThemeId> {
        ThemeId::new(raw)
    }

    fn load_theme(&self, id: &ThemeId) -> Result<kiseki_pkg::LoadedTheme> {
        // `--dry-run` promises zero filesystem mutations, so it must not
        // materialise a bundled pack into the official store.
        if self.dry_run {
            self.library.load_readonly(&self.platform, id)
        } else {
            self.library.load(&self.platform, id)
        }
    }

    fn resolve_profile(&self, override_profile: Option<&str>) -> Result<RenderingProfile> {
        match override_profile {
            Some(raw) => raw.parse(),
            None => Ok(self
                .state
                .as_ref()
                .map(|state| state.rendering.profile)
                .unwrap_or_default()),
        }
    }

    fn state_or_initial(&self) -> State {
        self.state.clone().unwrap_or_else(|| {
            State::new(
                kiseki_pkg::default_theme_id(),
                &self.platform.now_rfc3339(),
            )
        })
    }

    fn composition(&self, state: &State) -> Composition {
        Composition {
            palette: state.composition.palette.clone(),
            syntax: state.composition.syntax.clone(),
            atmosphere: state.composition.atmosphere.clone(),
            typography: state.composition.typography.clone(),
            overrides: state.composition.overrides.clone(),
        }
    }

    fn composition_sources(&self, composition: &Composition) -> Result<Vec<ThemePack>> {
        let mut packs = Vec::new();
        for id in composition.referenced_themes() {
            packs.push(self.load_theme(id)?.pack);
        }
        Ok(packs)
    }

    fn user_overrides(&self, state: &State) -> UserOverrides {
        UserOverrides {
            bg_opacity: state.rendering.bg_opacity_override,
            crt: Some(state.rendering.crt),
            font_locked: state.typography.locked || self.config.font_locked,
            font_style: state.typography.style_override.clone(),
            font_size_pt: state.typography.size_override_pt,
        }
    }

    fn resolve(
        &self,
        loaded: &kiseki_pkg::LoadedTheme,
        state: &State,
        profile: RenderingProfile,
    ) -> Result<ResolvedTheme> {
        let composition = self.composition(state);
        let sources = self.composition_sources(&composition)?;
        ResolvedTheme::resolve(
            &loaded.pack,
            profile,
            &composition,
            &self.user_overrides(state),
            &sources,
        )
        .map_err(KisekiError::Compiler)
    }

    fn font_families(&self, theme: &ResolvedTheme) -> Vec<String> {
        let home = self
            .platform
            .env_var("HOME")
            .or_else(|| self.platform.env_var("USERPROFILE"))
            .map(PathBuf::from);
        let dirs = default_font_dirs(self.platform.os(), home.as_deref(), &|key| self.platform.env_var(key));
        let scanner = FontScanner::new();
        let inventory = scanner.scan(&dirs);
        let statuses = font_status(&theme.typography.families, &inventory, self.platform.os());
        kiseki_platform::config_families(&statuses)
    }

    fn compile_theme(
        &self,
        loaded: &kiseki_pkg::LoadedTheme,
        theme: &ResolvedTheme,
        state: &State,
    ) -> Result<kiseki_compiler::CompileOutput> {
        let families = self.font_families(theme);
        // `kiseki compose` may donate the atmosphere (wallpaper + shaders) and
        // the overrides (logo) from other themes. The resolved theme already
        // names the donor's assets, so the compile options must point at the
        // donor's roots as well.
        let composition = &state.composition;
        let atmosphere_dir = composition
            .atmosphere
            .as_ref()
            .and_then(|id| self.load_theme(id).ok())
            .map(|donor| donor.root);
        let overrides_dir = composition
            .overrides
            .as_ref()
            .and_then(|id| self.load_theme(id).ok())
            .map(|donor| donor.root);
        let (glsl, hlsl) = match composition.atmosphere.as_ref() {
            Some(id) => match self.load_theme(id) {
                Ok(donor) => (donor.glsl, donor.hlsl),
                Err(_) => (loaded.glsl.clone(), loaded.hlsl.clone()),
            },
            None => (loaded.glsl.clone(), loaded.hlsl.clone()),
        };
        let logo = match composition.overrides.as_ref() {
            Some(id) => match self.load_theme(id) {
                Ok(donor) => donor.logo,
                Err(_) => loaded.logo.clone(),
            },
            None => loaded.logo.clone(),
        };
        let options = CompileOptions::new(self.paths.config_dir(), loaded.root.clone())
            .with_prompt_policy(self.config.prompt_policy)
            .with_aliases(self.config.shell_aliases)
            .with_font_families(families)
            .with_atmosphere_dir(atmosphere_dir)
            .with_overrides_dir(overrides_dir);
        compile(
            &CompileInput::new(theme, options)
                .with_glsl(glsl.as_deref())
                .with_hlsl(hlsl.as_deref())
                .with_logo(logo.as_deref()),
        )
    }

    fn options(&self, command: &str, cli_only: Option<&str>, allow_create: bool) -> TransactionOptions {
        let (only, explicit_only) = self.effective_only(cli_only);
        TransactionOptions {
            command: command.to_string(),
            render_only: self.render_only,
            dry_run: self.dry_run,
            only,
            explicit_only,
            aliases: self.config.shell_aliases,
            prompt_policy: self.config.prompt_policy,
            allow_create_host_files: allow_create,
            require_all_targets: false,
            lock_timeout: kiseki_platform::LOCK_TIMEOUT,
        }
    }

    fn apply(
        &mut self,
        loaded: &kiseki_pkg::LoadedTheme,
        state_after: State,
        profile: RenderingProfile,
        command: &str,
        cli_only: Option<&str>,
        allow_create: bool,
    ) -> Result<TransactionReport> {
        self.apply_inner(loaded, state_after, profile, command, cli_only, allow_create, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_inner(
        &mut self,
        loaded: &kiseki_pkg::LoadedTheme,
        state_after: State,
        profile: RenderingProfile,
        command: &str,
        cli_only: Option<&str>,
        allow_create: bool,
        require_all_targets: bool,
    ) -> Result<TransactionReport> {
        let theme = self.resolve(loaded, &state_after, profile)?;
        let output = self.compile_theme(loaded, &theme, &state_after)?;
        let state_before = self.state_or_initial();
        let mut state_after = state_after;
        state_after.active_theme = loaded.pack.manifest.id.clone();
        state_after.rendering.profile = profile;
        let mut options = self.options(command, cli_only, allow_create);
        options.require_all_targets = require_all_targets;
        let manager = TransactionManager::new(&self.platform, AdapterRegistry::milestone1());
        let report = manager.apply(kiseki_tx::ApplyRequest {
            theme: &theme,
            artifacts: &output.artifacts,
            state_before,
            state_after,
            env: self.env(),
            options,
        })?;
        let blocked = require_all_targets
            && report.targets.iter().any(|target| match target.status {
                TargetStatus::Failed | TargetStatus::RolledBack => true,
                TargetStatus::RenderOnly => !report.render_only,
                TargetStatus::Applied | TargetStatus::Skipped | TargetStatus::DryRun => false,
            });
        if !self.dry_run && !blocked {
            self.state = Some(report.state.clone());
        }
        if !self.config.render_only && !self.render_only && !self.dry_run {
            for diagnostic in output
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            {
                self.ui.warn(&diagnostic.message);
            }
        }
        Ok(report)
    }

    /// Resolves the authoritative adapter allow-list.
    ///
    /// `config.enabledAdapters` and `--only` are intersected. Both are
    /// authoritative, so an empty intersection runs *zero* adapters instead of
    /// silently falling back to every detected host.
    fn effective_only(&self, cli_only: Option<&str>) -> (Vec<String>, bool) {
        let requested: Option<Vec<String>> = cli_only.map(|raw| {
            raw.split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect()
        });
        match (self.config.enabled_adapters.as_ref(), requested) {
            (Some(enabled), Some(requested)) => (
                enabled
                    .iter()
                    .filter(|id| requested.iter().any(|only| only == *id))
                    .cloned()
                    .collect(),
                true,
            ),
            (Some(enabled), None) => (enabled.clone(), true),
            (None, Some(requested)) => (requested, true),
            (None, None) => (Vec::new(), false),
        }
    }

    // -------------------------------------------------------------- status
    fn status(&mut self) -> Result<()> {
        let adapters = self.detect_all();
        let theme = self
            .state
            .as_ref()
            .and_then(|state| self.load_theme(&state.active_theme).ok())
            .and_then(|loaded| {
                let state = self.state.as_ref()?;
                self.resolve(&loaded, state, state.rendering.profile).ok()
            });
        let artifacts = self
            .state
            .as_ref()
            .and_then(|state| self.load_theme(&state.active_theme).ok())
            .and_then(|loaded| {
                let state = self.state.as_ref()?;
                let resolved = self.resolve(&loaded, state, state.rendering.profile).ok()?;
                self.compile_theme(&loaded, &resolved, state).ok()
            })
            .map(|output| output.artifacts);

        if self.json {
            self.emit_json(json!({
                "state": self.state.as_ref().map(state_json),
                "config": {
                    "promptPolicy": self.config.prompt_policy.as_str(),
                    "renderOnly": self.config.render_only,
                    "aliases": self.config.shell_aliases,
                    "fontLocked": self.config.font_locked,
                },
                "environment": env_json(self.env()),
                "theme": theme.as_ref().map(theme_json),
                "adapters": adapters.iter().map(|(id, detection)| json!({
                    "id": id,
                    "installed": detection.installed,
                    "path": detection.config_path,
                    "pathKind": detection.path_kind.label(),
                    "active": detection.active_in_session,
                    "notes": detection.notes,
                })).collect::<Vec<_>>(),
                "artifacts": artifacts.as_ref().map(|map| map.paths()),
                "paths": {
                    "config": self.paths.config_dir(),
                    "cache": self.paths.cache_dir(),
                    "data": self.paths.data_dir(),
                },
            }));
            return Ok(());
        }

        self.ui.status(
            self.state.as_ref(),
            &self.config,
            self.env(),
            theme.as_ref(),
            artifacts.as_ref(),
            &adapters,
            &self.paths,
        );
        Ok(())
    }

    // ----------------------------------------------------------------- use
    fn use_theme(&mut self, args: UseArgs) -> Result<()> {
        let id = self.theme_id(&args.theme)?;
        let mut loaded = self.load_theme(&id)?;
        let mut state = self.state_or_initial();
        state.active_theme = id.clone();
        if let Some(raw) = args.appearance.as_deref() {
            let (requested, persist) = if raw.eq_ignore_ascii_case("auto") {
                state.appearance_mode = AppearanceMode::Auto;
                (Some(self.env.system_appearance), None)
            } else {
                let appearance: Appearance = raw.parse()?;
                state.appearance_mode = AppearanceMode::Manual;
                (Some(appearance), Some(appearance))
            };
            if let Some(appearance) = requested {
                let paired = match appearance {
                    Appearance::Light => state.pairing.light.clone(),
                    Appearance::Dark => state.pairing.dark.clone(),
                };
                if let Some(paired) = paired {
                    if let Ok(paired_theme) = self.load_theme(&paired) {
                        state.active_theme = paired_theme.pack.manifest.id.clone();
                        loaded = paired_theme;
                    }
                }
            }
            if let (Some(appearance), false) = (persist, self.dry_run) {
                let appearance_file = self.paths.config_dir().join("appearance");
                self.platform
                    .write_preserving_symlinks(&appearance_file, format!("{appearance}\n").as_bytes())?;
            }
        }
        let profile = self.resolve_profile(args.profile.as_deref())?;
        if args.session {
            self.ui.warn(
                "--session is not supported by the enabled terminals in Milestone 1; applying globally",
            );
        }
        let command = format!("use {}", state.active_theme);
        let report = self.apply(
            &loaded,
            state,
            profile,
            &command,
            args.only.as_deref(),
            false,
        )?;
        if self.json {
            self.emit_json(transaction_json(&report));
        } else {
            let state = report.state.clone();
            let resolved = self
                .resolve(&loaded, &state, profile)
                .unwrap_or_else(|_| ResolvedTheme::from_pack(&loaded.pack, profile));
            self.ui.use_report(&report, &resolved, self.dry_run);
        }
        Ok(())
    }

    fn pick(&mut self) -> Result<()> {
        let entries = self.library.list(&self.platform)?;
        let active = self.state.as_ref().map(|state| state.active_theme.as_str().to_string());
        let items = self.build_picker_items(&entries);
        match kiseki_tui::run_picker(items, active.as_deref())? {
            PickerAction::Select(id) => {
                let args = UseArgs {
                    theme: id,
                    profile: None,
                    appearance: None,
                    session: false,
                    only: None,
                };
                self.use_theme(args)
            }
            _ => {
                if self.json {
                    self.emit_json(json!({ "cancelled": true }));
                } else {
                    self.ui.info("已取消，未修改当前主题");
                }
                Ok(())
            }
        }
    }

    fn compose(&mut self, args: ComposeArgs) -> Result<()> {
        let mut state = self.state_or_initial();
        if args.reset {
            state.composition.clear();
        }
        for (slot, raw) in [
            ("palette", args.palette.as_deref()),
            ("syntax", args.syntax.as_deref()),
            ("atmosphere", args.atmosphere.as_deref()),
            ("typography", args.typography.as_deref()),
            ("overrides", args.overrides.as_deref()),
        ] {
            let Some(raw) = raw else { continue };
            let id = self.theme_id(raw)?;
            self.load_theme(&id)?;
            match slot {
                "palette" => state.composition.palette = Some(id),
                "syntax" => state.composition.syntax = Some(id),
                "atmosphere" => state.composition.atmosphere = Some(id),
                "typography" => state.composition.typography = Some(id),
                "overrides" => state.composition.overrides = Some(id),
                _ => {}
            }
        }
        if let Some(prompt) = args.prompt.as_deref() {
            match prompt {
                "palette-only" | "palette" => {
                    self.config.prompt_policy = PromptPolicy::PaletteOnly;
                    self.save_config()?;
                }
                "none" | "disabled" | "off" => {
                    self.config.prompt_policy = PromptPolicy::Disabled;
                    self.save_config()?;
                }
                other => {
                    let id = self.theme_id(other)?;
                    self.load_theme(&id)?;
                    state.composition.overrides = Some(id);
                }
            }
        }

        let id = state.active_theme.clone();
        let loaded = self.load_theme(&id)?;
        let profile = state.rendering.profile;
        let report = self.apply(
            &loaded,
            state.clone(),
            profile,
            "compose",
            None,
            false,
        )?;
        if self.json {
            self.emit_json(json!({
                "composition": composition_json(&report.state),
                "transaction": transaction_json(&report),
            }));
        } else {
            self.ui.compose(&report.state);
        }
        Ok(())
    }

    fn profile_command(&mut self, profile: Option<String>) -> Result<()> {
        let Some(raw) = profile else {
            let current = self.resolve_profile(None)?;
            if self.json {
                self.emit_json(json!({ "profile": current.as_str() }));
            } else {
                self.ui.kv("profile", current.as_str());
            }
            return Ok(());
        };
        let profile: RenderingProfile = raw.parse()?;
        let mut state = self.state_or_initial();
        state.rendering.profile = profile;
        let id = state.active_theme.clone();
        let loaded = self.load_theme(&id)?;
        let report = self.apply(
            &loaded,
            state,
            profile,
            &format!("profile {profile}"),
            None,
            false,
        )?;
        self.after_simple(&report);
        Ok(())
    }

    fn live(&mut self, action: Option<String>) -> Result<()> {
        let mut state = self.state_or_initial();
        let current = state.rendering.profile;
        let target = match action.as_deref().unwrap_or("toggle") {
            "on" | "enable" => RenderingProfile::Balanced,
            "off" | "disable" => RenderingProfile::Static,
            "toggle" => {
                if current == RenderingProfile::Static {
                    RenderingProfile::Balanced
                } else {
                    RenderingProfile::Static
                }
            }
            other => other.parse()?,
        };
        state.rendering.profile = target;
        let id = state.active_theme.clone();
        let loaded = self.load_theme(&id)?;
        let report = self.apply(&loaded, state, target, "live", None, false)?;
        self.after_simple(&report);
        Ok(())
    }

    fn crt(&mut self, action: Option<String>) -> Result<()> {
        let mut state = self.state_or_initial();
        state.rendering.crt = match action.as_deref().unwrap_or("toggle") {
            "on" | "enable" => true,
            "off" | "disable" => false,
            "toggle" => !state.rendering.crt,
            other => return Err(KisekiError::other(format!("invalid crt action {other:?}; use on|off|toggle"))),
        };
        let id = state.active_theme.clone();
        let profile = state.rendering.profile;
        let loaded = self.load_theme(&id)?;
        let report = self.apply(&loaded, state, profile, "crt", None, false)?;
        self.after_simple(&report);
        Ok(())
    }

    fn bg(&mut self, value: String) -> Result<()> {
        let mut state = self.state_or_initial();
        state.rendering.bg_opacity_override = match value.trim() {
            "off" | "none" => Some(0.0),
            "reset" | "default" => None,
            raw => {
                let parsed: f64 = raw
                    .parse()
                    .map_err(|_| KisekiError::other(format!("invalid opacity {raw:?}; expected 0.0..1.0, off or reset")))?;
                if !(0.0..=1.0).contains(&parsed) {
                    return Err(KisekiError::other(format!(
                        "opacity {parsed} is outside 0.0..=1.0"
                    )));
                }
                Some(parsed)
            }
        };
        let id = state.active_theme.clone();
        let profile = state.rendering.profile;
        let loaded = self.load_theme(&id)?;
        let report = self.apply(&loaded, state, profile, "bg", None, false)?;
        self.after_simple(&report);
        Ok(())
    }

    fn font(&mut self, args: FontArgs) -> Result<()> {
        if args.lock && args.unlock {
            return Err(KisekiError::other("--lock and --unlock are mutually exclusive"));
        }
        if args.lock {
            self.config.font_locked = true;
            self.save_config()?;
        }
        if args.unlock {
            self.config.font_locked = false;
            self.save_config()?;
        }
        let mut state = self.state_or_initial();
        if args.unlock {
            // Unlocking is a single concept stored in two places: the config
            // flag and the persisted state bit. Clear both or the next
            // `kiseki use` would still treat the font as locked.
            state.typography.locked = false;
        }
        if args.style == "reset" {
            state.typography.style_override = None;
            state.typography.size_override_pt = None;
            state.typography.locked = false;
        } else {
            state.typography.style_override = Some(args.style.clone());
            if let Some(size) = args.size {
                if !(6.0..=36.0).contains(&size) {
                    return Err(KisekiError::other("font size must be within 6..36 points"));
                }
                state.typography.size_override_pt = Some(size);
            }
            if self.config.font_locked {
                state.typography.locked = true;
            }
        }
        let id = state.active_theme.clone();
        let profile = state.rendering.profile;
        let loaded = self.load_theme(&id)?;
        let report = self.apply(&loaded, state, profile, "font", None, false)?;
        self.after_simple(&report);
        Ok(())
    }

    fn after_simple(&mut self, report: &TransactionReport) {
        if self.json {
            self.emit_json(transaction_json(report));
        } else if self.dry_run {
            self.ui.info(&format!(
                "预览变更（未写入）：{} · {} · 将影响 {} 个目标",
                report.state.active_theme,
                ui::profile_label(report.state.rendering.profile),
                report.targets.len()
            ));
        } else {
            self.ui.success(&format!(
                "已更新 · {} · {} · gen {}",
                report.state.active_theme,
                ui::profile_label(report.state.rendering.profile),
                report.state.generation
            ));
        }
    }

    // ---------------------------------------------------------------- init
    fn init(&mut self, args: InitArgs) -> Result<()> {
        let detection = kiseki_pkg::detect_v1(&self.platform)?;
        let v1_detected = detection.is_some();
        // A v1 install carries preferences that must survive `init`, most
        // importantly a `static` (shader-less) profile chosen for performance.
        let v1_theme = detection
            .as_ref()
            .and_then(|detection| detection.preferences.theme.clone())
            .filter(|raw| self.theme_id(raw).is_ok());
        let v1_profile = detection.as_ref().map(|detection| {
            if detection.preferences.live {
                RenderingProfile::Balanced
            } else {
                RenderingProfile::Static
            }
        });

        let entries = self.library.list(&self.platform)?;
        let is_available = |id: &str| entries.iter().any(|entry| entry.id.as_str() == id);
        let fallback_theme = kiseki_pkg::default_theme_id().as_str().to_string();
        // A v1 theme (or a theme persisted by an earlier standalone migrate) can
        // be absent from the installed library; pin the fallback explicitly so an
        // unrelated community theme never hijacks the announced default.
        let v1_theme = v1_theme.map(|theme| {
            if is_available(&theme) {
                theme
            } else {
                self.ui.warn(&format!(
                    "v1 theme '{theme}' is not available in v2; using {fallback_theme}"
                ));
                fallback_theme.clone()
            }
        });
        let saved_theme = self.state.as_ref().map(|state| {
            let id = state.active_theme.as_str().to_string();
            if is_available(&id) {
                id
            } else {
                self.ui.warn(&format!(
                    "configured theme '{id}' is not available; using {fallback_theme}"
                ));
                fallback_theme.clone()
            }
        });
        let items = self.build_picker_items(&entries);
        let user_prompt_config = self
            .registry
            .by_id("prompt")
            .map(|adapter| adapter.detect(self.env(), &self.platform))
            .transpose()?
            .is_some_and(|detection| !detection.notes.is_empty());
        let input = InitWizardInput {
            themes: items,
            detected_terminal: self.env().current_terminal.clone(),
            detected_shell: self.platform.env_var("SHELL"),
            v1_detected,
            user_prompt_config,
            default_render_only: self.render_only,
            default_theme: saved_theme.or(v1_theme),
            default_profile: self
                .state
                .as_ref()
                .map(|state| state.rendering.profile)
                .or(v1_profile),
        };
        let choices = if args.non_interactive {
            InitWizardChoices::non_interactive(&input)
        } else {
            match kiseki_tui::run_init_wizard(input)? {
                Some(choices) => choices,
                None => {
                    if self.json {
                        self.emit_json(json!({ "cancelled": true }));
                    } else {
                        self.ui.info("已取消初始化，未修改任何文件");
                    }
                    return Ok(());
                }
            }
        };

        self.config.prompt_policy = choices.prompt_policy;
        self.config.render_only = choices.render_only || self.render_only;
        self.config.shell_aliases = args.aliases || choices.aliases;
        // The transaction engine reads `render_only` from `self`/`env`, so the
        // freshly chosen policy has to reach both before anything runs.
        self.render_only = self.config.render_only;
        self.env.render_only = self.render_only;

        // Run v1 -> v2 migration only after user confirms (never on cancel).
        if let Some(detection) = detection {
            let plan = kiseki_pkg::plan_migration(&self.platform, detection)?;
            if self.dry_run {
                // `--dry-run` promises zero mutations, so the migration is
                // reported but never executed.
                if !self.json {
                    self.ui.migration_plan(&plan);
                    self.ui.info("预览变更（未写入）：跳过 v1 迁移");
                }
            } else {
                if !args.non_interactive && !self.json {
                    self.ui.migration_plan(&plan);
                }
                let report = plan.execute_with_options(&self.platform, self.render_only)?;
                if !self.json {
                    self.ui.migration_report(&report);
                }
                self.state = State::load(&self.platform, &self.paths)?;
            }
        }

        self.save_config()?;

        let id = self.theme_id(&choices.theme)?;
        let loaded = self.load_theme(&id)?;
        let mut state = self.state_or_initial();
        state.active_theme = id;
        state.rendering.profile = choices.profile;
        let profile = choices.profile;
        let report = self.apply(
            &loaded,
            state,
            profile,
            "init",
            None,
            true,
        )?;
        if self.json {
            self.emit_json(json!({
                "choices": {
                    "theme": choices.theme,
                    "profile": choices.profile.as_str(),
                    "promptPolicy": choices.prompt_policy.as_str(),
                    "renderOnly": self.config.render_only,
                    "aliases": self.config.shell_aliases,
                },
                "transaction": transaction_json(&report),
            }));
        } else {
            let resolved = self
                .resolve(&loaded, &report.state, report.state.rendering.profile)
                .ok();
            self.ui
                .init_summary(&self.config, &report.state, resolved.as_ref(), Some(&report));
        }
        Ok(())
    }

    // -------------------------------------------------------------- doctor
    fn doctor(&mut self, args: DoctorArgs) -> Result<()> {
        let mut state = State::load(&self.platform, &self.paths)?;
        let config = Config::load(&self.platform, &self.paths);
        let config_for_report = match &config {
            Ok(config) => json!({
                "promptPolicy": config.prompt_policy.as_str(),
                "renderOnly": config.render_only,
            }),
            Err(error) => json!({ "error": error.to_string() }),
        };
        let mut report = doctor(
            &self.platform,
            &self.registry,
            self.env(),
            state.as_ref(),
            config,
        )?;
        if args.fix && self.dry_run {
            report.push(kiseki_tx::DoctorCheck::new(
                "doctor.fix",
                kiseki_tx::DoctorCheckStatus::Info,
                "dry-run: no lock was removed and no preview was recovered",
            ));
        } else if args.fix {
            let mut fixed = Vec::new();
            if self.platform.exists(&self.paths.lock_file()) {
                let owner = self
                    .platform
                    .read_to_string(&self.paths.lock_file())?
                    .and_then(|raw| {
                        raw.trim()
                            .strip_prefix("pid=")
                            .and_then(|pid| pid.parse::<u32>().ok())
                    });
                if owner.is_none_or(|pid| !self.platform.process_alive(pid)) {
                    self.platform.remove_file(&self.paths.lock_file())?;
                    fixed.push("removed stale lock file".to_string());
                } else {
                    fixed.push("lock is held by a live process; left in place".to_string());
                }
            }
            if let Some(recovery) = PreviewSession::recover_stale(&self.platform, &self.paths)? {
                state = State::load(&self.platform, &self.paths)?;
                fixed.push(format!(
                    "restored state from crashed preview (pid {})",
                    recovery.pid
                ));
            }
            for note in fixed {
                report.push(kiseki_tx::DoctorCheck::new(
                    "doctor.fix",
                    kiseki_tx::DoctorCheckStatus::Info,
                    note,
                ));
            }
        }

        let theme_validation = match state.as_ref() {
            Some(state) => match self.load_theme(&state.active_theme) {
                Ok(loaded) => kiseki_pkg::validate_theme_dir(&loaded.root).ok(),
                Err(error) => {
                    // A broken active pack is a blocking finding, not something
                    // doctor may silently skip with `.ok()`.
                    report.push(kiseki_tx::DoctorCheck::new(
                        "theme.validate",
                        kiseki_tx::DoctorCheckStatus::Error,
                        format!(
                            "active theme '{}' cannot be loaded: {error}",
                            state.active_theme
                        ),
                    ));
                    None
                }
            },
            None => None,
        };

        if let Some(validation) = theme_validation.as_ref() {
            report.push(if validation.is_valid() {
                kiseki_tx::DoctorCheck::new(
                    "theme.validate",
                    kiseki_tx::DoctorCheckStatus::Ok,
                    format!("active theme pack is valid ({} files)", validation.file_count),
                )
            } else {
                kiseki_tx::DoctorCheck::new(
                    "theme.validate",
                    kiseki_tx::DoctorCheckStatus::Error,
                    "active theme pack has blocking validation errors",
                )
            });
        }

        if let Some(theme) = state
            .as_ref()
            .and_then(|state| self.load_theme(&state.active_theme).ok())
            .and_then(|loaded| {
                let state = self.state_or_initial();
                self.resolve(&loaded, &state, state.rendering.profile).ok()
            })
        {
            let home = self
                .platform
                .env_var("HOME")
                .or_else(|| self.platform.env_var("USERPROFILE"))
                .map(PathBuf::from);
            let dirs = default_font_dirs(self.platform.os(), home.as_deref(), &|key| {
                self.platform.env_var(key)
            });
            let scanner = FontScanner::new();
            let inventory = scanner.scan(&dirs);
            let statuses = font_status(&theme.typography.families, &inventory, self.platform.os());
            let missing: Vec<&str> = statuses
                .iter()
                .filter(|status| !status.installed)
                .map(|status| status.requested.as_str())
                .collect();
            if missing.is_empty() {
                report.push(kiseki_tx::DoctorCheck::new(
                    "fonts.installed",
                    kiseki_tx::DoctorCheckStatus::Ok,
                    format!("{} theme font families resolved", statuses.len()),
                ));
            } else {
                report.push(
                    kiseki_tx::DoctorCheck::new(
                        "fonts.missing",
                        kiseki_tx::DoctorCheckStatus::Warning,
                        format!("missing theme fonts: {}", missing.join(", ")),
                    )
                    .with_hint("the terminal falls back to an installed monospace/CJK font"),
                );
            }
        }

        // Ask the real Ghostty binary when it is available; this is the only
        // check that can prove a generated config is accepted by the host.
        let ghostty_conf = self
            .paths
            .generated(kiseki_compiler::artifact_names::GHOSTTY_CONF);
        if self.platform.exists(&ghostty_conf) {
            match kiseki_platform::validate_ghostty_config(&self.platform, &ghostty_conf) {
                Ok(Some(outcome)) if outcome.ok => report.push(kiseki_tx::DoctorCheck::new(
                    "adapter.ghostty.validate",
                    kiseki_tx::DoctorCheckStatus::Ok,
                    "ghostty +validate-config accepted generated/ghostty.conf",
                )),
                Ok(Some(outcome)) => report.push(kiseki_tx::DoctorCheck::new(
                    "adapter.ghostty.validate",
                    kiseki_tx::DoctorCheckStatus::Error,
                    format!(
                        "ghostty rejected generated/ghostty.conf: {}",
                        outcome.stderr.trim()
                    ),
                )),
                Ok(None) => report.push(kiseki_tx::DoctorCheck::new(
                    "adapter.ghostty.validate",
                    kiseki_tx::DoctorCheckStatus::Info,
                    "ghostty binary not on PATH; config was linted statically",
                )),
                Err(error) => report.push(kiseki_tx::DoctorCheck::new(
                    "adapter.ghostty.validate",
                    kiseki_tx::DoctorCheckStatus::Warning,
                    format!("could not run ghostty +validate-config: {error}"),
                )),
            }
        }

        if self.json {
            self.emit_json(json!({
                "checks": report.checks.iter().map(|check| json!({
                    "name": check.name,
                    "status": check.status.as_str(),
                    "message": check.message,
                    "hint": check.hint,
                })).collect::<Vec<_>>(),
                "state": state.as_ref().map(state_json),
                "config": config_for_report,
                "themeValidation": theme_validation.as_ref().map(|validation| json!({
                    "valid": validation.is_valid(),
                    "files": validation.file_count,
                    "bytes": validation.total_bytes,
                    "shaders": validation.shader_summaries(),
                    "diagnostics": validation.diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
                })),
                "hasErrors": report.has_errors(),
            }));
            // Match human mode: a blocking finding is a non-zero exit, but the
            // JSON document is emitted exactly once above.
            if report.has_errors() {
                return Err(KisekiError::other(
                    "doctor found blocking problems; see the report above",
                ));
            }
            return Ok(());
        }
        self.ui.doctor(&report);
        if let (Some(validation), Some(state)) = (theme_validation.as_ref(), state.as_ref()) {
            if let Ok(loaded) = self.load_theme(&state.active_theme) {
                self.ui.validation(&loaded.root, validation);
            }
        }
        if report.has_errors() {
            return Err(KisekiError::other(
                "doctor found blocking problems; see the report above",
            ));
        }
        Ok(())
    }

    // ------------------------------------------------------------ rollback
    fn rollback(&mut self, args: RollbackArgs) -> Result<()> {
        let manager = TransactionManager::new(&self.platform, AdapterRegistry::milestone1());
        if args.list {
            let summaries = manager.list_journals()?;
            if self.json {
                self.emit_json(json!({
                    "journals": summaries.iter().map(journal_json).collect::<Vec<_>>(),
                }));
            } else {
                self.ui.journals(&summaries);
            }
            return Ok(());
        }
        let tx_id = match args.tx_id {
            Some(tx_id) => tx_id,
            None => manager
                .list_journals()?
                .first()
                .map(|summary| summary.tx_id.clone())
                .ok_or_else(|| KisekiError::other("no transaction to roll back"))?,
        };
        if self.dry_run {
            if self.json {
                self.emit_json(json!({ "dryRun": true, "txId": tx_id }));
            } else {
                self.ui
                    .info(&format!("预览变更（未写入）：将回滚事务 {tx_id}"));
            }
            return Ok(());
        }
        let report = manager.rollback(&tx_id)?;
        self.state = Some(report.state.clone());
        if self.json {
            self.emit_json(json!({
                "txId": report.tx_id,
                "restored": report.restored,
                "state": state_json(&report.state),
            }));
        } else {
            self.ui.rollback(&report);
        }
        Ok(())
    }

    // ------------------------------------------------------------- recover
    fn recover(&mut self, args: RecoverArgs) -> Result<()> {
        if !args.safe {
            return Err(KisekiError::other(
                "only `kiseki recover --safe` is supported; add --safe to continue",
            ));
        }
        let manager = TransactionManager::new(&self.platform, AdapterRegistry::milestone1());
        if args.rollback && !self.dry_run {
            if let Some(summary) = manager.list_journals()?.first() {
                let report = manager.rollback(&summary.tx_id)?;
                self.state = Some(report.state.clone());
            }
        }
        let mut state = if args.rollback && self.dry_run {
            manager
                .latest_journal_state()?
                .or_else(|| State::load(&self.platform, &self.paths).ok().flatten())
        } else {
            match State::load(&self.platform, &self.paths) {
                Ok(state) => state,
                Err(error) => {
                    // A corrupt state.json is exactly what `recover` is for. Read
                    // the newest journal's `state_before` only: rolling back here
                    // would mutate host files (and must never happen on --dry-run).
                    self.ui
                        .warn(&format!("{error}; rebuilding state from the last journal"));
                    manager.latest_journal_state()?
                }
            }
        }
        .unwrap_or_else(|| {
            State::new(
                kiseki_pkg::default_theme_id(),
                &self.platform.now_rfc3339(),
            )
        });

        // Apply the safe profile parameters in memory so both `--dry-run` and
        // real execution preview/apply the exact same theme and composition.
        state.rendering.profile = RenderingProfile::Static;
        state.rendering.crt = false;
        state.rendering.bg_opacity_override = None;

        let mut loaded = match if self.dry_run {
            self.library.load_readonly(&self.platform, &state.active_theme)
        } else {
            self.load_theme(&state.active_theme)
        } {
            Ok(loaded) => loaded,
            Err(error) => {
                let fallback = kiseki_pkg::default_theme_id();
                self.ui.warn(&format!(
                    "active theme '{}' cannot be loaded ({error}); falling back to {}",
                    state.active_theme, fallback
                ));
                state.composition.clear();
                state.active_theme = fallback.clone();
                if self.dry_run {
                    self.library.load_readonly(&self.platform, &fallback)?
                } else {
                    self.library.repair_bundled(&self.platform, fallback.as_str())?
                }
            }
        };

        // A composition donor may have been removed since the last `use`.
        // Dropping the stale layer is safer than leaving the shader mounted.
        if let Err(error) = self.resolve(&loaded, &state, RenderingProfile::Static) {
            if state.composition.is_empty() {
                return Err(error);
            }
            self.ui.warn(&format!(
                "composition cannot be resolved ({error}); clearing it before recovery"
            ));
            state.composition.clear();
            if let Err(error) = self.resolve(&loaded, &state, RenderingProfile::Static) {
                let fallback = kiseki_pkg::default_theme_id();
                self.ui.warn(&format!(
                    "active theme '{}' cannot be resolved ({error}); falling back to {fallback}",
                    state.active_theme
                ));
                state.active_theme = fallback.clone();
                loaded = if self.dry_run {
                    self.library.load_readonly(&self.platform, &fallback)?
                } else {
                    self.library.repair_bundled(&self.platform, fallback.as_str())?
                };
            }
        }

        if self.dry_run {
            // Preview only: never abort a live preview session or touch disk.
            if self.json {
                self.emit_json(json!({
                    "dryRun": true,
                    "state": state_json(&state),
                }));
            } else {
                self.ui.info(&format!(
                    "预览变更（未写入）：将恢复为 {} · static",
                    state.active_theme
                ));
            }
            return Ok(());
        }

        // Abort any active preview session *without* committing state yet; keep
        // `self.state` untouched so `apply_inner` records the pre-recovery state
        // as `state_before` in the journal.
        manager.prepare_safe_state(&mut state)?;
        state.rendering.profile = RenderingProfile::Static;

        let report = self.apply_inner(
            &loaded,
            state,
            RenderingProfile::Static,
            "recover --safe",
            None,
            false,
            true,
        )?;
        // Recovery is only successful when every target actually reached disk.
        // A read-only `generated/` file (or a read-only host config that still
        // points at the dynamic shader) means the live shader survived.
        let blocking: Vec<String> = report
            .targets
            .iter()
            .filter(|target| match target.status {
                TargetStatus::Failed | TargetStatus::RolledBack => true,
                // In an intentional render-only transaction the host was never
                // mounted, so its render-only downgrade is not a stale shader.
                TargetStatus::RenderOnly => !report.render_only,
                TargetStatus::Applied | TargetStatus::Skipped | TargetStatus::DryRun => false,
            })
            .map(|target| format!("{} ({})", target.target.display(), target.status.as_str()))
            .collect();
        if self.json {
            self.emit_json(json!({
                "ok": blocking.is_empty(),
                "state": state_json(&report.state),
                "transaction": transaction_json(&report),
                "blockingTargets": blocking,
            }));
        } else if blocking.is_empty() {
            self.ui.recover(&report.state);
        }
        if !blocking.is_empty() {
            return Err(KisekiError::other(format!(
                "recover --safe could not write every target: {}",
                blocking.join(", ")
            )));
        }
        Ok(())
    }

    // --------------------------------------------------------------- theme
    fn theme(&mut self, args: ThemeArgs) -> Result<()> {
        match args.command {
            ThemeCommand::List => {
                let entries = self.library.list(&self.platform)?;
                if self.json {
                    self.emit_json(json!({
                        "themes": entries.iter().map(theme_entry_json).collect::<Vec<_>>(),
                    }));
                } else {
                    let active = self
                        .state
                        .as_ref()
                        .map(|state| state.active_theme.as_str().to_string());
                    self.ui.theme_list(&entries, active.as_deref());
                }
            }
            ThemeCommand::Info { theme } => {
                let id = self.theme_id(&theme)?;
                let loaded = self.load_theme(&id)?;
                let state = self.state_or_initial();
                let profile = state.rendering.profile;
                let resolved = self.resolve(&loaded, &state, profile)?;
                let contrast = resolved.contrast_report();
                if self.json {
                    self.emit_json(json!({
                        "manifest": loaded.pack.manifest,
                        "source": loaded.source.as_str(),
                        "root": loaded.root,
                        "contrast": contrast.checks.iter().map(|check| json!({
                            "name": check.name,
                            "ratio": check.ratio,
                            "minimum": check.minimum,
                            "passes": check.passes_minimum(),
                        })).collect::<Vec<_>>(),
                    }));
                } else {
                    self.ui.theme_info(&resolved, &contrast, &loaded.root);
                }
            }
            ThemeCommand::Validate { path } => {
                let validation = kiseki_pkg::validate_theme_dir(&path)?;
                if self.json {
                    self.emit_json(json!({
                        "path": path,
                        "valid": validation.is_valid(),
                        "files": validation.file_count,
                        "bytes": validation.total_bytes,
                        "diagnostics": validation.diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
                        "shaders": validation.shader_summaries(),
                        "contrast": validation.contrast.as_ref().map(|contrast| contrast.checks.iter().map(|check| json!({
                            "name": check.name,
                            "ratio": check.ratio,
                            "minimum": check.minimum,
                            "passes": check.passes_minimum(),
                        })).collect::<Vec<_>>()),
                    }));
                } else {
                    self.ui.validation(&path, &validation);
                }
                if !validation.is_valid() {
                    return Err(KisekiError::schema("theme pack has blocking errors"));
                }
            }
            ThemeCommand::Install { path, force } => {
                if self.dry_run {
                    // Validate the pack (read-only) so the preview is honest,
                    // but never copy a byte into the community store.
                    let validation = kiseki_pkg::validate_theme_dir(&path)?;
                    let id = validation
                        .pack
                        .as_ref()
                        .map(|pack| pack.manifest.id.as_str().to_string());
                    if self.json {
                        self.emit_json(json!({
                            "dryRun": true,
                            "id": id,
                            "path": path,
                            "valid": validation.is_valid(),
                            "files": validation.file_count,
                            "bytes": validation.total_bytes,
                        }));
                    } else {
                        self.ui.info(&format!(
                            "预览变更（未写入）：将安装 {}（{} 个文件）",
                            id.unwrap_or_else(|| path.display().to_string()),
                            validation.file_count
                        ));
                    }
                    if !validation.is_valid() {
                        // The preview is honest about the pack, but an invalid
                        // pack must still be a non-zero exit.
                        return Err(KisekiError::schema("theme pack has blocking errors"));
                    }
                    return Ok(());
                }
                let installed = kiseki_pkg::install_from_dir(&self.platform, &path, force)?;
                if self.json {
                    self.emit_json(json!({
                        "id": installed.id,
                        "path": installed.path,
                        "files": installed.file_count,
                        "bytes": installed.total_bytes,
                    }));
                } else {
                    self.ui.success(&format!(
                        "installed {} ({} files, {:.1} KiB) at {}",
                        installed.id,
                        installed.file_count,
                        installed.total_bytes as f64 / 1024.0,
                        installed.path.display()
                    ));
                }
            }
            ThemeCommand::Remove { theme } => {
                if self.dry_run {
                    // Resolve the id exactly like the real removal would, but
                    // stop before deleting anything.
                    let id = self.theme_id(&theme)?;
                    let path = self.paths.community_themes_dir().join(id.as_str());
                    if self.json {
                        self.emit_json(json!({ "dryRun": true, "removed": theme, "path": path }));
                    } else {
                        self.ui.info(&format!(
                            "预览变更（未写入）：将从 {} 移除 {theme}",
                            path.display()
                        ));
                    }
                    return Ok(());
                }
                let removed = kiseki_pkg::remove_installed(&self.platform, &theme)?;
                if self.json {
                    self.emit_json(json!({ "removed": theme, "path": removed }));
                } else {
                    self.ui.success(&format!("removed {theme}"));
                }
            }
        }
        Ok(())
    }

    // --------------------------------------------------------------- fonts
    fn fonts(&mut self, args: FontsArgs) -> Result<()> {
        match args.command {
            FontsCommand::Status => {
                let state = self.state_or_initial();
                let loaded = self.load_theme(&state.active_theme)?;
                let theme = self.resolve(&loaded, &state, state.rendering.profile)?;
                let home = self
                    .platform
                    .env_var("HOME")
                    .or_else(|| self.platform.env_var("USERPROFILE"))
                    .map(PathBuf::from);
                let dirs = default_font_dirs(self.platform.os(), home.as_deref(), &|key| self.platform.env_var(key));
                let scanner = FontScanner::new();
                let inventory = scanner.scan(&dirs);
                let statuses = font_status(&theme.typography.families, &inventory, self.platform.os());
                if self.json {
                    self.emit_json(json!({
                        "families": statuses.iter().map(|status| json!({
                            "requested": status.requested,
                            "catalogId": status.catalog_id,
                            "resolvedFamily": status.resolved_family,
                            "installed": status.installed,
                            "path": status.path,
                        })).collect::<Vec<_>>(),
                        "scannedDirs": dirs,
                    }));
                } else {
                    self.ui.fonts(&statuses, &theme.typography.families);
                }
            }
            FontsCommand::Install { target } => {
                return Err(KisekiError::other(format!(
                    "`kiseki fonts install {target}` lands in Milestone 2 (SHA256-verified font catalog)"
                )));
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- migrate
    fn migrate(&mut self, args: MigrateArgs) -> Result<()> {
        let Some(detection) = kiseki_pkg::detect_v1(&self.platform)? else {
            if self.json {
                self.emit_json(json!({ "detected": false }));
            } else {
                self.ui.info("no v1 installation detected");
            }
            return Ok(());
        };
        let plan = kiseki_pkg::plan_migration(&self.platform, detection)?;
        if args.apply && self.dry_run {
            if self.json {
                self.emit_json(json!({
                    "applied": false,
                    "dryRun": true,
                    "plan": plan.summary(),
                    "backup": plan.backup_dir,
                }));
            } else {
                self.ui.migration_plan(&plan);
                self.ui.info("预览变更（未写入）：跳过迁移执行");
            }
        } else if args.apply {
            let report = plan.execute_with_options(&self.platform, self.render_only)?;
            self.state = State::load(&self.platform, &self.paths)?;
            if self.json {
                self.emit_json(json!({
                    "applied": true,
                    "backup": report.backup_dir,
                    "removedHooks": report.removed_marker_blocks,
                    "deletedShims": report.deleted_shims,
                    "state": report.state.as_ref().map(state_json),
                }));
            } else {
                self.ui.migration_report(&report);
            }
        } else if self.json {
            self.emit_json(json!({
                "applied": false,
                "plan": plan.summary(),
                "backup": plan.backup_dir,
            }));
        } else {
            self.ui.migration_plan(&plan);
        }
        Ok(())
    }

    // ----------------------------------------------------------- uninstall
    fn uninstall(&mut self, args: UninstallArgs) -> Result<()> {
        let state = self.state_or_initial();
        let mut plans = Vec::new();
        for adapter in self.registry.iter() {
            let detection = adapter.detect(self.env(), &self.platform)?;
            if detection.installed || detection.config_path.is_some() {
                plans.push(adapter.plan_unhook(&detection, &self.paths)?);
            }
        }
        let manager = TransactionManager::new(&self.platform, AdapterRegistry::milestone1());
        let report = manager.apply_unhooks(&state, plans, "uninstall", self.dry_run)?;

        // A read-only or failed host unhook means the host still references
        // Kiseki. Deleting `generated/`, the journals or the WT baseline would
        // remove the only way to finish the uninstall after fixing permissions.
        let incomplete = report.targets.iter().any(|target| {
            matches!(
                target.status,
                TargetStatus::RenderOnly | TargetStatus::RolledBack | TargetStatus::Failed
            )
        });
        if incomplete && !self.dry_run {
            self.ui.warn(
                "some host files could not be unhooked; recovery artifacts (generated/, journal/, wt-baseline.json) were kept — fix the permissions and re-run `kiseki uninstall`",
            );
        }

        if !self.dry_run {
            self.platform.remove_file(&self.paths.state_file())?;
            self.platform.remove_file(&self.paths.gen_file())?;
            self.platform.remove_file(&self.paths.config_file())?;
            self.platform.remove_file(&self.paths.preview_recovery_file())?;
            self.platform.remove_file(&self.paths.preview_lock_file())?;
            self.platform
                .remove_file(&self.paths.config_dir().join("appearance"))?;
            if !incomplete {
                remove_dir_all(&self.paths.generated_dir())?;
            }
            remove_dir_all(&self.paths.preview_dir())?;
            if args.purge && !incomplete {
                remove_dir_all(self.paths.data_dir())?;
                remove_dir_all(self.paths.cache_dir())?;
            }
            self.state = None;
        }
        let purged = args.purge && !incomplete;
        if self.json {
            self.emit_json(json!({
                "purged": purged,
                "incomplete": incomplete,
                "transaction": transaction_json(&report),
            }));
        } else {
            self.ui.uninstall(&report, purged, incomplete);
        }
        Ok(())
    }

    // ---------------------------------------------------------- shell-init
    fn shell_init(&mut self, shell: &str) -> Result<()> {
        let shell = kiseki_compiler::ShellKind::parse(shell)
            .ok_or_else(|| KisekiError::other("shell must be zsh or pwsh"))?;
        let script = kiseki_compiler::shell_init_script(shell, self.paths.config_dir(), self.config.shell_aliases);
        if self.json {
            self.emit_json(json!({ "shell": shell.as_str(), "script": script }));
        } else {
            print!("{script}");
            std::io::stdout()
                .flush()
                .map_err(|error| KisekiError::io("stdout", error))?;
        }
        Ok(())
    }

    fn completions(&mut self, shell: &str) -> Result<()> {
        let generator = match shell.to_ascii_lowercase().as_str() {
            "bash" => clap_complete::Shell::Bash,
            "zsh" => clap_complete::Shell::Zsh,
            "fish" => clap_complete::Shell::Fish,
            "powershell" | "pwsh" => clap_complete::Shell::PowerShell,
            "elvish" => clap_complete::Shell::Elvish,
            other => {
                return Err(KisekiError::other(format!(
                    "unsupported shell {other:?}; expected bash|zsh|fish|powershell|elvish"
                )))
            }
        };
        if self.json {
            let mut buffer = Vec::new();
            clap_complete::generate(generator, &mut Cli::command(), "kiseki", &mut buffer);
            self.emit_json(json!({
                "shell": shell,
                "script": String::from_utf8_lossy(&buffer),
            }));
        } else {
            let mut stdout = std::io::stdout();
            clap_complete::generate(generator, &mut Cli::command(), "kiseki", &mut stdout);
        }
        Ok(())
    }

    fn fetch(&mut self) -> Result<()> {
        let config = self.paths.generated(kiseki_compiler::artifact_names::FASTFETCH);
        if !self.platform.exists(&config) {
            return Err(KisekiError::other(
                "no generated fastfetch config yet; run `kiseki use <theme>` first",
            ));
        }
        let Some(binary) = self.platform.which("fastfetch") else {
            if self.json {
                self.emit_json(json!({ "installed": false, "config": config }));
            } else {
                self.ui.warn("fastfetch is not installed");
                self.ui.bullet(&config.display().to_string());
            }
            return Ok(());
        };
        let output = self
            .platform
            .run(&binary.to_string_lossy(), &["--config", &config.to_string_lossy()])?;
        if self.json {
            self.emit_json(json!({
                "status": output.status,
                "stdout": output.stdout,
                "stderr": output.stderr,
            }));
        } else {
            print!("{}", output.stdout);
            if !output.success() {
                return Err(KisekiError::other(format!(
                    "fastfetch exited with {}: {}",
                    output.status,
                    output.stderr.trim()
                )));
            }
        }
        Ok(())
    }

    fn detect_all(&self) -> Vec<(String, DetectionReport)> {
        let options = PlanOptions::default();
        self.registry
            .detect_all(self.env(), &self.platform, &options)
            .into_iter()
            .filter_map(|(id, result)| result.ok().map(|detection| (id, detection)))
            .collect()
    }

    fn build_picker_items(&self, entries: &[ThemeEntry]) -> Vec<PickerItem> {
        entries
            .iter()
            .map(|entry| {
                let mut item = PickerItem::new(entry.id.as_str(), &entry.name)
                    .with_appearance(entry.appearance)
                    .active(
                        self.state
                            .as_ref()
                            .is_some_and(|state| state.active_theme == entry.id),
                    );
                if let Some(tagline) = &entry.tagline {
                    item = item.with_subtitle(tagline);
                }
                if entry.source == ThemeSource::Bundled {
                    item = item.with_badge("内置");
                }
                // Read-only: launching `kiseki init` and pressing Esc must
                // write nothing, so never materialise a bundled pack here.
                if let Ok(pack) = self.library.load_pack_readonly(&entry.id) {
                    let resolved = ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced);
                    item = item.with_swatch(ui::swatch_from_resolved(&resolved));
                }
                item
            })
            .collect()
    }
}

fn emit_json(value: serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
    );
}

fn diagnostic_json(diagnostic: &Diagnostic) -> serde_json::Value {
    json!({
        "severity": diagnostic.severity.as_str(),
        "code": diagnostic.code,
        "message": diagnostic.message,
        "hint": diagnostic.hint,
    })
}

fn state_json(state: &State) -> serde_json::Value {
    json!({
        "schemaVersion": state.schema_version,
        "generation": state.generation,
        "activeTheme": state.active_theme,
        "appearanceMode": match state.appearance_mode {
            AppearanceMode::Manual => "manual",
            AppearanceMode::Auto => "auto",
        },
        "composition": composition_json(state),
        "rendering": {
            "profile": state.rendering.profile.as_str(),
            "crt": state.rendering.crt,
            "bgOpacityOverride": state.rendering.bg_opacity_override,
        },
        "typography": {
            "locked": state.typography.locked,
            "styleOverride": state.typography.style_override,
            "sizeOverridePt": state.typography.size_override_pt,
        },
        "updatedAt": state.updated_at,
    })
}

fn composition_json(state: &State) -> serde_json::Value {
    json!({
        "palette": state.composition.palette.as_ref().map(|id| id.as_str()),
        "syntax": state.composition.syntax.as_ref().map(|id| id.as_str()),
        "atmosphere": state.composition.atmosphere.as_ref().map(|id| id.as_str()),
        "typography": state.composition.typography.as_ref().map(|id| id.as_str()),
        "overrides": state.composition.overrides.as_ref().map(|id| id.as_str()),
    })
}

fn env_json(env: &EnvContext) -> serde_json::Value {
    json!({
        "os": env.os.as_str(),
        "terminal": env.current_terminal,
        "isSsh": env.is_ssh,
        "isWsl": env.is_wsl,
        "multiplexer": env.multiplexer.map(|mux| mux.as_str()),
        "powerSource": env.power_source.as_str(),
        "systemAppearance": env.system_appearance.as_str(),
        "renderOnly": env.render_only,
    })
}

fn theme_json(theme: &ResolvedTheme) -> serde_json::Value {
    json!({
        "id": theme.id,
        "name": theme.name,
        "appearance": theme.appearance.as_str(),
        "profile": theme.profile.as_str(),
        "shaderEnabled": theme.shader_enabled,
        "crtEnabled": theme.crt_enabled,
        "wallpaperOpacity": theme.wallpaper_opacity,
        "windowOpacity": theme.window_opacity,
        "base": theme.palette.surfaces.base.to_hex(),
        "foreground": theme.palette.text.primary.to_hex(),
        "accent": theme.palette.accents.primary.to_hex(),
    })
}

fn transaction_json(report: &TransactionReport) -> serde_json::Value {
    json!({
        "txId": report.tx_id,
        "dryRun": report.dry_run,
        "renderOnly": report.render_only,
        "generation": report.state.generation,
        "targets": report.targets.iter().map(|target| json!({
            "adapter": target.adapter,
            "target": target.target,
            "status": target.status.as_str(),
            "notes": target.notes,
        })).collect::<Vec<_>>(),
        "diagnostics": report.diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
        "reload": report.reload.iter().map(|(adapter, detail)| json!({
            "adapter": adapter,
            "detail": detail,
        })).collect::<Vec<_>>(),
    })
}

fn theme_entry_json(entry: &ThemeEntry) -> serde_json::Value {
    json!({
        "id": entry.id,
        "name": entry.name,
        "version": entry.version,
        "appearance": entry.appearance.as_str(),
        "source": entry.source.as_str(),
        "tagline": entry.tagline,
        "layers": entry.layers,
        "root": entry.root,
    })
}

fn journal_json(summary: &JournalSummary) -> serde_json::Value {
    json!({
        "txId": summary.tx_id,
        "createdAt": summary.created_at,
        "command": summary.command,
        "theme": summary.theme,
        "profile": summary.profile.as_str(),
        "entries": summary.entries,
        "path": summary.path,
    })
}

fn remove_dir_all(path: &std::path::Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(KisekiError::io(path, error)),
    }
}
