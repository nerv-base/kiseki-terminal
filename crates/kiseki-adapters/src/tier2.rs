//! Tier 2 ecosystem opt-in adapters (RFC-0003 §4).
//!
//! All ten Tier 2 artefacts are always compiled into `<kiseki_config>/generated/`
//! so dotfiles / Nix users can reference them immediately. Host drops (e.g.
//! `themes/kiseki.*`, `colors/kiseki.*`, or `tmux.conf` anchor) only happen
//! when the user explicitly opts in via `--only <id>` or `enabledAdapters` in
//! `config.toml`, guaranteeing zero unsolicited edits to complex editor/tool configs.

use std::path::PathBuf;

use kiseki_compiler::artifact_names;
use kiseki_compiler::marker::{MARKER_BEGIN, MARKER_END};
use kiseki_platform::{PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, Multiplexer, OsKind, Result};

use crate::{
    render_only_diagnostic, Adapter, AdapterKind, AdapterPlan, DetectionReport, FileOperation,
    IntegrationTier, PlanContext, ValidationReport,
};

fn home_dir(platform: &dyn Platform) -> Option<PathBuf> {
    platform
        .env_var("HOME")
        .or_else(|| platform.env_var("USERPROFILE"))
        .map(PathBuf::from)
}

fn xdg_config_home(platform: &dyn Platform) -> Option<PathBuf> {
    if let Some(xdg) = platform.env_var("XDG_CONFIG_HOME").filter(|v| !v.trim().is_empty()) {
        return Some(PathBuf::from(xdg));
    }
    home_dir(platform).map(|h| h.join(".config"))
}

fn appdata_or_xdg(platform: &dyn Platform) -> Option<PathBuf> {
    if platform.os() == OsKind::Windows {
        platform
            .env_var("APPDATA")
            .map(PathBuf::from)
            .or_else(|| xdg_config_home(platform))
    } else {
        xdg_config_home(platform)
    }
}

fn plan_drop_in_theme(
    id: &'static str,
    artifact_rel: &'static str,
    ctx: &PlanContext<'_>,
    detection: &DetectionReport,
    usage_hint: &str,
) -> AdapterPlan {
    let mut plan = AdapterPlan::new(id);
    if !ctx.options.explicit_only {
        // Generated file is still emitted via `generated_baseline`; keep plan
        // empty so default auto runs never mutate Tier 2 host directories.
        return plan;
    }
    plan.add_generated(ctx.artifacts, artifact_rel);
    let Some(content) = ctx.artifacts.get(artifact_rel) else {
        return plan;
    };
    let Some(target) = detection.config_path.as_ref() else {
        return plan;
    };
    if !ctx.options.host_writes_allowed(ctx.env) {
        plan.render_only = true;
        plan.diagnostic(
            Diagnostic::info(
                format!("adapter.{id}.render-only"),
                format!("host writes suppressed; copy generated/{artifact_rel} manually"),
            )
            .with_hint(usage_hint.to_string()),
        );
        return plan;
    }
    if !detection.path_kind.is_writable() {
        plan.render_only = true;
        plan.diagnostic(render_only_diagnostic(
            id,
            target,
            detection.path_kind.label(),
            usage_hint.to_string(),
        ));
        return plan;
    }
    plan.push(FileOperation::WriteHostFile {
        target: target.clone(),
        content: content.to_string(),
    });
    plan
}

fn plan_drop_in_unhook(id: &'static str, detection: &DetectionReport) -> AdapterPlan {
    let mut plan = AdapterPlan::new(id);
    if let Some(target) = detection.config_path.as_ref() {
        plan.push(FileOperation::RemoveHostFile {
            target: target.clone(),
        });
    }
    plan
}

// ---------------------------------------------------------------------------
// Neovim
// ---------------------------------------------------------------------------

pub struct NeovimAdapter;

impl NeovimAdapter {
    fn target_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_NVIM_COLORS_FILE")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        let base = if platform.os() == OsKind::Windows {
            platform
                .env_var("LOCALAPPDATA")
                .map(PathBuf::from)
                .or_else(|| xdg_config_home(platform))?
        } else {
            xdg_config_home(platform)?
        };
        Some(base.join("nvim").join("colors").join("kiseki.lua"))
    }
}

impl Adapter for NeovimAdapter {
    fn id(&self) -> &'static str { "neovim" }
    fn display_name(&self) -> &'static str { "Neovim" }
    fn kind(&self) -> AdapterKind { AdapterKind::Editor }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = Self::target_path(platform);
        let path_kind = config_path.as_deref().map(|p| platform.inspect(p)).unwrap_or(PathKind::Missing);
        let installed = platform.which("nvim").is_some() || path_kind != PathKind::Missing;
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: platform.env_var("NVIM").is_some(),
            notes: vec!["opt-in Tier 2: use `--only neovim` then `:colorscheme kiseki`".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, _e: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::TRUECOLOR | CapabilityFlags::PALETTE_16
    }

    fn plan(&self, ctx: &PlanContext<'_>, detection: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        Ok(plan_drop_in_theme(
            self.id(),
            artifact_names::NVIM_LUA,
            ctx,
            detection,
            "add `vim.cmd.colorscheme('kiseki')` or `dofile('.../generated/nvim/kiseki.lua')` to init.lua",
        ))
    }

    fn validate(&self, _p: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        Ok(ValidationReport::ok())
    }

    fn reload(&self, _d: &DetectionReport, _c: CapabilityFlags, _pl: &dyn Platform) -> Result<ReloadOutcome> {
        Ok(ReloadOutcome::AutoReload {
            detail: "run `:colorscheme kiseki` in Neovim to pick up the updated palette".to_string(),
        })
    }

    fn plan_unhook(&self, detection: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        Ok(plan_drop_in_unhook(self.id(), detection))
    }
}

// ---------------------------------------------------------------------------
// Vim
// ---------------------------------------------------------------------------

pub struct VimAdapter;

impl VimAdapter {
    fn target_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_VIM_COLORS_FILE")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        Some(home_dir(platform)?.join(".vim").join("colors").join("kiseki.vim"))
    }
}

impl Adapter for VimAdapter {
    fn id(&self) -> &'static str { "vim" }
    fn display_name(&self) -> &'static str { "Vim" }
    fn kind(&self) -> AdapterKind { AdapterKind::Editor }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = Self::target_path(platform);
        let path_kind = config_path.as_deref().map(|p| platform.inspect(p)).unwrap_or(PathKind::Missing);
        let installed = platform.which("vim").is_some() || path_kind != PathKind::Missing;
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes: vec!["opt-in Tier 2: use `--only vim` then `:colorscheme kiseki`".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, _e: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::TRUECOLOR | CapabilityFlags::PALETTE_16
    }

    fn plan(&self, ctx: &PlanContext<'_>, detection: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        Ok(plan_drop_in_theme(
            self.id(),
            artifact_names::VIM_VIM,
            ctx,
            detection,
            "add `colorscheme kiseki` to ~/.vimrc",
        ))
    }

    fn validate(&self, _p: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        Ok(ValidationReport::ok())
    }

    fn reload(&self, _d: &DetectionReport, _c: CapabilityFlags, _pl: &dyn Platform) -> Result<ReloadOutcome> {
        Ok(ReloadOutcome::AutoReload {
            detail: "run `:colorscheme kiseki` in Vim".to_string(),
        })
    }

    fn plan_unhook(&self, detection: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        Ok(plan_drop_in_unhook(self.id(), detection))
    }
}

// ---------------------------------------------------------------------------
// Helix
// ---------------------------------------------------------------------------

pub struct HelixAdapter;

impl HelixAdapter {
    fn target_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_HELIX_THEME_FILE")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        Some(appdata_or_xdg(platform)?.join("helix").join("themes").join("kiseki.toml"))
    }
}

impl Adapter for HelixAdapter {
    fn id(&self) -> &'static str { "helix" }
    fn display_name(&self) -> &'static str { "Helix" }
    fn kind(&self) -> AdapterKind { AdapterKind::Editor }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = Self::target_path(platform);
        let path_kind = config_path.as_deref().map(|p| platform.inspect(p)).unwrap_or(PathKind::Missing);
        let installed = platform.which("hx").is_some()
            || platform.which("helix").is_some()
            || path_kind != PathKind::Missing;
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes: vec!["opt-in Tier 2: set `theme = \"kiseki\"` in helix/config.toml".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, env: &EnvContext) -> CapabilityFlags {
        let mut flags = CapabilityFlags::TRUECOLOR | CapabilityFlags::PALETTE_16;
        if !env.is_ssh && env.os != OsKind::Windows {
            flags |= CapabilityFlags::HOT_RELOAD_SIGNAL;
        }
        flags
    }

    fn plan(&self, ctx: &PlanContext<'_>, detection: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        Ok(plan_drop_in_theme(
            self.id(),
            artifact_names::HELIX_TOML,
            ctx,
            detection,
            "set `theme = \"kiseki\"` in ~/.config/helix/config.toml",
        ))
    }

    fn validate(&self, plan: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for (path, content) in plan.generated_writes() {
            if path == artifact_names::HELIX_TOML && toml::from_str::<toml::Value>(content).is_err() {
                report.push(Diagnostic::error(
                    "adapter.helix.invalid-toml",
                    "generated helix/kiseki.toml is not valid TOML",
                ));
            }
        }
        Ok(report)
    }

    fn reload(&self, _d: &DetectionReport, caps: CapabilityFlags, platform: &dyn Platform) -> Result<ReloadOutcome> {
        if !caps.contains(CapabilityFlags::HOT_RELOAD_SIGNAL) {
            return Ok(ReloadOutcome::AutoReload {
                detail: "Helix theme updated; run `:config-reload` inside Helix".to_string(),
            });
        }
        let args: &[&str] = if platform.os() == OsKind::MacOs {
            &["-USR1", "-a", "-x", "hx"]
        } else {
            &["-USR1", "-x", "hx"]
        };
        match platform.run("pkill", args) {
            Ok(out) if out.success() => Ok(ReloadOutcome::Signalled {
                detail: "sent SIGUSR1 to hx".to_string(),
            }),
            _ => Ok(ReloadOutcome::NotRunning {
                detail: "no running Helix (hx) instance found".to_string(),
            }),
        }
    }

    fn plan_unhook(&self, detection: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        Ok(plan_drop_in_unhook(self.id(), detection))
    }
}

// ---------------------------------------------------------------------------
// tmux
// ---------------------------------------------------------------------------

pub struct TmuxAdapter;

impl TmuxAdapter {
    fn config_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_TMUX_CONF")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        if platform.os() == OsKind::Windows {
            return None;
        }
        let home = home_dir(platform)?;
        let xdg = xdg_config_home(platform)?.join("tmux").join("tmux.conf");
        let dot = home.join(".tmux.conf");
        if platform.exists(&dot) {
            Some(dot)
        } else if platform.exists(&xdg) {
            Some(xdg)
        } else {
            Some(dot)
        }
    }
}

impl Adapter for TmuxAdapter {
    fn id(&self) -> &'static str { "tmux" }
    fn display_name(&self) -> &'static str { "tmux" }
    fn kind(&self) -> AdapterKind { AdapterKind::Multiplexer }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        if platform.os() == OsKind::Windows {
            return Ok(DetectionReport::not_installed("tmux targets Unix/WSL"));
        }
        let config_path = Self::config_path(platform);
        let path_kind = config_path.as_deref().map(|p| platform.inspect(p)).unwrap_or(PathKind::Missing);
        let installed = platform.which("tmux").is_some()
            || path_kind != PathKind::Missing
            || env.multiplexer == Some(Multiplexer::Tmux);
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: env.multiplexer == Some(Multiplexer::Tmux),
            notes: vec!["opt-in Tier 2: use `--only tmux` to mount source-file in tmux.conf".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, env: &EnvContext) -> CapabilityFlags {
        let mut flags = CapabilityFlags::TRUECOLOR | CapabilityFlags::PALETTE_16 | CapabilityFlags::SUPPORTS_INCLUDE;
        if !env.is_ssh && env.os != OsKind::Windows {
            flags |= CapabilityFlags::HOT_RELOAD_SIGNAL;
        }
        flags
    }

    fn plan(&self, ctx: &PlanContext<'_>, detection: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        if !ctx.options.explicit_only {
            return Ok(plan);
        }
        plan.add_generated(ctx.artifacts, artifact_names::TMUX_CONF);
        let gen_path = ctx.paths.generated(artifact_names::TMUX_CONF);
        let Some(target) = detection.config_path.as_ref() else {
            return Ok(plan);
        };
        if !ctx.options.host_writes_allowed(ctx.env) {
            plan.render_only = true;
            plan.diagnostic(
                Diagnostic::info("adapter.tmux.render-only", "host writes suppressed")
                    .with_hint(format!("source-file \"{}\"", gen_path.display())),
            );
            return Ok(plan);
        }
        if !detection.path_kind.is_writable() {
            plan.render_only = true;
            plan.diagnostic(render_only_diagnostic(
                self.id(),
                target,
                detection.path_kind.label(),
                format!("source-file \"{}\"", gen_path.display()),
            ));
            return Ok(plan);
        }
        plan.push(FileOperation::UpsertMarkerBlock {
            target: target.clone(),
            begin: MARKER_BEGIN.to_string(),
            end: MARKER_END.to_string(),
            body: format!("source-file \"{}\"", gen_path.display()),
        });
        Ok(plan)
    }

    fn validate(&self, _p: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        Ok(ValidationReport::ok())
    }

    fn reload(&self, _d: &DetectionReport, caps: CapabilityFlags, platform: &dyn Platform) -> Result<ReloadOutcome> {
        if !caps.contains(CapabilityFlags::HOT_RELOAD_SIGNAL) || platform.which("tmux").is_none() {
            return Ok(ReloadOutcome::NotRunning {
                detail: "tmux reload skipped".to_string(),
            });
        }
        let gen = platform.paths().generated(artifact_names::TMUX_CONF);
        match platform.run("tmux", &["source-file", &gen.to_string_lossy()]) {
            Ok(out) if out.success() => Ok(ReloadOutcome::Signalled {
                detail: "reloaded tmux via `tmux source-file`".to_string(),
            }),
            _ => Ok(ReloadOutcome::NotRunning {
                detail: "no active tmux server found".to_string(),
            }),
        }
    }

    fn plan_unhook(&self, detection: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        if let Some(target) = detection.config_path.as_ref() {
            plan.push(FileOperation::RemoveMarkerBlock {
                target: target.clone(),
                begin: MARKER_BEGIN.to_string(),
                end: MARKER_END.to_string(),
            });
        }
        Ok(plan)
    }
}

// ---------------------------------------------------------------------------
// Zellij
// ---------------------------------------------------------------------------

pub struct ZellijAdapter;

impl ZellijAdapter {
    fn target_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_ZELLIJ_THEME_FILE")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        Some(xdg_config_home(platform)?.join("zellij").join("themes").join("kiseki.kdl"))
    }
}

impl Adapter for ZellijAdapter {
    fn id(&self) -> &'static str { "zellij" }
    fn display_name(&self) -> &'static str { "Zellij" }
    fn kind(&self) -> AdapterKind { AdapterKind::Multiplexer }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = Self::target_path(platform);
        let path_kind = config_path.as_deref().map(|p| platform.inspect(p)).unwrap_or(PathKind::Missing);
        let installed = platform.which("zellij").is_some()
            || path_kind != PathKind::Missing
            || env.multiplexer == Some(Multiplexer::Zellij);
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: env.multiplexer == Some(Multiplexer::Zellij),
            notes: vec!["opt-in Tier 2: set `theme \"kiseki\"` in zellij/config.kdl".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, _e: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::TRUECOLOR | CapabilityFlags::PALETTE_16
    }

    fn plan(&self, ctx: &PlanContext<'_>, detection: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        Ok(plan_drop_in_theme(
            self.id(),
            artifact_names::ZELLIJ_KDL,
            ctx,
            detection,
            "set `theme \"kiseki\"` in ~/.config/zellij/config.kdl",
        ))
    }

    fn validate(&self, _p: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        Ok(ValidationReport::ok())
    }

    fn reload(&self, _d: &DetectionReport, _c: CapabilityFlags, _pl: &dyn Platform) -> Result<ReloadOutcome> {
        Ok(ReloadOutcome::AutoReload {
            detail: "Zellij theme file updated".to_string(),
        })
    }

    fn plan_unhook(&self, detection: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        Ok(plan_drop_in_unhook(self.id(), detection))
    }
}

// ---------------------------------------------------------------------------
// OpenCode
// ---------------------------------------------------------------------------

pub struct OpenCodeAdapter;

impl OpenCodeAdapter {
    fn target_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_OPENCODE_THEME_FILE")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        Some(xdg_config_home(platform)?.join("opencode").join("themes").join("kiseki.json"))
    }
}

impl Adapter for OpenCodeAdapter {
    fn id(&self) -> &'static str { "opencode" }
    fn display_name(&self) -> &'static str { "OpenCode" }
    fn kind(&self) -> AdapterKind { AdapterKind::AgentTui }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = Self::target_path(platform);
        let path_kind = config_path.as_deref().map(|p| platform.inspect(p)).unwrap_or(PathKind::Missing);
        let installed = platform.which("opencode").is_some() || path_kind != PathKind::Missing;
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes: vec!["opt-in Tier 2: select `kiseki` theme in OpenCode".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, _e: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::TRUECOLOR
    }

    fn plan(&self, ctx: &PlanContext<'_>, detection: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        Ok(plan_drop_in_theme(
            self.id(),
            artifact_names::OPENCODE_JSON,
            ctx,
            detection,
            "select `kiseki` theme inside OpenCode",
        ))
    }

    fn validate(&self, plan: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for (path, content) in plan.generated_writes() {
            if path == artifact_names::OPENCODE_JSON && serde_json::from_str::<serde_json::Value>(content).is_err() {
                report.push(Diagnostic::error(
                    "adapter.opencode.invalid-json",
                    "generated opencode.json is not valid JSON",
                ));
            }
        }
        Ok(report)
    }

    fn reload(&self, _d: &DetectionReport, _c: CapabilityFlags, _pl: &dyn Platform) -> Result<ReloadOutcome> {
        Ok(ReloadOutcome::AutoReload {
            detail: "OpenCode theme JSON updated".to_string(),
        })
    }

    fn plan_unhook(&self, detection: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        Ok(plan_drop_in_unhook(self.id(), detection))
    }
}

// ---------------------------------------------------------------------------
// bat
// ---------------------------------------------------------------------------

pub struct BatAdapter;

impl BatAdapter {
    fn target_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_BAT_THEME_FILE")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        if let Some(bat_cfg) = platform.env_var("BAT_CONFIG_DIR").filter(|v| !v.trim().is_empty()) {
            return Some(PathBuf::from(bat_cfg).join("themes").join("Kiseki.tmTheme"));
        }
        Some(appdata_or_xdg(platform)?.join("bat").join("themes").join("Kiseki.tmTheme"))
    }
}

impl Adapter for BatAdapter {
    fn id(&self) -> &'static str { "bat" }
    fn display_name(&self) -> &'static str { "bat" }
    fn kind(&self) -> AdapterKind { AdapterKind::DevTool }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = Self::target_path(platform);
        let path_kind = config_path.as_deref().map(|p| platform.inspect(p)).unwrap_or(PathKind::Missing);
        let installed = platform.which("bat").is_some()
            || platform.which("batcat").is_some()
            || path_kind != PathKind::Missing;
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes: vec!["opt-in Tier 2: use `--only bat` then export BAT_THEME=Kiseki".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, _e: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::TRUECOLOR
    }

    fn plan(&self, ctx: &PlanContext<'_>, detection: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        Ok(plan_drop_in_theme(
            self.id(),
            artifact_names::BAT_TMTHEME,
            ctx,
            detection,
            "run `bat cache --build` and set `BAT_THEME=Kiseki`",
        ))
    }

    fn validate(&self, _p: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        Ok(ValidationReport::ok())
    }

    fn reload(&self, _d: &DetectionReport, _c: CapabilityFlags, platform: &dyn Platform) -> Result<ReloadOutcome> {
        let bin = if platform.which("bat").is_some() {
            Some("bat")
        } else if platform.which("batcat").is_some() {
            Some("batcat")
        } else {
            None
        };
        let Some(bin) = bin else {
            return Ok(ReloadOutcome::Skipped {
                detail: "bat binary not found; run `bat cache --build` after installing bat".to_string(),
            });
        };
        match platform.run(bin, &["cache", "--build"]) {
            Ok(out) if out.success() => Ok(ReloadOutcome::Signalled {
                detail: format!("rebuilt bat theme cache (`{bin} cache --build`)"),
            }),
            Ok(out) => Ok(ReloadOutcome::Failed {
                detail: format!("{bin} cache --build exited with {}: {}", out.status, out.stderr.trim()),
            }),
            Err(err) => Ok(ReloadOutcome::Failed {
                detail: err.to_string(),
            }),
        }
    }

    fn plan_unhook(&self, detection: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        Ok(plan_drop_in_unhook(self.id(), detection))
    }
}

// ---------------------------------------------------------------------------
// delta (pure generated artefact, never edits ~/.gitconfig)
// ---------------------------------------------------------------------------

pub struct DeltaAdapter;

impl Adapter for DeltaAdapter {
    fn id(&self) -> &'static str { "delta" }
    fn display_name(&self) -> &'static str { "git-delta" }
    fn kind(&self) -> AdapterKind { AdapterKind::DevTool }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        Ok(DetectionReport {
            installed: platform.which("delta").is_some(),
            version: None,
            config_path: None,
            path_kind: PathKind::Missing,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes: vec!["opt-in Tier 2: include generated/delta.gitconfig from ~/.gitconfig".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, _e: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::TRUECOLOR
    }

    fn plan(&self, ctx: &PlanContext<'_>, _d: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        if !ctx.options.explicit_only {
            return Ok(plan);
        }
        plan.add_generated(ctx.artifacts, artifact_names::DELTA_GITCONFIG);
        plan.diagnostic(
            Diagnostic::info(
                "adapter.delta.include",
                "generated/delta.gitconfig updated; add [include] to ~/.gitconfig to activate",
            )
            .with_hint(format!(
                "[include]\n  path = {}\n[delta]\n  features = kiseki",
                ctx.paths.generated(artifact_names::DELTA_GITCONFIG).display()
            )),
        );
        Ok(plan)
    }

    fn validate(&self, _p: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        Ok(ValidationReport::ok())
    }

    fn reload(&self, _d: &DetectionReport, _c: CapabilityFlags, _pl: &dyn Platform) -> Result<ReloadOutcome> {
        Ok(ReloadOutcome::AutoReload {
            detail: "git-delta reads generated/delta.gitconfig on next git diff".to_string(),
        })
    }

    fn plan_unhook(&self, _d: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        Ok(AdapterPlan::new(self.id()))
    }
}

// ---------------------------------------------------------------------------
// lazygit (pure generated artefact, never edits user's lazygit config.yml)
// ---------------------------------------------------------------------------

pub struct LazygitAdapter;

impl Adapter for LazygitAdapter {
    fn id(&self) -> &'static str { "lazygit" }
    fn display_name(&self) -> &'static str { "lazygit" }
    fn kind(&self) -> AdapterKind { AdapterKind::DevTool }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        Ok(DetectionReport {
            installed: platform.which("lazygit").is_some(),
            version: None,
            config_path: None,
            path_kind: PathKind::Missing,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes: vec!["opt-in Tier 2: append generated/lazygit.yml to LG_CONFIG_FILE".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, _e: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::TRUECOLOR
    }

    fn plan(&self, ctx: &PlanContext<'_>, _d: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        if !ctx.options.explicit_only {
            return Ok(plan);
        }
        plan.add_generated(ctx.artifacts, artifact_names::LAZYGIT_YML);
        plan.diagnostic(
            Diagnostic::info(
                "adapter.lazygit.include",
                "generated/lazygit.yml updated; include via LG_CONFIG_FILE",
            )
            .with_hint(format!(
                "export LG_CONFIG_FILE=\"$HOME/.config/lazygit/config.yml,{}\"",
                ctx.paths.generated(artifact_names::LAZYGIT_YML).display()
            )),
        );
        Ok(plan)
    }

    fn validate(&self, _p: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        Ok(ValidationReport::ok())
    }

    fn reload(&self, _d: &DetectionReport, _c: CapabilityFlags, _pl: &dyn Platform) -> Result<ReloadOutcome> {
        Ok(ReloadOutcome::AutoReload {
            detail: "lazygit reads generated/lazygit.yml on next launch".to_string(),
        })
    }

    fn plan_unhook(&self, _d: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        Ok(AdapterPlan::new(self.id()))
    }
}

// ---------------------------------------------------------------------------
// btop
// ---------------------------------------------------------------------------

pub struct BtopAdapter;

impl BtopAdapter {
    fn target_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_BTOP_THEME_FILE")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        Some(xdg_config_home(platform)?.join("btop").join("themes").join("kiseki.theme"))
    }
}

impl Adapter for BtopAdapter {
    fn id(&self) -> &'static str { "btop" }
    fn display_name(&self) -> &'static str { "btop" }
    fn kind(&self) -> AdapterKind { AdapterKind::SystemMonitor }
    fn tier(&self) -> IntegrationTier { IntegrationTier::Tier2OptInArtifact }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = Self::target_path(platform);
        let path_kind = config_path.as_deref().map(|p| platform.inspect(p)).unwrap_or(PathKind::Missing);
        let installed = platform.which("btop").is_some() || path_kind != PathKind::Missing;
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes: vec!["opt-in Tier 2: select `kiseki` theme in btop options".to_string()],
        })
    }

    fn capabilities(&self, _d: &DetectionReport, _e: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::TRUECOLOR
    }

    fn plan(&self, ctx: &PlanContext<'_>, detection: &DetectionReport, _c: CapabilityFlags) -> Result<AdapterPlan> {
        Ok(plan_drop_in_theme(
            self.id(),
            artifact_names::BTOP_THEME,
            ctx,
            detection,
            "set `color_theme = \"kiseki\"` in ~/.config/btop/btop.conf",
        ))
    }

    fn validate(&self, _p: &AdapterPlan, _pl: &dyn Platform) -> Result<ValidationReport> {
        Ok(ValidationReport::ok())
    }

    fn reload(&self, _d: &DetectionReport, _c: CapabilityFlags, _pl: &dyn Platform) -> Result<ReloadOutcome> {
        Ok(ReloadOutcome::AutoReload {
            detail: "btop theme updated".to_string(),
        })
    }

    fn plan_unhook(&self, detection: &DetectionReport, _p: &kiseki_platform::KisekiPaths) -> Result<AdapterPlan> {
        Ok(plan_drop_in_unhook(self.id(), detection))
    }
}
