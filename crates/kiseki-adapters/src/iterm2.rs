//! iTerm2 adapter with Parent Profile inheritance (RFC-0003 §5.2).
//!
//! Writes a pure visual Dynamic Profile to
//! `~/Library/Application Support/iTerm2/DynamicProfiles/kiseki-terminal.json`
//! inheriting from the user's original default profile via
//! `"Dynamic Profile Parent Name"` so hotkeys, triggers, scrollback and working
//! directory are never lost.

use std::path::PathBuf;

use kiseki_compiler::iterm2::{lint, with_parent_name, DYNAMIC_PROFILE_NAME};
use kiseki_platform::{PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, OsKind, Result};

use crate::{
    render_only_diagnostic, Adapter, AdapterKind, AdapterPlan, DetectionReport, FileOperation,
    IntegrationTier, PlanContext, ValidationReport,
};

const PARENT_NOTE_PREFIX: &str = "parent-profile: ";

/// iTerm2 terminal adapter (macOS).
pub struct ITerm2Adapter;

impl ITerm2Adapter {
    fn dynamic_profile_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform
            .env_var("KISEKI_ITERM2_DYNAMIC_PROFILE")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(PathBuf::from(explicit));
        }
        if platform.os() != OsKind::MacOs {
            return None;
        }
        let home = platform.env_var("HOME").map(PathBuf::from)?;
        Some(
            home.join("Library")
                .join("Application Support")
                .join("iTerm2")
                .join("DynamicProfiles")
                .join("kiseki-terminal.json"),
        )
    }

    fn detect_parent_profile(platform: &dyn Platform) -> Option<String> {
        if let Some(explicit) = platform
            .env_var("KISEKI_ITERM2_PARENT_PROFILE")
            .filter(|v| !v.trim().is_empty())
        {
            return Some(explicit);
        }
        if platform.os() != OsKind::MacOs {
            return None;
        }
        let home = platform.env_var("HOME").map(PathBuf::from)?;
        let plist = home
            .join("Library")
            .join("Preferences")
            .join("com.googlecode.iterm2.plist");
        if !platform.exists(&plist) {
            return None;
        }
        let output = platform
            .run("plutil", &["-convert", "json", "-o", "-", &plist.to_string_lossy()])
            .ok()?;
        if !output.success() {
            return None;
        }
        let parsed: serde_json::Value = serde_json::from_str(&output.stdout).ok()?;
        let default_guid = parsed.get("Default Bookmark Guid").and_then(|v| v.as_str());
        let bookmarks = parsed.get("New Bookmarks")?.as_array()?;
        let found = default_guid
            .and_then(|guid| {
                bookmarks
                    .iter()
                    .find(|entry| entry.get("Guid").and_then(|v| v.as_str()) == Some(guid))
            })
            .or_else(|| bookmarks.first())?;
        let name = found.get("Name")?.as_str()?.trim();
        if name.is_empty() || name == DYNAMIC_PROFILE_NAME {
            None
        } else {
            Some(name.to_string())
        }
    }

    fn app_installed(platform: &dyn Platform) -> bool {
        if platform.exists(&PathBuf::from("/Applications/iTerm.app")) {
            return true;
        }
        if let Some(home) = platform.env_var("HOME") {
            let user_app = PathBuf::from(&home).join("Applications/iTerm.app");
            let app_support = PathBuf::from(&home).join("Library/Application Support/iTerm2");
            if platform.exists(&user_app) || platform.exists(&app_support) {
                return true;
            }
        }
        false
    }
}

impl Adapter for ITerm2Adapter {
    fn id(&self) -> &'static str {
        "iterm2"
    }

    fn display_name(&self) -> &'static str {
        "iTerm2"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::Terminal
    }

    fn tier(&self) -> IntegrationTier {
        IntegrationTier::Tier1Managed
    }

    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        if platform.os() != OsKind::MacOs {
            return Ok(DetectionReport::not_installed("iTerm2 is macOS-only"));
        }
        let config_path = Self::dynamic_profile_path(platform);
        let path_kind = config_path
            .as_deref()
            .map(|path| platform.inspect(path))
            .unwrap_or(PathKind::Missing);
        let installed = Self::app_installed(platform)
            || path_kind != PathKind::Missing
            || env.terminal_is("iterm2");

        let mut notes = Vec::new();
        if installed {
            if let Some(parent) = Self::detect_parent_profile(platform) {
                notes.push(format!("{PARENT_NOTE_PREFIX}{parent}"));
            }
        }
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: env.terminal_is("iterm2"),
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
            | CapabilityFlags::HOT_RELOAD_WATCH
            | CapabilityFlags::SESSION_LIVE_OSC
            | CapabilityFlags::PARENT_INHERITANCE;
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
        let parent = detection
            .notes
            .iter()
            .find_map(|note| note.strip_prefix(PARENT_NOTE_PREFIX));

        let raw_profile = ctx
            .artifacts
            .get(kiseki_compiler::artifact_names::ITERM2_DYNAMIC_PROFILE)
            .unwrap_or_default();
        let profile_content = match parent {
            Some(parent_name) if !raw_profile.is_empty() => {
                with_parent_name(raw_profile, parent_name)?
            }
            _ => raw_profile.to_string(),
        };
        if !profile_content.is_empty() {
            if let Ok(rel) = kiseki_compiler::ArtifactPath::new(
                kiseki_compiler::artifact_names::ITERM2_DYNAMIC_PROFILE,
            ) {
                plan.push(FileOperation::WriteGenerated {
                    relative: rel,
                    content: profile_content.clone(),
                });
            }
        }

        let Some(target) = detection.config_path.as_ref() else {
            return Ok(plan);
        };
        if !detection.installed {
            return Ok(plan);
        }
        if !ctx.options.host_writes_allowed(ctx.env) {
            plan.render_only = true;
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.iterm2.render-only",
                    "host writes suppressed; copy generated/iterm2-dynamic-profile.json into DynamicProfiles",
                )
                .with_hint(target.display().to_string()),
            );
            return Ok(plan);
        }
        if !detection.path_kind.is_writable() {
            plan.render_only = true;
            plan.diagnostic(render_only_diagnostic(
                self.id(),
                target,
                detection.path_kind.label(),
                format!("link generated/iterm2-dynamic-profile.json to {}", target.display()),
            ));
            return Ok(plan);
        }

        plan.push(FileOperation::WriteHostFile {
            target: target.clone(),
            content: profile_content,
        });
        Ok(plan)
    }

    fn validate(&self, plan: &AdapterPlan, _platform: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for diagnostic in plan.diagnostics.iter().filter(|d| d.is_fatal()) {
            report.push(diagnostic.clone());
        }
        for (path, content) in plan.generated_writes() {
            if path == kiseki_compiler::artifact_names::ITERM2_DYNAMIC_PROFILE {
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
                detail: "iTerm2 reload skipped".to_string(),
            });
        }
        Ok(ReloadOutcome::AutoReload {
            detail: format!(
                "iTerm2 Dynamic Profile `{DYNAMIC_PROFILE_NAME}` updated; iTerm2 reloads automatically"
            ),
        })
    }

    fn plan_unhook(
        &self,
        detection: &DetectionReport,
        _paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        if let Some(target) = detection.config_path.as_ref() {
            plan.push(FileOperation::RemoveHostFile {
                target: target.clone(),
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
    fn injects_parent_profile_name_into_dynamic_profile() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        platform.set_os(OsKind::MacOs);
        let dyn_path = dir.path().join("DynamicProfiles/kiseki-terminal.json");
        platform.set_env("KISEKI_ITERM2_DYNAMIC_PROFILE", &dyn_path.to_string_lossy());
        platform.set_env("KISEKI_ITERM2_PARENT_PROFILE", "My Work Profile");

        let env = EnvContext {
            os: OsKind::MacOs,
            current_terminal: Some("iterm2".to_string()),
            ..EnvContext::local()
        };
        let detection = ITerm2Adapter.detect(&env, &platform).unwrap();
        assert!(detection.installed);

        let theme = theme();
        let compiled = compile(&CompileInput::new(
            &theme,
            CompileOptions::new(platform.paths().config_dir(), "/themes/nerv-eva"),
        ))
        .unwrap();
        let plan = ITerm2Adapter
            .plan(
                &PlanContext {
                    theme: &theme,
                    artifacts: &compiled.artifacts,
                    options: &PlanOptions::default(),
                    env: &env,
                    paths: platform.paths(),
                },
                &detection,
                ITerm2Adapter.capabilities(&detection, &env),
            )
            .unwrap();
        let host_json = plan
            .operations
            .iter()
            .find_map(|op| match op {
                FileOperation::WriteHostFile { content, .. } => Some(content),
                _ => None,
            })
            .expect("host write");
        assert!(host_json.contains("\"Dynamic Profile Parent Name\": \"My Work Profile\""));
        assert!(!host_json.contains("Silence Bell"));
    }
}
