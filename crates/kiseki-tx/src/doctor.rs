//! `kiseki doctor` checks (RFC-0001 §3, RFC-0003 §5.1).
//!
//! Doctor is read-only: it never fixes anything unless the CLI explicitly
//! chooses to, which keeps diagnostics safe to run from a rescue TTY.

use kiseki_adapters::{AdapterRegistry, DetectionReport};
use kiseki_platform::{PathKind, Platform};
use kiseki_state::{Config, PreviewRecovery, State};
use kiseki_types::{EnvContext, Result};

/// Severity of one doctor finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DoctorCheckStatus {
    Ok,
    Info,
    Warning,
    Error,
}

impl DoctorCheckStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    pub const fn is_failure(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// One named finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorCheckStatus,
    pub message: String,
    pub hint: Option<String>,
}

impl DoctorCheck {
    pub fn new(name: impl Into<String>, status: DoctorCheckStatus, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            message: message.into(),
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// The complete doctor report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, check: DoctorCheck) {
        self.checks.push(check);
    }

    pub fn has_errors(&self) -> bool {
        self.checks.iter().any(|check| check.status.is_failure())
    }

    pub fn count(&self, status: DoctorCheckStatus) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == status)
            .count()
    }

    pub fn items(&self, status: DoctorCheckStatus) -> impl Iterator<Item = &DoctorCheck> {
        self.checks.iter().filter(move |check| check.status == status)
    }
}

/// Runs every read-only check.
pub fn doctor(
    platform: &dyn Platform,
    registry: &AdapterRegistry,
    env: &EnvContext,
    state: Option<&State>,
    config: Result<Config>,
) -> Result<DoctorReport> {
    let mut report = DoctorReport::new();
    let paths = platform.paths();

    // --- paths -----------------------------------------------------------
    match platform.inspect(paths.config_dir()) {
        PathKind::Missing => report.push(DoctorCheck::new(
            "paths.config",
            DoctorCheckStatus::Info,
            format!("{} does not exist yet; `kiseki init` will create it", paths.config_dir().display()),
        )),
        kind if kind.is_writable() => report.push(DoctorCheck::new(
            "paths.config",
            DoctorCheckStatus::Ok,
            format!("{} is writable", paths.config_dir().display()),
        )),
        kind => report.push(
            DoctorCheck::new(
                "paths.config",
                DoctorCheckStatus::Warning,
                format!("{} is {}", paths.config_dir().display(), kind.label()),
            )
            .with_hint("move KISEKI_HOME to a writable directory or fix permissions"),
        ),
    }

    // --- config ----------------------------------------------------------
    match config {
        Ok(config) => {
            if config.render_only {
                report.push(DoctorCheck::new(
                    "config.render-only",
                    DoctorCheckStatus::Info,
                    "config.toml enables render-only mode",
                ));
            }
            if config.prompt_policy != kiseki_types::PromptPolicy::ManagedTemplate {
                report.push(DoctorCheck::new(
                    "config.prompt",
                    DoctorCheckStatus::Info,
                    format!("prompt policy is {}", config.prompt_policy),
                ));
            }
        }
        Err(error) => report.push(
            DoctorCheck::new("config.parse", DoctorCheckStatus::Error, error.to_string())
                .with_hint("fix or delete config.toml; defaults will be used"),
        ),
    }

    // --- state -----------------------------------------------------------
    match state {
        Some(state) => {
            report.push(DoctorCheck::new(
                "state.active",
                DoctorCheckStatus::Ok,
                format!(
                    "active theme {} at profile {} (generation {})",
                    state.active_theme, state.rendering.profile, state.generation
                ),
            ));
            match State::read_gen(platform, paths)? {
                Some(generation) if generation == state.generation => report.push(DoctorCheck::new(
                    "state.gen",
                    DoctorCheckStatus::Ok,
                    format!("state.gen matches generation {generation}"),
                )),
                Some(generation) => report.push(DoctorCheck::new(
                    "state.gen",
                    DoctorCheckStatus::Warning,
                    format!(
                        "state.gen says {generation} but state.json says {}",
                        state.generation
                    ),
                )),
                None => report.push(DoctorCheck::new(
                    "state.gen",
                    DoctorCheckStatus::Warning,
                    "state.gen is missing; shell hooks will not refresh",
                )),
            }
            if state.appearance_mode == kiseki_state::AppearanceMode::Auto
                && state.pairing.dark.is_none()
                && state.pairing.light.is_none()
            {
                report.push(DoctorCheck::new(
                    "state.pairing",
                    DoctorCheckStatus::Warning,
                    "appearance mode is auto but no light/dark pairing is configured",
                ));
            }
        }
        None => report.push(DoctorCheck::new(
            "state.load",
            DoctorCheckStatus::Info,
            "no state.json yet; first `kiseki use` will create it",
        )),
    }

    // --- preview recovery ------------------------------------------------
    if PreviewRecovery::load(platform, paths)?.is_some() {
        if let Some(recovery) = PreviewRecovery::load(platform, paths)? {
            if recovery.is_owner_alive(platform) {
                report.push(DoctorCheck::new(
                    "preview.active",
                    DoctorCheckStatus::Warning,
                    format!("a live preview is running (pid {})", recovery.pid),
                ));
            } else {
                report.push(
                    DoctorCheck::new(
                        "preview.stale",
                        DoctorCheckStatus::Warning,
                        format!(
                            "preview from pid {} crashed; it will be restored on the next transaction",
                            recovery.pid
                        ),
                    )
                    .with_hint("run `kiseki recover --safe` to restore immediately"),
                );
            }
        }
    }

    // --- lock ------------------------------------------------------------
    if platform.exists(&paths.lock_file()) {
        report.push(
            DoctorCheck::new(
                "lock.present",
                DoctorCheckStatus::Warning,
                format!("{} exists; another kiseki process may be running", paths.lock_file().display()),
            )
            .with_hint("if no kiseki process is running, delete the stale lock file"),
        );
    }

    // --- adapters --------------------------------------------------------
    for adapter in registry.iter() {
        let name = format!("adapter.{}", adapter.id());
        match adapter.detect(env, platform) {
            Ok(detection) => {
                report.push(DoctorCheck::new(
                    format!("{name}.detect"),
                    if detection.installed {
                        DoctorCheckStatus::Ok
                    } else {
                        DoctorCheckStatus::Info
                    },
                    format!(
                        "{}: {} ({})",
                        adapter.display_name(),
                        if detection.installed { "installed" } else { "not installed" },
                        detection.path_kind.label()
                    ),
                ));
                for note in &detection.notes {
                    let status = if note.contains("overrides") {
                        DoctorCheckStatus::Warning
                    } else {
                        DoctorCheckStatus::Info
                    };
                    report.push(DoctorCheck::new(
                        format!("{name}.note"),
                        status,
                        note.clone(),
                    ));
                }
                for (path, kind) in detection.all_config_paths() {
                    if matches!(kind, PathKind::ReadOnlyFile | PathKind::ReadOnlySymlink(_)) {
                        report.push(
                            DoctorCheck::new(
                                format!("{name}.read-only"),
                                DoctorCheckStatus::Warning,
                                format!("{} is read-only; adapter runs render-only", path.display()),
                            )
                            .with_hint("add the generated include from the render-only hint manually"),
                        );
                    }
                }
            }
            Err(error) => report.push(
                DoctorCheck::new(
                    format!("{name}.detect"),
                    DoctorCheckStatus::Error,
                    error.to_string(),
                )
                .with_hint("report this at https://github.com/nerv-base/kiseki-terminal/issues"),
            ),
        }
    }

    // --- generated artefacts --------------------------------------------
    let generated = platform.list_dir(&paths.generated_dir())?;
    if generated.is_empty() {
        report.push(DoctorCheck::new(
            "generated.empty",
            DoctorCheckStatus::Info,
            "no generated artefacts yet",
        ));
    } else {
        report.push(DoctorCheck::new(
            "generated.present",
            DoctorCheckStatus::Ok,
            format!("{} generated entries", generated.len()),
        ));
    }

    Ok(report)
}

/// Extracts adapter detection for callers that need the raw data.
pub fn detect_adapters(
    platform: &dyn Platform,
    registry: &AdapterRegistry,
    env: &EnvContext,
) -> Vec<(String, Result<DetectionReport>)> {
    registry
        .iter()
        .map(|adapter| (adapter.id().to_string(), adapter.detect(env, platform)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_platform::MockPlatform;
    use kiseki_types::ThemeId;

    fn platform() -> (tempfile::TempDir, MockPlatform) {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        (dir, platform)
    }

    #[test]
    fn fresh_environment_reports_info_not_errors() {
        let (_dir, platform) = platform();
        let report = doctor(
            &platform,
            &AdapterRegistry::milestone1(),
            &EnvContext::local(),
            None,
            Ok(Config::default()),
        )
        .unwrap();
        assert!(!report.has_errors(), "{:?}", report.checks);
        assert!(report.count(DoctorCheckStatus::Info) > 0);
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "state.load"));
    }

    #[test]
    fn malformed_config_is_an_error() {
        let (_dir, platform) = platform();
        let report = doctor(
            &platform,
            &AdapterRegistry::milestone1(),
            &EnvContext::local(),
            None,
            Err(kiseki_types::KisekiError::State("bad config".to_string())),
        )
        .unwrap();
        assert!(report.has_errors());
        assert!(report
            .items(DoctorCheckStatus::Error)
            .any(|check| check.name == "config.parse"));
    }

    #[test]
    fn stale_preview_and_lock_are_warnings() {
        let (_dir, platform) = platform();
        let paths = platform.paths();
        let state = State::new(ThemeId::new("nerv-eva").unwrap(), "now");
        state.save(&platform, paths).unwrap();
        state.write_gen(&platform, paths).unwrap();
        platform
            .write_preserving_symlinks(&paths.lock_file(), b"pid=1\n")
            .unwrap();
        let recovery = kiseki_state::PreviewRecovery {
            schema_version: 1,
            pid: 999_999,
            started_at: "now".to_string(),
            preview_theme: ThemeId::new("kyoto-sakura").unwrap(),
            previous_state: state.clone(),
        };
        recovery.save(&platform, paths).unwrap();
        platform.set_pid_alive(999_999, false);

        let report = doctor(
            &platform,
            &AdapterRegistry::milestone1(),
            &EnvContext::local(),
            Some(&state),
            Ok(Config::default()),
        )
        .unwrap();
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "preview.stale"));
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "lock.present"));
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "state.gen" && check.status == DoctorCheckStatus::Ok));
    }

    #[test]
    fn wrong_generation_is_reported() {
        let (_dir, platform) = platform();
        let paths = platform.paths();
        let mut state = State::new(ThemeId::new("nerv-eva").unwrap(), "now");
        state.save(&platform, paths).unwrap();
        state.write_gen(&platform, paths).unwrap();
        state.generation = 10;
        let report = doctor(
            &platform,
            &AdapterRegistry::milestone1(),
            &EnvContext::local(),
            Some(&state),
            Ok(Config::default()),
        )
        .unwrap();
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "state.gen" && check.status == DoctorCheckStatus::Warning));
    }
}
