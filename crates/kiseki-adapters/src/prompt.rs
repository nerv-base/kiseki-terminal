//! Prompt adapter for Oh My Posh and Starship (RFC-0003 §4, ADR-006).
//!
//! The adapter never rewrites a user's prompt config. It writes generated
//! artefacts and, when a hand-authored config is detected, emits a warning so
//! `init` can ask the user whether to keep their layout (palette-only) or use
//! the managed template.

use std::path::PathBuf;

use kiseki_compiler::artifact::names as artifact_names;
use kiseki_platform::{PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, PromptPolicy, Result};

use crate::{
    Adapter, AdapterKind, AdapterPlan, DetectionReport, FileOperation, IntegrationTier, PlanContext,
    ValidationReport,
};

/// Oh My Posh / Starship prompt adapter.
pub struct PromptAdapter;

impl PromptAdapter {
    fn omp_config_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform.env_var("POSH_THEME") {
            return Some(PathBuf::from(explicit));
        }
        let home = platform
            .env_var("HOME")
            .or_else(|| platform.env_var("USERPROFILE"))
            .map(PathBuf::from)?;
        let candidate = home
            .join(".config")
            .join("oh-my-posh")
            .join("config.json");
        Some(candidate)
    }

    fn starship_config_path(platform: &dyn Platform) -> Option<PathBuf> {
        if let Some(explicit) = platform.env_var("STARSHIP_CONFIG") {
            return Some(PathBuf::from(explicit));
        }
        let home = platform
            .env_var("HOME")
            .or_else(|| platform.env_var("USERPROFILE"))
            .map(PathBuf::from)?;
        Some(home.join(".config").join("starship.toml"))
    }
}

impl Adapter for PromptAdapter {
    fn id(&self) -> &'static str {
        "prompt"
    }

    fn display_name(&self) -> &'static str {
        "Oh My Posh / Starship"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::Prompt
    }

    fn tier(&self) -> IntegrationTier {
        IntegrationTier::Tier1Managed
    }

    fn detect(&self, _env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let omp_path = Self::omp_config_path(platform);
        let starship_path = Self::starship_config_path(platform);
        let omp_kind = omp_path
            .as_deref()
            .map(|path| platform.inspect(path))
            .unwrap_or(PathKind::Missing);
        let starship_kind = starship_path
            .as_deref()
            .map(|path| platform.inspect(path))
            .unwrap_or(PathKind::Missing);

        let mut notes = Vec::new();
        if omp_kind != PathKind::Missing {
            notes.push(format!(
                "existing Oh My Posh config detected at {}",
                omp_path.as_ref().unwrap().display()
            ));
        }
        if starship_kind != PathKind::Missing {
            notes.push(format!(
                "existing Starship config detected at {}",
                starship_path.as_ref().unwrap().display()
            ));
        }
        let config_path = if omp_kind != PathKind::Missing {
            omp_path
        } else if starship_kind != PathKind::Missing {
            starship_path
        } else {
            omp_path
        };
        let path_kind = config_path
            .as_deref()
            .map(|path| platform.inspect(path))
            .unwrap_or(PathKind::Missing);
        Ok(DetectionReport {
            installed: platform.which("oh-my-posh").is_some()
                || platform.which("starship").is_some()
                || path_kind != PathKind::Missing,
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: platform.env_var("POSH_SESSION_ID").is_some(),
            notes,
        })
    }

    fn capabilities(&self, _detection: &DetectionReport, _env: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::empty()
    }

    fn plan(
        &self,
        ctx: &PlanContext<'_>,
        detection: &DetectionReport,
        _caps: CapabilityFlags,
    ) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        match ctx.options.prompt_policy {
            PromptPolicy::Disabled => {
                plan.diagnostic(Diagnostic::info(
                    "adapter.prompt.disabled",
                    "prompt policy is disabled; no prompt artefact was planned",
                ));
                return Ok(plan);
            }
            PromptPolicy::PaletteOnly => {
                plan.add_generated(ctx.artifacts, artifact_names::STARSHIP_PALETTE);
                plan.add_generated(ctx.artifacts, artifact_names::PROMPT_OMP_PALETTE);
                if !detection.notes.is_empty() {
                    plan.diagnostic(
                        Diagnostic::info(
                            "adapter.prompt.palette-only",
                            "existing prompt layout kept; only palette fragments were written",
                        )
                        .with_hint("reference `palettes.kiseki` / `palette.list.kiseki_*` from your own config"),
                    );
                }
                return Ok(plan);
            }
            PromptPolicy::ManagedTemplate => {}
        }

        plan.add_generated(ctx.artifacts, artifact_names::PROMPT_OMP);
        plan.add_generated(ctx.artifacts, artifact_names::STARSHIP_PALETTE);
        if !detection.notes.is_empty() {
            for note in &detection.notes {
                plan.diagnostic(
                    Diagnostic::warning("adapter.prompt.user-config", note.clone())
                        .with_hint(
                            "run `kiseki init` and choose `palette-only`, or pass `--prompt palette-only`",
                        ),
                );
            }
        }
        Ok(plan)
    }

    fn validate(&self, plan: &AdapterPlan, _platform: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for diagnostic in plan.diagnostics.iter().filter(|diagnostic| diagnostic.is_fatal()) {
            report.push(diagnostic.clone());
        }
        for (path, content) in plan.generated_writes() {
            match path {
                artifact_names::PROMPT_OMP => match serde_json::from_str::<serde_json::Value>(content) {
                    Ok(value) => {
                        if !kiseki_compiler::prompt::omp_is_declarative(&value) {
                            report.push(Diagnostic::error(
                                "adapter.prompt.executable-segment",
                                "generated Oh My Posh config contains a command segment",
                            ));
                        }
                    }
                    Err(error) => report.push(Diagnostic::error(
                        "adapter.prompt.invalid-json",
                        format!("generated prompt.omp.json does not parse: {error}"),
                    )),
                },
                artifact_names::PROMPT_OMP_PALETTE
                    if serde_json::from_str::<serde_json::Value>(content).is_err() =>
                {
                    report.push(Diagnostic::error(
                        "adapter.prompt.invalid-json",
                        "generated prompt palette fragment does not parse",
                    ));
                }
                artifact_names::STARSHIP_PALETTE => {
                    if content.contains("[custom.") {
                        report.push(Diagnostic::error(
                            "adapter.prompt.custom-command",
                            "generated Starship config must never contain [custom.*]",
                        ));
                    }
                    if toml::from_str::<toml::Value>(content).is_err() {
                        report.push(Diagnostic::error(
                            "adapter.prompt.invalid-toml",
                            "generated starship-palette.toml does not parse",
                        ));
                    }
                }
                _ => {}
            }
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
            detail: "prompt environment refreshes on the next shell prompt".to_string(),
        })
    }

    fn plan_unhook(
        &self,
        _detection: &DetectionReport,
        _paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan> {
        // Kiseki never edits the user's prompt config, so nothing to unhook.
        Ok(AdapterPlan::new(self.id()))
    }
}

/// True when an operation targets a prompt artefact.
pub fn is_prompt_artifact(operation: &FileOperation) -> bool {
    matches!(
        operation,
        FileOperation::WriteGenerated { relative, .. }
            if matches!(
                relative.as_str(),
                artifact_names::PROMPT_OMP
                    | artifact_names::PROMPT_OMP_PALETTE
                    | artifact_names::STARSHIP_PALETTE
            )
    )
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

    fn compiled(config_dir: &std::path::Path, policy: PromptPolicy) -> kiseki_compiler::CompileOutput {
        let theme = theme();
        let options = CompileOptions::new(config_dir, "/data/themes/nerv-eva")
            .with_prompt_policy(policy);
        compile(
            &CompileInput::new(&theme, options)
                .with_glsl(Some(GLSL))
                .with_hlsl(Some(HLSL)),
        )
        .unwrap()
    }

    fn detection(notes: Vec<String>) -> DetectionReport {
        DetectionReport {
            installed: true,
            version: None,
            config_path: Some(PathBuf::from("/home/zz/.config/oh-my-posh/config.json")),
            path_kind: PathKind::Regular,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes,
        }
    }

    #[test]
    fn managed_template_writes_omp_and_starship_and_warns_about_user_config() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let output = compiled(platform.paths().config_dir(), PromptPolicy::ManagedTemplate);
        let plan = PromptAdapter
            .plan(
                &PlanContext {
                    theme: &theme(),
                    artifacts: &output.artifacts,
                    options: &PlanOptions::default(),
                    env: &EnvContext::local(),
                    paths: platform.paths(),
                },
                &detection(vec!["existing Oh My Posh config detected".to_string()]),
                CapabilityFlags::empty(),
            )
            .unwrap();
        assert!(plan.has_generated("prompt.omp.json"));
        assert!(plan.has_generated("starship-palette.toml"));
        assert!(plan
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "adapter.prompt.user-config"));
        assert!(!plan.has_fatal());
        assert_eq!(plan.host_writes().count(), 0, "prompt config is never edited");
    }

    #[test]
    fn palette_only_skips_the_managed_template() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let output = compiled(platform.paths().config_dir(), PromptPolicy::PaletteOnly);
        let plan = PromptAdapter
            .plan(
                &PlanContext {
                    theme: &theme(),
                    artifacts: &output.artifacts,
                    options: &PlanOptions {
                        prompt_policy: PromptPolicy::PaletteOnly,
                        ..PlanOptions::default()
                    },
                    env: &EnvContext::local(),
                    paths: platform.paths(),
                },
                &detection(vec!["existing config".to_string()]),
                CapabilityFlags::empty(),
            )
            .unwrap();
        assert!(!plan.has_generated("prompt.omp.json"));
        assert!(plan.has_generated("starship-palette.toml"));
        assert!(plan.has_generated("prompt.omp.palette.json"));
        assert!(plan
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "adapter.prompt.palette-only"));
    }

    #[test]
    fn disabled_plans_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let output = compiled(platform.paths().config_dir(), PromptPolicy::Disabled);
        let plan = PromptAdapter
            .plan(
                &PlanContext {
                    theme: &theme(),
                    artifacts: &output.artifacts,
                    options: &PlanOptions {
                        prompt_policy: PromptPolicy::Disabled,
                        ..PlanOptions::default()
                    },
                    env: &EnvContext::local(),
                    paths: platform.paths(),
                },
                &detection(Vec::new()),
                CapabilityFlags::empty(),
            )
            .unwrap();
        assert!(plan.operations.is_empty());
    }

    #[test]
    fn validate_rejects_executable_prompt_configs() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let mut plan = AdapterPlan::new("prompt");
        plan.push(FileOperation::WriteGenerated {
            relative: kiseki_compiler::ArtifactPath::new("prompt.omp.json").unwrap(),
            content: r#"{"blocks":[{"segments":[{"type":"command","command":"rm -rf /"}]}]}"#.to_string(),
        });
        let report = PromptAdapter.validate(&plan, &platform).unwrap();
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "adapter.prompt.executable-segment"));
    }

    #[test]
    fn validate_rejects_starship_custom_commands() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let mut plan = AdapterPlan::new("prompt");
        plan.push(FileOperation::WriteGenerated {
            relative: kiseki_compiler::ArtifactPath::new("starship-palette.toml").unwrap(),
            content: "[custom.evil]\ncommand = \"rm -rf /\"\n".to_string(),
        });
        let report = PromptAdapter.validate(&plan, &platform).unwrap();
        assert!(report.has_fatal());
    }
}
