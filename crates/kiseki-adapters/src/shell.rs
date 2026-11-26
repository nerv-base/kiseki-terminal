//! Zsh and PowerShell adapters (RFC-0001 §5, RFC-0003 §4).
//!
//! Both adapters follow the same shape: the generated `env.*` file carries the
//! visual environment, and a tiny marker block in the user's profile adds a
//! zero-fork `state.gen` sync hook. Nothing else in the profile is touched, and
//! no alias (`cat`, `ls`, `grep`), `zoxide init` or key binding is ever emitted.

use kiseki_compiler::marker::{MARKER_BEGIN, MARKER_END};
use kiseki_compiler::shell::{self, ShellKind};
use kiseki_platform::{PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, OsKind, Result};

use crate::{
    render_only_diagnostic, Adapter, AdapterKind, AdapterPlan, DetectionReport, FileOperation,
    IntegrationTier, PlanContext, ValidationReport,
};

// ---------------------------------------------------------------------------
// Zsh
// ---------------------------------------------------------------------------

/// Zsh adapter.
pub struct ZshAdapter;

impl ZshAdapter {
    fn config_path(platform: &dyn Platform) -> Option<std::path::PathBuf> {
        if let Some(explicit) = platform.env_var("KISEKI_ZSHRC") {
            return Some(std::path::PathBuf::from(explicit));
        }
        let home = platform
            .env_var("HOME")
            .or_else(|| platform.env_var("USERPROFILE"))
            .map(std::path::PathBuf::from)?;
        let zdotdir = platform
            .env_var("ZDOTDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or(home);
        Some(zdotdir.join(".zshrc"))
    }
}

impl Adapter for ZshAdapter {
    fn id(&self) -> &'static str {
        "zsh"
    }

    fn display_name(&self) -> &'static str {
        "Zsh"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::Shell
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
        let installed = platform.which("zsh").is_some()
            || path_kind != PathKind::Missing
            || platform
                .env_var("SHELL")
                .is_some_and(|shell| shell.ends_with("zsh"));
        let mut notes = Vec::new();
        if path_kind == PathKind::Missing {
            notes.push("no .zshrc found; generated env.zsh is available for manual sourcing".to_string());
        }
        if env.is_ssh {
            notes.push("SSH session: host config writes are suppressed".to_string());
        }
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            active_in_session: platform
                .env_var("SHELL")
                .is_some_and(|shell| shell.ends_with("zsh")),
            notes,
        })
    }

    fn capabilities(&self, _detection: &DetectionReport, _env: &EnvContext) -> CapabilityFlags {
        // Shells provide environment only; terminal visuals belong to the
        // terminal adapter.
        CapabilityFlags::empty()
    }

    fn plan(
        &self,
        ctx: &PlanContext<'_>,
        detection: &DetectionReport,
        _caps: CapabilityFlags,
    ) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        plan.add_generated(ctx.artifacts, kiseki_compiler::artifact_names::ENV_ZSH);
        let body = shell::zsh_init_script(ctx.paths.config_dir(), ctx.options.aliases);
        for diagnostic in shell::lint_shell_artifacts(&body) {
            plan.diagnostic(diagnostic);
        }

        let Some(config_path) = detection.config_path.as_ref() else {
            return Ok(plan);
        };
        if !ctx.options.host_writes_allowed(ctx.env) {
            plan.render_only = true;
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.zsh.render-only",
                    "host writes suppressed; source generated/env.zsh manually",
                )
                .with_hint(format!(
                    "source \"{}/env.zsh\"",
                    ctx.paths.config_dir().join("generated").display()
                )),
            );
            return Ok(plan);
        }
        if detection.path_kind == PathKind::Missing
            && (!ctx.options.allow_create_host_files || !detection.installed)
        {
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.zsh.no-profile",
                    "no .zshrc found; generated env.zsh was not mounted",
                )
                .with_hint(format!(
                    "add `source \"{}/env.zsh\"` to your zsh startup file",
                    ctx.paths.config_dir().join("generated").display()
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
                    "add `source \"{}/env.zsh\"` to your Nix/Home Manager zsh config",
                    ctx.paths.config_dir().join("generated").display()
                ),
            ));
            return Ok(plan);
        }

        plan.push(FileOperation::UpsertMarkerBlock {
            target: config_path.clone(),
            begin: MARKER_BEGIN.to_string(),
            end: MARKER_END.to_string(),
            body,
        });
        Ok(plan)
    }

    fn validate(&self, plan: &AdapterPlan, _platform: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for diagnostic in plan.diagnostics.iter().filter(|diagnostic| diagnostic.is_fatal()) {
            report.push(diagnostic.clone());
        }
        for operation in &plan.operations {
            match operation {
                FileOperation::UpsertMarkerBlock { body, .. } => {
                    for diagnostic in shell::lint_shell_artifacts(body) {
                        report.push(diagnostic);
                    }
                }
                FileOperation::WriteGenerated { relative, content }
                    if relative.as_str() == kiseki_compiler::artifact_names::ENV_ZSH =>
                {
                    for diagnostic in shell::lint_shell_artifacts(content) {
                        report.push(diagnostic);
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
            detail: "running shells sync on the next prompt via state.gen".to_string(),
        })
    }

    fn plan_unhook(
        &self,
        detection: &DetectionReport,
        _paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan> {
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
// PowerShell
// ---------------------------------------------------------------------------

/// PowerShell 7 / Windows PowerShell adapter.
pub struct PowerShellAdapter;

impl PowerShellAdapter {
    fn config_path(platform: &dyn Platform) -> Option<std::path::PathBuf> {
        if let Some(explicit) = platform.env_var("KISEKI_PWSH_PROFILE") {
            return Some(std::path::PathBuf::from(explicit));
        }
        let home = platform
            .env_var("HOME")
            .or_else(|| platform.env_var("USERPROFILE"))
            .map(std::path::PathBuf::from)?;
        Some(match platform.os() {
            OsKind::Windows => home
                .join("Documents")
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
            OsKind::MacOs | OsKind::Linux => home
                .join(".config")
                .join("powershell")
                .join("Microsoft.PowerShell_profile.ps1"),
        })
    }
}

impl Adapter for PowerShellAdapter {
    fn id(&self) -> &'static str {
        "pwsh"
    }

    fn display_name(&self) -> &'static str {
        "PowerShell"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::Shell
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
        let installed = platform.which("pwsh").is_some()
            || platform.which("powershell").is_some()
            || path_kind != PathKind::Missing;
        let mut notes = Vec::new();
        if path_kind == PathKind::Missing {
            notes.push("no PowerShell profile found; generated env.ps1 is available for manual sourcing".to_string());
        }
        if env.is_ssh {
            notes.push("SSH session: host config writes are suppressed".to_string());
        }
        Ok(DetectionReport {
            installed,
            version: None,
            config_path,
            path_kind,
            active_in_session: env.terminal_is("pwsh") || env.terminal_is("powershell"),
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
        plan.add_generated(ctx.artifacts, kiseki_compiler::artifact_names::ENV_PS1);
        let body = shell::pwsh_init_script(ctx.paths.config_dir(), ctx.options.aliases);
        for diagnostic in shell::lint_shell_artifacts(&body) {
            plan.diagnostic(diagnostic);
        }

        let Some(config_path) = detection.config_path.as_ref() else {
            return Ok(plan);
        };
        if !ctx.options.host_writes_allowed(ctx.env) {
            plan.render_only = true;
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.pwsh.render-only",
                    "host writes suppressed; dot-source generated/env.ps1 manually",
                )
                .with_hint(format!(
                    ". \"{}/env.ps1\"",
                    ctx.paths.config_dir().join("generated").display()
                )),
            );
            return Ok(plan);
        }
        if detection.path_kind == PathKind::Missing
            && (!ctx.options.allow_create_host_files || !detection.installed)
        {
            plan.diagnostic(
                Diagnostic::info(
                    "adapter.pwsh.no-profile",
                    "no PowerShell profile found; generated env.ps1 was not mounted",
                )
                .with_hint(format!(
                    "add `. \"{}/env.ps1\"` to your $PROFILE",
                    ctx.paths.config_dir().join("generated").display()
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
                    "add `. \"{}/env.ps1\"` to your Nix/Home Manager PowerShell profile",
                    ctx.paths.config_dir().join("generated").display()
                ),
            ));
            return Ok(plan);
        }

        plan.push(FileOperation::UpsertMarkerBlock {
            target: config_path.clone(),
            begin: MARKER_BEGIN.to_string(),
            end: MARKER_END.to_string(),
            body,
        });
        Ok(plan)
    }

    fn validate(&self, plan: &AdapterPlan, _platform: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for diagnostic in plan.diagnostics.iter().filter(|diagnostic| diagnostic.is_fatal()) {
            report.push(diagnostic.clone());
        }
        for operation in &plan.operations {
            match operation {
                FileOperation::UpsertMarkerBlock { body, .. } => {
                    for diagnostic in shell::lint_shell_artifacts(body) {
                        report.push(diagnostic);
                    }
                }
                FileOperation::WriteGenerated { relative, content }
                    if relative.as_str() == kiseki_compiler::artifact_names::ENV_PS1 =>
                {
                    for diagnostic in shell::lint_shell_artifacts(content) {
                        report.push(diagnostic);
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
            detail: "running shells sync on the next prompt via state.gen".to_string(),
        })
    }

    fn plan_unhook(
        &self,
        detection: &DetectionReport,
        _paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan> {
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

/// The shell a `shell-init` command was asked for.
pub fn script_for(kind: ShellKind, config_dir: &std::path::Path, aliases: bool) -> String {
    shell::init_script(kind, config_dir, aliases)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlanOptions;
    use kiseki_compiler::{compile, CompileInput, CompileOptions};
    use kiseki_platform::MockPlatform;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_types::{PromptPolicy, RenderingProfile};

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
            .with_prompt_policy(policy)
            .with_aliases(true);
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

    fn detection_for(path: &std::path::Path, kind: PathKind) -> DetectionReport {
        DetectionReport {
            installed: true,
            version: None,
            config_path: Some(path.to_path_buf()),
            path_kind: kind,
            active_in_session: true,
            notes: Vec::new(),
        }
    }

    #[test]
    fn zsh_plan_writes_env_and_one_anchor_block() {
        let (dir, platform) = platform();
        let zshrc = dir.path().join(".zshrc");
        platform.write_preserving_symlinks(&zshrc, b"# my zshrc\nexport FOO=1\n").unwrap();
        let output = compiled(platform.paths().config_dir(), PromptPolicy::ManagedTemplate);
        let env = EnvContext::local();
        let options = PlanOptions {
            aliases: true,
            ..PlanOptions::default()
        };
        let plan = ZshAdapter
            .plan(
                &PlanContext {
                    theme: &theme(),
                    artifacts: &output.artifacts,
                    options: &options,
                    env: &env,
                    paths: platform.paths(),
                },
                &detection_for(&zshrc, PathKind::Regular),
                CapabilityFlags::empty(),
            )
            .unwrap();
        assert!(plan.has_generated("env.zsh"));
        let body = plan
            .operations
            .iter()
            .find_map(|operation| match operation {
                FileOperation::UpsertMarkerBlock { body, .. } => Some(body),
                _ => None,
            })
            .expect("anchor");
        assert!(body.contains("_kiseki_sync_hook"));
        assert!(body.contains("add-zsh-hook precmd _kiseki_sync_hook"));
        assert!(body.contains("alias theme='kiseki use'"));
        assert!(!body.contains("alias cat"));
        assert!(!body.contains("zoxide"));
        assert!(!plan.has_fatal());
    }

    #[test]
    fn zsh_missing_profile_is_not_created_without_init() {
        let (dir, platform) = platform();
        let zshrc = dir.path().join(".zshrc");
        let output = compiled(platform.paths().config_dir(), PromptPolicy::ManagedTemplate);
        let plan = ZshAdapter
            .plan(
                &PlanContext {
                    theme: &theme(),
                    artifacts: &output.artifacts,
                    options: &PlanOptions::default(),
                    env: &EnvContext::local(),
                    paths: platform.paths(),
                },
                &detection_for(&zshrc, PathKind::Missing),
                CapabilityFlags::empty(),
            )
            .unwrap();
        assert_eq!(plan.host_writes().count(), 0);
        assert!(plan.has_generated("env.zsh"));
        assert!(plan
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "adapter.zsh.no-profile"));
    }

    #[test]
    fn read_only_zshrc_downgrades_to_render_only() {
        let (dir, platform) = platform();
        let zshrc = dir.path().join(".zshrc");
        let output = compiled(platform.paths().config_dir(), PromptPolicy::ManagedTemplate);
        let plan = ZshAdapter
            .plan(
                &PlanContext {
                    theme: &theme(),
                    artifacts: &output.artifacts,
                    options: &PlanOptions::default(),
                    env: &EnvContext::local(),
                    paths: platform.paths(),
                },
                &detection_for(&zshrc, PathKind::ReadOnlyFile),
                CapabilityFlags::empty(),
            )
            .unwrap();
        assert!(plan.render_only);
        assert_eq!(plan.host_writes().count(), 0);
        assert!(plan.has_generated("env.zsh"));
    }

    #[test]
    fn pwsh_plan_uses_dotnet_read_for_zero_forks() {
        let (dir, platform) = platform();
        let profile = dir.path().join("profile.ps1");
        platform.write_preserving_symlinks(&profile, b"# profile\n").unwrap();
        let output = compiled(platform.paths().config_dir(), PromptPolicy::PaletteOnly);
        let plan = PowerShellAdapter
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
                &detection_for(&profile, PathKind::Regular),
                CapabilityFlags::empty(),
            )
            .unwrap();
        assert!(plan.has_generated("env.ps1"));
        let body = plan
            .operations
            .iter()
            .find_map(|operation| match operation {
                FileOperation::UpsertMarkerBlock { body, .. } => Some(body),
                _ => None,
            })
            .expect("anchor");
        assert!(body.contains("[System.IO.File]::ReadAllText"));
        assert!(!body.contains("& kiseki"));
        assert!(!body.contains("Set-PSReadLineKeyHandler"));
        assert!(!plan.has_fatal());
    }

    #[test]
    fn detect_finds_profiles_through_overrides() {
        let (dir, platform) = platform();
        let zshrc = dir.path().join("zshrc");
        let profile = dir.path().join("profile.ps1");
        platform.write_preserving_symlinks(&zshrc, b"").unwrap();
        platform.write_preserving_symlinks(&profile, b"").unwrap();
        platform.set_env("KISEKI_ZSHRC", &zshrc.to_string_lossy());
        platform.set_env("KISEKI_PWSH_PROFILE", &profile.to_string_lossy());
        let env = EnvContext::local();
        let zsh = ZshAdapter.detect(&env, &platform).unwrap();
        assert!(zsh.installed);
        assert_eq!(zsh.config_path.as_deref(), Some(zshrc.as_path()));
        let pwsh = PowerShellAdapter.detect(&env, &platform).unwrap();
        assert!(pwsh.installed);
        assert_eq!(pwsh.config_path.as_deref(), Some(profile.as_path()));
    }

    #[test]
    fn unhook_removes_the_marker_block() {
        let detection = detection_for(
            std::path::Path::new("/home/zz/.zshrc"),
            PathKind::Regular,
        );
        let plan = ZshAdapter
            .plan_unhook(&detection, &kiseki_platform::KisekiPaths::new("/c", "/k", "/d"))
            .unwrap();
        assert!(matches!(
            plan.operations[0],
            FileOperation::RemoveMarkerBlock { .. }
        ));
    }
}
