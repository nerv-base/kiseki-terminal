//! Fastfetch adapter (RFC-0003 §4).
//!
//! Fastfetch reads a single config file and executes `command` modules. The
//! generated config is therefore static-only and lives entirely inside
//! `generated/`; nothing in the user's home is edited, and no global `fetch`
//! command is installed.

use kiseki_compiler::artifact::names as artifact_names;
use kiseki_platform::{Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, Result};

use crate::{Adapter, AdapterKind, AdapterPlan, DetectionReport, IntegrationTier, PlanContext, ValidationReport};

/// Fastfetch monitor adapter.
pub struct FastfetchAdapter;

impl Adapter for FastfetchAdapter {
    fn id(&self) -> &'static str {
        "fastfetch"
    }

    fn display_name(&self) -> &'static str {
        "Fastfetch"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::SystemMonitor
    }

    fn tier(&self) -> IntegrationTier {
        IntegrationTier::Tier1Managed
    }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let installed = kiseki_platform::deps::executable(platform, "fastfetch").is_some();
        Ok(DetectionReport {
            installed,
            version: None,
            config_path: None,
            path_kind: kiseki_platform::PathKind::Missing,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes: if installed {
                vec!["fastfetch found on PATH; run it through `kiseki fetch`".to_string()]
            } else {
                vec!["fastfetch is not installed; generated config is ready for later".to_string()]
            },
        })
    }

    fn capabilities(&self, _detection: &DetectionReport, _env: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::empty()
    }

    fn plan(
        &self,
        ctx: &PlanContext<'_>,
        _detection: &DetectionReport,
        _caps: CapabilityFlags,
    ) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        plan.add_generated(ctx.artifacts, artifact_names::FASTFETCH);
        if let Some(content) = ctx.artifacts.get(artifact_names::FASTFETCH) {
            match kiseki_compiler::fastfetch::parse_body(content) {
                Some(value) => {
                    if !kiseki_compiler::fastfetch::is_declarative(&value) {
                        plan.diagnostic(Diagnostic::error(
                            "adapter.fastfetch.executable-module",
                            "generated fastfetch config contains a command module",
                        ));
                    }
                    if let Some(source) = value
                        .get("logo")
                        .and_then(|logo| logo.get("source"))
                        .and_then(serde_json::Value::as_str)
                    {
                        if !std::path::Path::new(source).is_absolute() {
                            plan.diagnostic(Diagnostic::error(
                                "adapter.fastfetch.logo-relative",
                                format!("fastfetch logo path {source:?} is not absolute"),
                            ));
                        }
                    }
                }
                None => {
                    plan.diagnostic(Diagnostic::error(
                        "adapter.fastfetch.invalid-json",
                        "generated fastfetch.jsonc does not parse",
                    ));
                }
            }
        }
        Ok(plan)
    }

    fn validate(&self, plan: &AdapterPlan, _platform: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for diagnostic in plan.diagnostics.iter().filter(|diagnostic| diagnostic.is_fatal()) {
            report.push(diagnostic.clone());
        }
        Ok(report)
    }

    fn reload(
        &self,
        _detection: &DetectionReport,
        _caps: CapabilityFlags,
        _platform: &dyn Platform,
    ) -> Result<ReloadOutcome> {
        Ok(ReloadOutcome::AutoReload {
            detail: "fastfetch reads the generated jsonc on every run".to_string(),
        })
    }

    fn plan_unhook(
        &self,
        _detection: &DetectionReport,
        _paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan> {
        Ok(AdapterPlan::new(self.id()))
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

    const GLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.glsl");
    const HLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.hlsl");

    fn theme() -> ResolvedTheme {
        let pack = ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
        };
        ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced)
    }

    #[test]
    fn plan_writes_a_declarative_config_with_an_absolute_logo() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let theme = theme();
        let output = compile(
            &CompileInput::new(&theme, CompileOptions::new(platform.paths().config_dir(), "/data/themes/nerv-eva"))
                .with_glsl(Some(GLSL))
                .with_hlsl(Some(HLSL))
                .with_logo(Some("logo")),
        )
        .unwrap();
        let plan = FastfetchAdapter
            .plan(
                &PlanContext {
                    theme: &theme,
                    artifacts: &output.artifacts,
                    options: &PlanOptions::default(),
                    env: &EnvContext::local(),
                    paths: platform.paths(),
                },
                &DetectionReport::not_installed("fastfetch missing"),
                CapabilityFlags::empty(),
            )
            .unwrap();
        assert!(plan.has_generated("fastfetch.jsonc"));
        let content = plan.generated_writes().next().unwrap().1;
        let parsed = kiseki_compiler::fastfetch::parse_body(content).unwrap();
        assert!(kiseki_compiler::fastfetch::is_declarative(&parsed));
        assert!(parsed["logo"]["source"].as_str().unwrap().starts_with('/'));
        assert!(parsed.get("modules").is_some());
        assert_eq!(plan.host_writes().count(), 0);
    }
}
