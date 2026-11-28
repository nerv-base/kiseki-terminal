//! WezTerm terminal adapter (RFC-0003 §4).
//!
//! Writes `generated/wezterm.lua` and anchors a safe `pcall(dofile, ...)` block
//! before `return <ident>` in the user's `wezterm.lua`. If the user's Lua
//! structure cannot be safely located (for example `return { ... }`), degrades
//! honestly to `render_only` with a copyable 2-line manual hook instead of
//! corrupting the file or lying about success.

use std::path::PathBuf;

use kiseki_compiler::wezterm::{
    can_patch_wezterm_lua, lint, manual_hint, LUA_MARKER_BEGIN, LUA_MARKER_END,
};
use kiseki_platform::{PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, OsKind, Result};

use crate::{
    render_only_diagnostic, Adapter, AdapterKind, AdapterPlan, DetectionReport, FileOperation,
    IntegrationTier, PlanContext, ValidationReport,
};

const ACTION_REQUIRED_NOTE: &str =
    "wezterm.lua does not end with a simple `return <ident>`; manual pcall(dofile) hook required";

/// WezTerm terminal adapter.
pub struct WezTermAdapter;

impl WezTermAdapter {
    fn config_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_WEZTERM_CONFIG")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        let home = platform
            .env_var("HOME")
            .or_else(|| platform.env_var("USERPROFILE"))
            .map(PathBuf::from)?;
        let config_home = platform
            .env_var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let xdg = config_home.join("wezterm").join("wezterm.lua");
        let dot = home.join(".wezterm.lua");
        if platform.exists(&dot) {
            Some(dot)
        } else if platform.exists(&xdg) {
            Some(xdg)
        } else {
            Some(dot)
        }
    }

    fn find_binary(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(bin) = platform.which("wezterm") {
            return Some(bin);
        }
        if platform.os() == OsKind::MacOs {
            for candidate in [
                PathBuf::from("/Applications/WezTerm.app/Contents/MacOS/wezterm"),
                platform
                    .env_var("HOME")
                    .map(|h| {
                        PathBuf::from(h).join("Applications/WezTerm.app/Contents/MacOS/wezterm")
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

impl Adapter for WezTermAdapter {
    fn id(&self) -> &'static str {
        "wezterm"
    }

    fn display_name(&self) -> &'static str {
        "WezTerm"
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
            || env.terminal_is("wezterm");

        let mut notes = Vec::new();
        if installed && path_kind == PathKind::Missing {
            notes.push("no wezterm.lua found; generated wezterm.lua is ready for dofile()".to_string());
        } else if let Some(path) = config_path.as_deref() {
            if path_kind != PathKind::Missing {
                if let Ok(Some(contents)) = platform.read_to_string(path) {
                    if !can_patch_wezterm_lua(&contents) {
                        notes.push(ACTION_REQUIRED_NOTE.to_string());
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
            active_in_session: env.terminal_is("wezterm"),
            notes,
        })
    }

    fn capabilities(&self, _detection: &DetectionReport, env: &EnvContext) -> CapabilityFlags {
        let mut flags = CapabilityFlags::PALETTE_16
            | CapabilityFlags::TRUECOLOR
            | CapabilityFlags::WINDOW_OPACITY
            | CapabilityFlags::WINDOW_BLUR
            | CapabilityFlags::WALLPAPER_STATIC
            | CapabilityFlags::FONT_STACK_FALLBACK
            | CapabilityFlags::UI_CHROME
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
        plan.add_generated(ctx.artifacts, kiseki_compiler::artifact_names::WEZTERM_LUA);
        let generated_path = ctx.paths.generated(kiseki_compiler::artifact_names::WEZTERM_LUA);

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
                    "adapter.wezterm.render-only",
                    "host writes suppressed; load generated/wezterm.lua manually",
                )
                .with_hint(manual_hint(&generated_path)),
            );
            return Ok(plan);
        }
        if detection.path_kind == PathKind::Missing && !ctx.options.allow_create_host_files {
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.wezterm.no-config",
                    "no wezterm.lua found; generated wezterm.lua left in place",
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
        if detection.notes.iter().any(|note| note == ACTION_REQUIRED_NOTE) {
            plan.render_only = true;
            plan.diagnostic(
                Diagnostic::warning(
                    "adapter.wezterm.action-required",
                    format!(
                        "{} uses a custom Lua return layout; skipped auto-patching to avoid breaking it",
                        config_path.display()
                    ),
                )
                .with_hint(manual_hint(&generated_path)),
            );
            return Ok(plan);
        }

        plan.push(FileOperation::PatchWezTermLua {
            target: config_path.clone(),
            begin: LUA_MARKER_BEGIN.to_string(),
            end: LUA_MARKER_END.to_string(),
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
            if path == kiseki_compiler::artifact_names::WEZTERM_LUA {
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
                detail: "WezTerm reload skipped".to_string(),
            });
        }
        Ok(ReloadOutcome::AutoReload {
            detail: "WezTerm watches wezterm.lua / loaded files and reloads automatically".to_string(),
        })
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
                begin: LUA_MARKER_BEGIN.to_string(),
                end: LUA_MARKER_END.to_string(),
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
    use kiseki_platform::MockPlatform;
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
    fn non_standard_return_table_degrades_honestly_to_render_only() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let lua = dir.path().join(".wezterm.lua");
        platform.write_preserving_symlinks(&lua, b"return { font_size = 13 }\n").unwrap();
        platform.set_env("KISEKI_WEZTERM_CONFIG", &lua.to_string_lossy());

        let env = EnvContext::local();
        let detection = WezTermAdapter.detect(&env, &platform).unwrap();
        assert!(detection.installed);

        let theme = theme();
        let compiled = compile(&CompileInput::new(
            &theme,
            CompileOptions::new(platform.paths().config_dir(), "/themes/nerv-eva"),
        ))
        .unwrap();
        let plan = WezTermAdapter
            .plan(
                &PlanContext {
                    theme: &theme,
                    artifacts: &compiled.artifacts,
                    options: &PlanOptions::default(),
                    env: &env,
                    paths: platform.paths(),
                },
                &detection,
                WezTermAdapter.capabilities(&detection, &env),
            )
            .unwrap();
        assert!(plan.render_only);
        assert_eq!(plan.host_writes().count(), 0);
        assert!(plan
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "adapter.wezterm.action-required"));
    }
}
