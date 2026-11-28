//! Alacritty terminal adapter (RFC-0003 §4).
//!
//! Writes `generated/alacritty.toml` and attaches it via `general.import` in
//! `alacritty.toml` without clobbering user comments or existing imports.

use std::path::PathBuf;

use kiseki_compiler::alacritty::{lint, manual_hint, patch_alacritty_toml};
use kiseki_compiler::marker::{MARKER_BEGIN, MARKER_END};
use kiseki_platform::{PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, OsKind, Result};

use crate::{
    render_only_diagnostic, Adapter, AdapterKind, AdapterPlan, DetectionReport, FileOperation,
    IntegrationTier, PlanContext, ValidationReport,
};

const UNPATCHABLE_NOTE: &str =
    "alacritty.toml has a custom [general] layout; manual general.import entry required";

/// Alacritty terminal adapter.
pub struct AlacrittyAdapter;

impl AlacrittyAdapter {
    fn config_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_ALACRITTY_CONFIG")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        match platform.os() {
            OsKind::Windows => {
                let appdata = platform.env_var("APPDATA").map(PathBuf::from)?;
                Some(appdata.join("alacritty").join("alacritty.toml"))
            }
            OsKind::MacOs | OsKind::Linux => {
                let home = platform.env_var("HOME").map(PathBuf::from)?;
                let config_home = platform
                    .env_var("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".config"));
                Some(config_home.join("alacritty").join("alacritty.toml"))
            }
        }
    }

    fn find_binary(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(bin) = platform.which("alacritty") {
            return Some(bin);
        }
        if platform.os() == OsKind::MacOs {
            for candidate in [
                PathBuf::from("/Applications/Alacritty.app/Contents/MacOS/alacritty"),
                platform
                    .env_var("HOME")
                    .map(|h| {
                        PathBuf::from(h).join("Applications/Alacritty.app/Contents/MacOS/alacritty")
                    })
                    .unwrap_or_default(),
            ] {
                if !candidate.as_os_str().is_empty() && platform.exists(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

impl Adapter for AlacrittyAdapter {
    fn id(&self) -> &'static str {
        "alacritty"
    }

    fn display_name(&self) -> &'static str {
        "Alacritty"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::Terminal
    }

    fn tier(&self) -> IntegrationTier {
        IntegrationTier::Tier1Managed
    }

    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = Self::config_path(platform);
        let path_kind = config_path
            .as_deref()
            .map(|path| platform.inspect(path))
            .unwrap_or(PathKind::Missing);
        let installed = Self::find_binary(platform).is_some()
            || path_kind != PathKind::Missing
            || env.terminal_is("alacritty");
        let mut notes = Vec::new();
        if installed && path_kind == PathKind::Missing {
            notes.push("no alacritty.toml found; generated alacritty.toml is ready for general.import".to_string());
        } else if let Some(path) = config_path.as_deref() {
            if path_kind != PathKind::Missing {
                if let Ok(Some(contents)) = platform.read_to_string(path) {
                    let probe = platform
                        .paths()
                        .generated(kiseki_compiler::artifact_names::ALACRITTY_TOML);
                    if patch_alacritty_toml(&contents, MARKER_BEGIN, MARKER_END, &probe).is_err() {
                        notes.push(UNPATCHABLE_NOTE.to_string());
                    }
                }
            }
        }
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: env.terminal_is("alacritty"),
            notes,
        })
    }

    fn capabilities(&self, _detection: &DetectionReport, env: &EnvContext) -> CapabilityFlags {
        let mut flags = CapabilityFlags::PALETTE_16
            | CapabilityFlags::TRUECOLOR
            | CapabilityFlags::WINDOW_OPACITY
            | CapabilityFlags::WINDOW_BLUR
            | CapabilityFlags::FONT_SINGLE_ONLY
            | CapabilityFlags::HOT_RELOAD_WATCH
            | CapabilityFlags::SUPPORTS_INCLUDE;
        if env.is_ssh {
            flags.remove(CapabilityFlags::HOT_RELOAD_WATCH);
        }
        flags
    }

    fn plan(
        &self,
        ctx: &PlanContext<'_>,
        detection: &DetectionReport,
        _caps: CapabilityFlags,
    ) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        plan.add_generated(ctx.artifacts, kiseki_compiler::artifact_names::ALACRITTY_TOML);
        let generated_path = ctx.paths.generated(kiseki_compiler::artifact_names::ALACRITTY_TOML);

        let Some(config_path) = detection.config_path.as_ref() else {
            return Ok(plan);
        };
        if !detection.installed {
            return Ok(plan);
        }
        if !ctx.options.host_writes_allowed(ctx.env) {
            plan.render_only = true;
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.alacritty.render-only",
                    "host writes suppressed; import generated/alacritty.toml manually",
                )
                .with_hint(manual_hint(&generated_path)),
            );
            return Ok(plan);
        }
        if detection.path_kind == PathKind::Missing && !ctx.options.allow_create_host_files {
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.alacritty.no-config",
                    "no alacritty.toml found; generated config left in place",
                )
                .with_hint(manual_hint(&generated_path)),
            );
            return Ok(plan);
        }
        if !detection.path_kind.is_writable() {
            plan.render_only = true;
            plan.diagnostic(render_only_diagnostic(
                self.id(),
                config_path,
                detection.path_kind.label(),
                manual_hint(&generated_path),
            ));
            return Ok(plan);
        }
        if detection.notes.iter().any(|note| note == UNPATCHABLE_NOTE) {
            plan.render_only = true;
            plan.diagnostic(
                Diagnostic::warning(
                    "adapter.alacritty.action-required",
                    format!(
                        "{} uses a custom TOML layout; skipped auto-patching",
                        config_path.display()
                    ),
                )
                .with_hint(manual_hint(&generated_path)),
            );
            return Ok(plan);
        }

        plan.push(FileOperation::PatchAlacrittyToml {
            target: config_path.clone(),
            begin: MARKER_BEGIN.to_string(),
            end: MARKER_END.to_string(),
            generated_path,
        });
        Ok(plan)
    }

    fn validate(&self, plan: &AdapterPlan, _platform: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for diagnostic in plan.diagnostics.iter().filter(|d| d.is_fatal()) {
            report.push(diagnostic.clone());
        }
        for (path, content) in plan.generated_writes() {
            if path == kiseki_compiler::artifact_names::ALACRITTY_TOML {
                for diagnostic in lint(content) {
                    report.push(diagnostic);
                }
            }
        }
        Ok(report)
    }

    fn reload(
        &self,
        detection: &DetectionReport,
        caps: CapabilityFlags,
        _platform: &dyn Platform,
    ) -> Result<ReloadOutcome> {
        if !caps.contains(CapabilityFlags::HOT_RELOAD_WATCH) || !detection.installed {
            return Ok(ReloadOutcome::NotRunning {
                detail: "Alacritty reload skipped".to_string(),
            });
        }
        Ok(ReloadOutcome::AutoReload {
            detail: "Alacritty live_config_reload applies imported theme automatically".to_string(),
        })
    }

    fn plan_unhook(
        &self,
        detection: &DetectionReport,
        paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        if let Some(config_path) = detection.config_path.as_ref() {
            plan.push(FileOperation::UnhookAlacrittyToml {
                target: config_path.clone(),
                begin: MARKER_BEGIN.to_string(),
                end: MARKER_END.to_string(),
                generated_path: paths.generated(kiseki_compiler::artifact_names::ALACRITTY_TOML),
            });
        }
        Ok(plan)
    }
}
