//! macOS Terminal.app adapter (RFC-0003 §4).
//!
//! Per RFC-0003, Apple Terminal is deliberately low-priority: it NEVER runs
//! AppleScript during default `auto` theme switches for Ghostty/iTerm2/Kitty
//! users. It only activates when the current session is `Apple_Terminal` OR
//! when the user explicitly requests `--only apple-terminal`.

use kiseki_compiler::apple_terminal::lint;
use kiseki_platform::{PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, OsKind, Result};

use crate::{
    Adapter, AdapterKind, AdapterPlan, DetectionReport, IntegrationTier, PlanContext,
    ValidationReport,
};

/// macOS Terminal.app adapter.
pub struct AppleTerminalAdapter;

impl Adapter for AppleTerminalAdapter {
    fn id(&self) -> &'static str {
        "apple-terminal"
    }

    fn display_name(&self) -> &'static str {
        "Apple Terminal"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::Terminal
    }

    fn tier(&self) -> IntegrationTier {
        IntegrationTier::Tier1Managed
    }

    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        if platform.os() != OsKind::MacOs {
            return Ok(DetectionReport::not_installed("Apple Terminal is macOS-only"));
        }
        let active = env.terminal_is("apple-terminal");
        let app_present = platform.exists(std::path::Path::new("/System/Applications/Utilities/Terminal.app"));
        let installed = active || app_present;
        let mut notes = Vec::new();
        if !active {
            notes.push("low-priority adapter: syncs only inside Apple Terminal or with --only apple-terminal".to_string());
        }
        Ok(DetectionReport {
            installed,
            version: None,
            config_path: None,
            path_kind: PathKind::Missing,
            extra_config_paths: Vec::new(),
            active_in_session: active,
            notes,
        })
    }

    fn capabilities(&self, _detection: &DetectionReport, env: &EnvContext) -> CapabilityFlags {
        let mut flags = CapabilityFlags::PALETTE_16 | CapabilityFlags::FONT_SINGLE_ONLY;
        if !env.is_ssh && env.os == OsKind::MacOs {
            flags |= CapabilityFlags::HOT_RELOAD_SIGNAL;
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
        if !detection.active_in_session && !ctx.options.explicit_only {
            return Ok(plan);
        }
        plan.add_generated(
            ctx.artifacts,
            kiseki_compiler::artifact_names::APPLE_TERMINAL_SCRIPT,
        );
        if !ctx.options.host_writes_allowed(ctx.env) {
            plan.render_only = true;
            plan.diagnostic(Diagnostic::info(
                "adapter.apple-terminal.render-only",
                "host writes suppressed; run generated/apple-terminal.applescript manually",
            ));
        }
        Ok(plan)
    }

    fn validate(&self, plan: &AdapterPlan, _platform: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for diagnostic in plan.diagnostics.iter().filter(|d| d.is_fatal()) {
            report.push(diagnostic.clone());
        }
        for (path, content) in plan.generated_writes() {
            if path == kiseki_compiler::artifact_names::APPLE_TERMINAL_SCRIPT {
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
        platform: &dyn Platform,
    ) -> Result<ReloadOutcome> {
        if !caps.contains(CapabilityFlags::HOT_RELOAD_SIGNAL) || !detection.installed {
            return Ok(ReloadOutcome::Skipped {
                detail: "Apple Terminal sync skipped".to_string(),
            });
        }
        let script = platform
            .paths()
            .generated(kiseki_compiler::artifact_names::APPLE_TERMINAL_SCRIPT);
        if !platform.exists(&script) {
            return Ok(ReloadOutcome::Skipped {
                detail: "generated apple-terminal.applescript missing".to_string(),
            });
        }
        match platform.run("osascript", &[&script.to_string_lossy()]) {
            Ok(output) if output.success() => Ok(ReloadOutcome::Signalled {
                detail: "applied visual theme to Apple Terminal via osascript".to_string(),
            }),
            Ok(output) => Ok(ReloadOutcome::Failed {
                detail: format!("osascript exited with {}: {}", output.status, output.stderr.trim()),
            }),
            Err(error) => Ok(ReloadOutcome::Failed {
                detail: error.to_string(),
            }),
        }
    }

    fn plan_unhook(
        &self,
        _detection: &DetectionReport,
        _paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan> {
        // Apple Terminal uses generated AppleScript only; no host config file to unhook.
        Ok(AdapterPlan::new(self.id()))
    }
}
