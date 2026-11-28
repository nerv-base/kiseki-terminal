//! Kitty terminal adapter (RFC-0003 §4).
//!
//! Integration is a single `include <generated/kitty.conf>` marker block in
//! `kitty.conf`. Reload delivers `SIGUSR1` (or `kitty @ set-colors` when remote
//! control is active).

use std::path::PathBuf;

use kiseki_compiler::marker::{MARKER_BEGIN, MARKER_END};
use kiseki_platform::{PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, OsKind, Result};

use crate::{
    render_only_diagnostic, Adapter, AdapterKind, AdapterPlan, DetectionReport, FileOperation,
    IntegrationTier, PlanContext, ValidationReport,
};

/// Kitty terminal adapter.
pub struct KittyAdapter;

impl KittyAdapter {
    fn config_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_KITTY_CONFIG")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        if platform.os() == OsKind::Windows {
            return None;
        }
        let home = platform.env_var("HOME").map(PathBuf::from)?;
        let config_home = platform
            .env_var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        Some(config_home.join("kitty").join("kitty.conf"))
    }

    fn find_binary(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(bin) = platform.which("kitty") {
            return Some(bin);
        }
        if platform.os() == OsKind::MacOs {
            for candidate in [
                PathBuf::from("/Applications/kitty.app/Contents/MacOS/kitty"),
                platform
                    .env_var("HOME")
                    .map(|h| PathBuf::from(h).join("Applications/kitty.app/Contents/MacOS/kitty"))
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

impl Adapter for KittyAdapter {
    fn id(&self) -> &'static str {
        "kitty"
    }

    fn display_name(&self) -> &'static str {
        "Kitty"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::Terminal
    }

    fn tier(&self) -> IntegrationTier {
        IntegrationTier::Tier1Managed
    }

    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        if platform.os() == OsKind::Windows {
            return Ok(DetectionReport::not_installed("Kitty is not supported on Windows"));
        }
        let config_path = Self::config_path(platform);
        let path_kind = config_path
            .as_deref()
            .map(|path| platform.inspect(path))
            .unwrap_or(PathKind::Missing);
        let binary = Self::find_binary(platform);
        let installed = binary.is_some()
            || path_kind != PathKind::Missing
            || env.terminal_is("kitty");
        let mut notes = Vec::new();
        if installed && path_kind == PathKind::Missing {
            notes.push("no kitty.conf found; generated kitty.conf is available for manual include".to_string());
        }
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: env.terminal_is("kitty"),
            notes,
        })
    }

    fn capabilities(&self, _detection: &DetectionReport, env: &EnvContext) -> CapabilityFlags {
        let mut flags = CapabilityFlags::PALETTE_16
            | CapabilityFlags::TRUECOLOR
            | CapabilityFlags::WINDOW_OPACITY
            | CapabilityFlags::WINDOW_BLUR
            | CapabilityFlags::WALLPAPER_STATIC
            | CapabilityFlags::FONT_SINGLE_ONLY
            | CapabilityFlags::UI_CHROME
            | CapabilityFlags::HOT_RELOAD_SIGNAL
            | CapabilityFlags::SESSION_LIVE_OSC
            | CapabilityFlags::SUPPORTS_INCLUDE;
        if env.is_ssh {
            flags.remove(CapabilityFlags::HOT_RELOAD_SIGNAL);
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
        plan.add_generated(ctx.artifacts, kiseki_compiler::artifact_names::KITTY_CONF);
        let generated_path = ctx.paths.generated(kiseki_compiler::artifact_names::KITTY_CONF);

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
                    "adapter.kitty.render-only",
                    "host writes suppressed; add the include manually if needed",
                )
                .with_hint(format!("include {}", generated_path.display())),
            );
            return Ok(plan);
        }
        if detection.path_kind == PathKind::Missing && !ctx.options.allow_create_host_files {
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.kitty.no-config",
                    "no kitty.conf to anchor; generated config left in place",
                )
                .with_hint(format!("add `include {}` to kitty.conf", generated_path.display())),
            );
            return Ok(plan);
        }
        if !detection.path_kind.is_writable() {
            plan.render_only = true;
            plan.diagnostic(render_only_diagnostic(
                self.id(),
                config_path,
                detection.path_kind.label(),
                format!("add `include {}` to your Kitty config", generated_path.display()),
            ));
            return Ok(plan);
        }

        let include = kiseki_compiler::kitty::include_line(&generated_path)?;
        plan.push(FileOperation::UpsertMarkerBlock {
            target: config_path.clone(),
            begin: MARKER_BEGIN.to_string(),
            end: MARKER_END.to_string(),
            body: include,
        });
        Ok(plan)
    }

    fn validate(&self, plan: &AdapterPlan, _platform: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for diagnostic in plan.diagnostics.iter().filter(|d| d.is_fatal()) {
            report.push(diagnostic.clone());
        }
        for (path, content) in plan.generated_writes() {
            if path == kiseki_compiler::artifact_names::KITTY_CONF {
                for diagnostic in kiseki_compiler::kitty::lint(content) {
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
        platform: &dyn Platform,
    ) -> Result<ReloadOutcome> {
        if !caps.contains(CapabilityFlags::HOT_RELOAD_SIGNAL) {
            return Ok(ReloadOutcome::NotRunning {
                detail: "reload skipped for this session".to_string(),
            });
        }
        if !detection.installed && !detection.active_in_session {
            return Ok(ReloadOutcome::NotRunning {
                detail: "Kitty is not installed".to_string(),
            });
        }
        if platform.os() == OsKind::Linux && platform.which("pkill").is_none() {
            return Ok(ReloadOutcome::NotRunning {
                detail: "pkill is unavailable, so no reload signal was sent".to_string(),
            });
        }
        let args: &[&str] = if platform.os() == OsKind::MacOs {
            &["-USR1", "-a", "-x", "kitty"]
        } else {
            &["-USR1", "-x", "kitty"]
        };
        match platform.run("pkill", args) {
            Ok(output) if output.success() => Ok(ReloadOutcome::Signalled {
                detail: "sent SIGUSR1 to kitty".to_string(),
            }),
            Ok(output) if output.status == 1 => Ok(ReloadOutcome::NotRunning {
                detail: "no running kitty process found".to_string(),
            }),
            Ok(output) => Ok(ReloadOutcome::Failed {
                detail: format!("pkill exited with {}: {}", output.status, output.stderr.trim()),
            }),
            Err(error) => Ok(ReloadOutcome::Failed {
                detail: error.to_string(),
            }),
        }
    }

    fn plan_unhook(
        &self,
        detection: &DetectionReport,
        _paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        if let Some(config_path) = detection.config_path.as_ref() {
            plan.push(FileOperation::RemoveMarkerBlock {
                target: config_path.clone(),
                begin: MARKER_BEGIN.to_string(),
                end: MARKER_END.to_string(),
            });
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlanOptions;
    use kiseki_compiler::{compile, CompileInput, CompileOptions};
    use kiseki_platform::{CommandOutput, MockPlatform};
    use kiseki_schema::{ResolvedTheme, ThemePack};
    use kiseki_types::RenderingProfile;

    fn theme() -> ResolvedTheme {
        let pack = ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json"))
                .unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json"))
                .unwrap(),
        };
        ResolvedTheme::from_pack(&pack, RenderingProfile::Static)
    }

    #[test]
    fn plans_include_anchor_and_reloads_with_sigusr1() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        platform.set_os(OsKind::Linux);
        let conf = dir.path().join("kitty.conf");
        platform.write_preserving_symlinks(&conf, b"font_size 12\n").unwrap();
        platform.set_env("KISEKI_KITTY_CONFIG", &conf.to_string_lossy());
        platform.set_env("KISEKI_WHICH_pkill", "/usr/bin/pkill");
        platform.set_command_output("pkill", CommandOutput::new(0, "", ""));

        let env = EnvContext::local();
        let detection = KittyAdapter.detect(&env, &platform).unwrap();
        assert!(detection.installed);

        let theme = theme();
        let compiled = compile(&CompileInput::new(
            &theme,
            CompileOptions::new(platform.paths().config_dir(), "/themes/nerv-eva"),
        ))
        .unwrap();
        let caps = KittyAdapter.capabilities(&detection, &env);
        let plan = KittyAdapter
            .plan(
                &PlanContext {
                    theme: &theme,
                    artifacts: &compiled.artifacts,
                    options: &PlanOptions::default(),
                    env: &env,
                    paths: platform.paths(),
                },
                &detection,
                caps,
            )
            .unwrap();
        assert!(plan.has_generated("kitty.conf"));
        assert_eq!(plan.host_writes().count(), 1);
        assert!(matches!(
            KittyAdapter.reload(&detection, caps, &platform).unwrap(),
            ReloadOutcome::Signalled { .. }
        ));
    }
}
