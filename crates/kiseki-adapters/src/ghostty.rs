//! Ghostty adapter (RFC-0003 §4).
//!
//! Integration is one optional `config-file = ?...` line. The generated file
//! owns colours, font, opacity, wallpaper and `custom-shader`; Ghostty's shell
//! integration, keybindings and commands are never touched.

use kiseki_compiler::marker::{MARKER_BEGIN, MARKER_END};
use kiseki_platform::{ghostty, PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, Result};

use crate::{
    render_only_diagnostic, Adapter, AdapterKind, AdapterPlan, DetectionReport, FileOperation,
    IntegrationTier, PlanContext, ValidationReport,
};

/// Ghostty terminal adapter.
pub struct GhosttyAdapter;

impl Adapter for GhosttyAdapter {
    fn id(&self) -> &'static str {
        "ghostty"
    }

    fn display_name(&self) -> &'static str {
        "Ghostty"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::Terminal
    }

    fn tier(&self) -> IntegrationTier {
        IntegrationTier::Tier1Managed
    }

    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = ghostty::default_config_path(platform);
        let path_kind = config_path
            .as_deref()
            .map(|path| platform.inspect(path))
            .unwrap_or(PathKind::Missing);
        let binary = ghostty::find_binary(platform);
        let installed = binary.is_some()
            || path_kind != PathKind::Missing
            || env.terminal_is("ghostty");
        let mut notes = Vec::new();
        if binary.is_none() {
            notes.push("ghostty binary not found on PATH or /Applications/Ghostty.app; generated config is not validated by the host".to_string());
        }
        if path_kind == PathKind::Missing {
            notes.push("no Ghostty config file found; generated config is available for manual include".to_string());
        }
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            active_in_session: env.terminal_is("ghostty"),
            notes,
        })
    }

    fn capabilities(&self, _detection: &DetectionReport, env: &EnvContext) -> CapabilityFlags {
        let mut flags = CapabilityFlags::PALETTE_16
            | CapabilityFlags::TRUECOLOR
            | CapabilityFlags::WINDOW_OPACITY
            | CapabilityFlags::WINDOW_BLUR
            | CapabilityFlags::WALLPAPER_STATIC
            | CapabilityFlags::WALLPAPER_ALIGNMENT
            | CapabilityFlags::FONT_STACK_FALLBACK
            | CapabilityFlags::UI_CHROME
            | CapabilityFlags::SHADER_POSTPROCESS
            | CapabilityFlags::HOT_RELOAD_SIGNAL
            | CapabilityFlags::SUPPORTS_INCLUDE;
        if env.is_ssh {
            // A local reload cannot help a remote session.
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
        plan.add_generated(ctx.artifacts, kiseki_compiler::artifact_names::GHOSTTY_CONF);

        let host_writes = ctx.options.host_writes_allowed(ctx.env);
        let Some(config_path) = detection.config_path.as_ref() else {
            return Ok(plan);
        };

        if !host_writes {
            plan.render_only = true;
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.ghostty.render-only",
                    "host writes suppressed; add the include manually if needed",
                )
                .with_hint(format!(
                    "config-file = ?{}",
                    ctx.paths
                        .config_dir()
                        .join("generated")
                        .join("ghostty.conf")
                        .display()
                )),
            );
            return Ok(plan);
        }

        if detection.path_kind == PathKind::Missing {
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.ghostty.no-config",
                    "no Ghostty config to anchor; generated config left in place",
                )
                .with_hint(format!(
                    "add `config-file = ?{}` to your Ghostty config",
                    ctx.paths
                        .config_dir()
                        .join("generated")
                        .join("ghostty.conf")
                        .display()
                )),
            );
            return Ok(plan);
        }

        if !detection.path_kind.is_writable() {
            plan.render_only = true;
            plan.diagnostic(render_only_diagnostic(
                self.id(),
                config_path,
                detection.path_kind.label(),
                format!(
                    "add `config-file = ?{}` to your Nix/Home Manager Ghostty config",
                    ctx.paths
                        .config_dir()
                        .join("generated")
                        .join("ghostty.conf")
                        .display()
                ),
            ));
            return Ok(plan);
        }

        let include = ghostty::include_line(
            &ctx.paths
                .config_dir()
                .join("generated")
                .join("ghostty.conf"),
        )?;
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
        for diagnostic in plan.diagnostics.iter().filter(|diagnostic| diagnostic.is_fatal()) {
            report.push(diagnostic.clone());
        }
        for (path, content) in plan.generated_writes() {
            if path.ends_with(kiseki_compiler::artifact_names::GHOSTTY_CONF) {
                for diagnostic in kiseki_compiler::ghostty::lint(content) {
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
                detail: "reload skipped for this session (SSH or unsupported host)".to_string(),
            });
        }
        if !detection.active_in_session && !detection.installed {
            return Ok(ReloadOutcome::NotRunning {
                detail: "Ghostty is not installed".to_string(),
            });
        }
        Ok(ghostty::reload(platform))
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
    use kiseki_platform::MockPlatform;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_types::RenderingProfile;
    use std::path::PathBuf;

    const GLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.glsl");
    const HLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.hlsl");

    fn theme() -> ResolvedTheme {
        let pack = ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
        };
        ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced)
    }

    fn compiled(config_dir: &std::path::Path) -> kiseki_compiler::CompileOutput {
        let theme = theme();
        let options = CompileOptions::new(config_dir, "/data/themes/nerv-eva");
        compile(
            &CompileInput::new(&theme, options)
                .with_glsl(Some(GLSL))
                .with_hlsl(Some(HLSL)),
        )
        .unwrap()
    }

    fn platform() -> (tempfile::TempDir, MockPlatform) {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        (dir, platform)
    }

    #[test]
    fn detect_reports_missing_config_and_binary() {
        let (_dir, platform) = platform();
        let adapter = GhosttyAdapter;
        let detection = adapter.detect(&EnvContext::local(), &platform).unwrap();
        assert!(!detection.installed);
        assert_eq!(detection.path_kind, PathKind::Missing);
        assert!(detection.notes.iter().any(|note| note.contains("no Ghostty config")));
    }

    #[test]
    fn detect_finds_the_config_and_marks_the_session_active() {
        let (dir, platform) = platform();
        let config = dir.path().join("ghostty").join("config");
        platform.create_dir_all(config.parent().unwrap()).unwrap();
        platform.write_preserving_symlinks(&config, b"font-size = 12\n").unwrap();
        platform.set_env("KISEKI_GHOSTTY_CONFIG", &config.to_string_lossy());
        platform.set_env("KISEKI_TERMINAL", "ghostty");
        let env = EnvContext {
            current_terminal: Some("ghostty".to_string()),
            ..EnvContext::local()
        };
        let detection = GhosttyAdapter.detect(&env, &platform).unwrap();
        assert!(detection.installed);
        assert!(detection.active_in_session);
        assert_eq!(detection.config_path.as_deref(), Some(config.as_path()));
        assert_eq!(detection.path_kind, PathKind::Regular);
    }

    #[test]
    fn plan_writes_generated_and_anchors_one_include_line() {
        let (dir, platform) = platform();
        let config = dir.path().join("ghostty").join("config");
        platform.create_dir_all(config.parent().unwrap()).unwrap();
        platform.write_preserving_symlinks(&config, b"# user config\nfont-size = 12\n").unwrap();
        let detection = DetectionReport {
            installed: true,
            version: None,
            config_path: Some(config.clone()),
            path_kind: PathKind::Regular,
            active_in_session: true,
            notes: Vec::new(),
        };
        let theme = theme();
        let output = compiled(platform.paths().config_dir());
        let options = PlanOptions::default();
        let env = EnvContext::local();
        let plan = GhosttyAdapter
            .plan(
                &PlanContext {
                    theme: &theme,
                    artifacts: &output.artifacts,
                    options: &options,
                    env: &env,
                    paths: &kiseki_platform::KisekiPaths::new("/c", "/k", "/d"),
                },
                &detection,
                GhosttyAdapter.capabilities(&detection, &env),
            )
            .unwrap();

        assert!(plan.has_generated("ghostty.conf"));
        let anchor = plan
            .operations
            .iter()
            .find_map(|operation| match operation {
                FileOperation::UpsertMarkerBlock { body, target, .. } => Some((body, target)),
                _ => None,
            })
            .expect("anchor");
        assert!(anchor.0.starts_with("config-file = ?"));
        assert!(anchor.0.replace('\\', "/").ends_with("generated/ghostty.conf"));
        assert_eq!(anchor.1, &config);
        let generated = plan.generated_writes().find(|(path, _)| *path == "ghostty.conf").unwrap().1;
        assert!(!generated.contains("shell-integration"));
        assert!(!generated.contains("keybind"));
        assert!(plan.host_writes().count() == 1);
    }

    #[test]
    fn read_only_symlink_downgrades_to_render_only() {
        let (dir, platform) = platform();
        let target = dir.path().join("nix-store-ghostty");
        platform.write_preserving_symlinks(&target, b"config\n").unwrap();
        let mut permissions = std::fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&target, permissions).unwrap();
        let link = dir.path().join("config-link");
        symlink(&target, &link);
        let detection = DetectionReport {
            installed: true,
            version: None,
            config_path: Some(link),
            path_kind: PathKind::ReadOnlySymlink(target),
            active_in_session: false,
            notes: Vec::new(),
        };
        let theme = theme();
        let output = compiled(platform.paths().config_dir());
        let plan = GhosttyAdapter
            .plan(
                &PlanContext {
                    theme: &theme,
                    artifacts: &output.artifacts,
                    options: &PlanOptions::default(),
                    env: &EnvContext::local(),
                    paths: &kiseki_platform::KisekiPaths::new("/c", "/k", "/d"),
                },
                &detection,
                CapabilityFlags::empty(),
            )
            .unwrap();
        assert!(plan.render_only);
        assert_eq!(plan.host_writes().count(), 0);
        assert!(plan.has_generated("ghostty.conf"));
        assert!(plan
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "adapter.ghostty.render-only"));
    }

    #[test]
    fn ssh_sessions_never_write_host_config() {
        let (dir, platform) = platform();
        let config = dir.path().join("config");
        platform.write_preserving_symlinks(&config, b"font-size = 12\n").unwrap();
        let detection = DetectionReport {
            installed: true,
            version: None,
            config_path: Some(config),
            path_kind: PathKind::Regular,
            active_in_session: false,
            notes: Vec::new(),
        };
        let theme = theme();
        let output = compiled(platform.paths().config_dir());
        let env = EnvContext {
            is_ssh: true,
            ..EnvContext::local()
        };
        let plan = GhosttyAdapter
            .plan(
                &PlanContext {
                    theme: &theme,
                    artifacts: &output.artifacts,
                    options: &PlanOptions::default(),
                    env: &env,
                    paths: &kiseki_platform::KisekiPaths::new("/c", "/k", "/d"),
                },
                &detection,
                GhosttyAdapter.capabilities(&detection, &env),
            )
            .unwrap();
        assert!(plan.render_only);
        assert_eq!(plan.host_writes().count(), 0);
    }

    #[test]
    fn unhook_removes_only_the_marker_block() {
        let detection = DetectionReport {
            installed: true,
            version: None,
            config_path: Some(PathBuf::from("/home/zz/.config/ghostty/config")),
            path_kind: PathKind::Regular,
            active_in_session: false,
            notes: Vec::new(),
        };
        let plan = GhosttyAdapter
            .plan_unhook(&detection, &kiseki_platform::KisekiPaths::new("/c", "/k", "/d"))
            .unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert!(matches!(
            plan.operations[0],
            FileOperation::RemoveMarkerBlock { .. }
        ));
    }

    fn symlink(target: &std::path::Path, link: &std::path::Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, link).unwrap();
    }
}
