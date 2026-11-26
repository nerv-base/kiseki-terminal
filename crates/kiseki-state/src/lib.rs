//! Persistent configuration, active state and crash-safe preview recovery.
//!
//! Three files matter:
//! * `config.toml` — long-lived user preferences (prompt policy, aliases, ...);
//! * `state.json` — the active theme, composition, rendering profile and the
//!   monotonic generation counter;
//! * `state.gen` — a single integer shell hooks read on every prompt.
//!
//! `state.json` and `state.gen` are always written atomically and in that
//! order, so a crash between them can only ever leave the shell one generation
//! behind, never reading a half-written JSON document.

use std::path::{Path, PathBuf};

use kiseki_platform::{KisekiPaths, Platform, WriteOutcome};
use kiseki_types::{KisekiError, PromptPolicy, RenderingProfile, Result, ThemeId};
use serde::{Deserialize, Serialize};

/// Schema version of `state.json` and `config.toml`.
pub const STATE_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// state.json
// ---------------------------------------------------------------------------

/// How the active theme is chosen when the system appearance changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AppearanceMode {
    /// The user picked a theme explicitly; never follow the OS.
    #[default]
    Manual,
    /// Follow the system appearance using `pairing`.
    Auto,
}

/// Optional light/dark companion themes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pairing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<ThemeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<ThemeId>,
}

/// Per-layer composition overrides (`kiseki compose`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompositionState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<ThemeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax: Option<ThemeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atmosphere: Option<ThemeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typography: Option<ThemeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<ThemeId>,
}

impl CompositionState {
    pub fn is_empty(&self) -> bool {
        self.palette.is_none()
            && self.syntax.is_none()
            && self.atmosphere.is_none()
            && self.typography.is_none()
            && self.overrides.is_none()
    }

    /// Referenced themes in a stable order.
    pub fn referenced(&self) -> Vec<&ThemeId> {
        [
            self.palette.as_ref(),
            self.syntax.as_ref(),
            self.atmosphere.as_ref(),
            self.typography.as_ref(),
            self.overrides.as_ref(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Rendering-related state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderingState {
    #[serde(default)]
    pub profile: RenderingProfile,
    #[serde(default)]
    pub crt: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg_opacity_override: Option<f64>,
}

impl Default for RenderingState {
    fn default() -> Self {
        Self {
            profile: RenderingProfile::Balanced,
            crt: false,
            bg_opacity_override: None,
        }
    }
}

/// Typography overrides and the lock flag.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TypographyState {
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_override_pt: Option<f64>,
}

/// The complete `state.json` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub schema_version: u32,
    pub generation: u64,
    pub active_theme: ThemeId,
    #[serde(default)]
    pub appearance_mode: AppearanceMode,
    #[serde(default)]
    pub pairing: Pairing,
    #[serde(default)]
    pub composition: CompositionState,
    #[serde(default)]
    pub rendering: RenderingState,
    #[serde(default)]
    pub typography: TypographyState,
    pub updated_at: String,
}

impl State {
    /// Fresh state for a first run.
    pub fn new(active_theme: ThemeId, now: &str) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            generation: 1,
            active_theme,
            appearance_mode: AppearanceMode::Manual,
            pairing: Pairing::default(),
            composition: CompositionState::default(),
            rendering: RenderingState::default(),
            typography: TypographyState::default(),
            updated_at: now.to_string(),
        }
    }

    /// Loads `state.json`, returning `None` when it does not exist yet.
    pub fn load(platform: &dyn Platform, paths: &KisekiPaths) -> Result<Option<Self>> {
        let Some(raw) = platform.read_to_string(&paths.state_file())? else {
            return Ok(None);
        };
        let state: State = serde_json::from_str(&raw)
            .map_err(|error| KisekiError::State(format!("{}: {error}", paths.state_file().display())))?;
        if state.schema_version != STATE_SCHEMA_VERSION {
            return Err(KisekiError::State(format!(
                "state.json schemaVersion {} is not supported (expected {STATE_SCHEMA_VERSION})",
                state.schema_version
            )));
        }
        Ok(Some(state))
    }

    /// Loads the state or synthesises a first-run document.
    pub fn load_or_initial(
        platform: &dyn Platform,
        paths: &KisekiPaths,
        fallback_theme: &ThemeId,
    ) -> Result<Self> {
        Ok(Self::load(platform, paths)?.unwrap_or_else(|| Self::new(fallback_theme.clone(), &platform.now_rfc3339())))
    }

    /// Writes `state.json` atomically. Does not touch `state.gen`.
    pub fn save(&self, platform: &dyn Platform, paths: &KisekiPaths) -> Result<()> {
        let encoded = serde_json::to_string_pretty(self).map_err(|error| KisekiError::Json(error.to_string()))?;
        platform
            .create_dir_all(paths.config_dir())?;
        reject_read_only(
            platform.write_preserving_symlinks(&paths.state_file(), format!("{encoded}\n").as_bytes())?,
            &paths.state_file(),
        )?;
        Ok(())
    }

    /// Increments the generation and stamps `updatedAt`.
    pub fn bump(&mut self, now: &str) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.updated_at = now.to_string();
        self.generation
    }

    /// Writes `state.gen` atomically. Called *after* `save`.
    pub fn write_gen(&self, platform: &dyn Platform, paths: &KisekiPaths) -> Result<()> {
        platform.create_dir_all(paths.config_dir())?;
        reject_read_only(
            platform.write_preserving_symlinks(
                &paths.gen_file(),
                format!("{}\n", self.generation).as_bytes(),
            )?,
            &paths.gen_file(),
        )?;
        Ok(())
    }

    /// Reads `state.gen`, tolerating a missing or malformed file.
    pub fn read_gen(platform: &dyn Platform, paths: &KisekiPaths) -> Result<Option<u64>> {
        let Some(raw) = platform.read_to_string(&paths.gen_file())? else {
            return Ok(None);
        };
        Ok(raw.trim().parse::<u64>().ok())
    }

    /// Saves the state and then advances the generation file.
    pub fn commit(&mut self, platform: &dyn Platform, paths: &KisekiPaths) -> Result<u64> {
        let generation = self.bump(&platform.now_rfc3339());
        self.save(platform, paths)?;
        self.write_gen(platform, paths)?;
        Ok(generation)
    }

    /// True when any layer composition is active.
    pub fn has_composition(&self) -> bool {
        !self.composition.is_empty()
    }
}

// ---------------------------------------------------------------------------
// config.toml
// ---------------------------------------------------------------------------

/// `config.toml` — long-lived user preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Config {
    /// Prompt integration behaviour chosen during `init`.
    pub prompt_policy: PromptPolicy,
    /// Adapter allow-list. `None` means "auto-detect everything installed".
    pub enabled_adapters: Option<Vec<String>>,
    /// Global artifacts-only mode.
    pub render_only: bool,
    /// Opt-in `theme` / `themes` aliases.
    pub shell_aliases: bool,
    /// When true, `kiseki font` cannot change the theme stack.
    pub font_locked: bool,
    /// Point-size scale for macOS CoreText vs Windows 96 DPI.
    pub dpi_scale: f64,
    /// Check for updates once a day (opt-in).
    pub auto_update_check: bool,
    /// Extra theme directories to scan for community packs.
    pub theme_dirs: Vec<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prompt_policy: PromptPolicy::ManagedTemplate,
            enabled_adapters: None,
            render_only: false,
            shell_aliases: false,
            font_locked: false,
            dpi_scale: 1.25,
            auto_update_check: false,
            theme_dirs: Vec::new(),
        }
    }
}

impl Config {
    /// Loads `config.toml`, returning defaults when it does not exist.
    pub fn load(platform: &dyn Platform, paths: &KisekiPaths) -> Result<Self> {
        let Some(raw) = platform.read_to_string(&paths.config_file())? else {
            return Ok(Self::default());
        };
        toml::from_str(&raw)
            .map_err(|error| KisekiError::State(format!("{}: {error}", paths.config_file().display())))
    }

    /// Writes `config.toml` atomically.
    pub fn save(&self, platform: &dyn Platform, paths: &KisekiPaths) -> Result<()> {
        let encoded = toml::to_string_pretty(self)
            .map_err(|error| KisekiError::State(format!("config.toml: {error}")))?;
        platform.create_dir_all(paths.config_dir())?;
        reject_read_only(
            platform.write_preserving_symlinks(&paths.config_file(), encoded.as_bytes())?,
            &paths.config_file(),
        )?;
        Ok(())
    }

    /// True when `adapter` is enabled by the current configuration.
    pub fn adapter_enabled(&self, adapter: &str) -> bool {
        self.enabled_adapters
            .as_ref()
            .is_none_or(|list| list.iter().any(|entry| entry == adapter))
    }
}

// ---------------------------------------------------------------------------
// preview/recovery.json
// ---------------------------------------------------------------------------

/// Written before a live preview mutates anything, so a crash can restore the
/// pre-preview state without user intervention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewRecovery {
    pub schema_version: u32,
    pub pid: u32,
    pub started_at: String,
    pub preview_theme: ThemeId,
    pub previous_state: State,
}

impl PreviewRecovery {
    pub fn load(platform: &dyn Platform, paths: &KisekiPaths) -> Result<Option<Self>> {
        let Some(raw) = platform.read_to_string(&paths.preview_recovery_file())? else {
            return Ok(None);
        };
        serde_json::from_str(&raw)
            .map(Some)
            .map_err(|error| KisekiError::State(format!("preview recovery: {error}")))
    }

    pub fn save(&self, platform: &dyn Platform, paths: &KisekiPaths) -> Result<()> {
        platform.create_dir_all(&paths.preview_dir())?;
        let encoded = serde_json::to_string_pretty(self)
            .map_err(|error| KisekiError::Json(error.to_string()))?;
        platform.write_preserving_symlinks(
            &paths.preview_recovery_file(),
            format!("{encoded}\n").as_bytes(),
        )?;
        Ok(())
    }

    /// True when the process that started the preview is still running.
    pub fn is_owner_alive(&self, platform: &dyn Platform) -> bool {
        platform.process_alive(self.pid)
    }
}

/// Zero-persistent-side-effect preview lifecycle.
pub struct PreviewSession;

impl PreviewSession {
    /// Starts a preview: records the current state and the previewed theme.
    ///
    /// Deliberately does **not** write `state.json` or `state.gen`.
    pub fn begin(
        platform: &dyn Platform,
        paths: &KisekiPaths,
        current: &State,
        preview_theme: ThemeId,
    ) -> Result<PreviewRecovery> {
        let recovery = PreviewRecovery {
            schema_version: STATE_SCHEMA_VERSION,
            pid: std::process::id(),
            started_at: platform.now_rfc3339(),
            preview_theme,
            previous_state: current.clone(),
        };
        recovery.save(platform, paths)?;
        platform.create_dir_all(&paths.preview_dir())?;
        platform.write_preserving_symlinks(
            &paths.preview_lock_file(),
            format!("pid={}\n", std::process::id()).as_bytes(),
        )?;
        Ok(recovery)
    }

    /// Restores the pre-preview state and clears the recovery files.
    pub fn abort(platform: &dyn Platform, paths: &KisekiPaths) -> Result<Option<PreviewRecovery>> {
        let Some(recovery) = PreviewRecovery::load(platform, paths)? else {
            return Ok(None);
        };
        recovery.previous_state.save(platform, paths)?;
        recovery.previous_state.write_gen(platform, paths)?;
        Self::clear(platform, paths)?;
        Ok(Some(recovery))
    }

    /// Reaps a crashed preview, leaving a live one alone.
    pub fn recover_stale(platform: &dyn Platform, paths: &KisekiPaths) -> Result<Option<PreviewRecovery>> {
        let Some(recovery) = PreviewRecovery::load(platform, paths)? else {
            return Ok(None);
        };
        if recovery.is_owner_alive(platform) {
            return Ok(None);
        }
        recovery.previous_state.save(platform, paths)?;
        recovery.previous_state.write_gen(platform, paths)?;
        Self::clear(platform, paths)?;
        Ok(Some(recovery))
    }

    /// Removes the recovery marker and lock without restoring anything.
    pub fn clear(platform: &dyn Platform, paths: &KisekiPaths) -> Result<()> {
        platform.remove_file(&paths.preview_recovery_file())?;
        platform.remove_file(&paths.preview_lock_file())?;
        Ok(())
    }

    /// True when a recovery marker exists.
    pub fn is_active(platform: &dyn Platform, paths: &KisekiPaths) -> Result<bool> {
        Ok(platform.exists(&paths.preview_recovery_file()))
    }
}

/// Convenience: a state file path under an arbitrary root (tests/tools).
pub fn state_path(root: &Path) -> PathBuf {
    root.join("state.json")
}

/// Engine state is authoritative: a read-only `state.json`/`state.gen`/`config`
/// must be a hard error, never a silent no-op that leaves the shell stale.
fn reject_read_only(outcome: WriteOutcome, path: &Path) -> Result<()> {
    match outcome {
        WriteOutcome::Written { .. } => Ok(()),
        WriteOutcome::SkippedReadOnly { reason, .. } => Err(KisekiError::State(format!(
            "{} is read-only ({reason}); kiseki cannot record state",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_platform::MockPlatform;

    fn platform() -> (tempfile::TempDir, MockPlatform) {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        (dir, platform)
    }

    fn theme(id: &str) -> ThemeId {
        ThemeId::new(id).unwrap()
    }

    #[test]
    fn state_roundtrips_and_validates_schema_version() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        let mut state = State::new(theme("nerv-eva"), "2026-11-26T09:15:00Z");
        state.rendering.profile = RenderingProfile::Ultra;
        state.rendering.crt = true;
        state.rendering.bg_opacity_override = Some(0.55);
        state.composition.atmosphere = Some(theme("shinkai-rain"));
        state.save(&platform, &paths).unwrap();

        let loaded = State::load(&platform, &paths).unwrap().unwrap();
        assert_eq!(loaded, state);

        let raw = platform.read_to_string(&paths.state_file()).unwrap().unwrap();
        let mut corrupted: serde_json::Value = serde_json::from_str(&raw).unwrap();
        corrupted["schemaVersion"] = serde_json::json!(99);
        platform
            .write_preserving_symlinks(&paths.state_file(), corrupted.to_string().as_bytes())
            .unwrap();
        assert!(State::load(&platform, &paths).is_err());
    }

    #[test]
    fn missing_state_returns_none_and_initial_is_generation_one() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        assert_eq!(State::load(&platform, &paths).unwrap(), None);
        let state = State::load_or_initial(&platform, &paths, &theme("nerv-eva")).unwrap();
        assert_eq!(state.generation, 1);
        assert_eq!(state.active_theme.as_str(), "nerv-eva");
    }

    #[test]
    fn commit_writes_state_then_gen_and_is_monotonic() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        let mut state = State::new(theme("nerv-eva"), "2026-11-26T09:15:00Z");
        assert_eq!(state.commit(&platform, &paths).unwrap(), 2);
        assert_eq!(
            platform.read_to_string(&paths.gen_file()).unwrap().unwrap(),
            "2\n"
        );
        assert_eq!(state.commit(&platform, &paths).unwrap(), 3);
        assert_eq!(State::read_gen(&platform, &paths).unwrap(), Some(3));
        assert_eq!(State::load(&platform, &paths).unwrap().unwrap().generation, 3);
    }

    #[test]
    fn gen_read_tolerates_garbage_and_absence() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        assert_eq!(State::read_gen(&platform, &paths).unwrap(), None);
        platform
            .write_preserving_symlinks(&paths.gen_file(), b"not-a-number\n")
            .unwrap();
        assert_eq!(State::read_gen(&platform, &paths).unwrap(), None);
    }

    #[test]
    fn config_defaults_and_roundtrip() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        let default = Config::load(&platform, &paths).unwrap();
        assert_eq!(default, Config::default());
        assert_eq!(default.prompt_policy, PromptPolicy::ManagedTemplate);
        assert!(default.adapter_enabled("ghostty"));

        let config = Config {
            prompt_policy: PromptPolicy::PaletteOnly,
            enabled_adapters: Some(vec!["ghostty".to_string()]),
            render_only: true,
            shell_aliases: true,
            font_locked: true,
            dpi_scale: 1.5,
            auto_update_check: true,
            theme_dirs: vec![PathBuf::from("/themes/extra")],
        };
        config.save(&platform, &paths).unwrap();
        let loaded = Config::load(&platform, &paths).unwrap();
        assert_eq!(loaded, config);
        assert!(loaded.adapter_enabled("ghostty"));
        assert!(!loaded.adapter_enabled("zsh"));
    }

    #[test]
    fn composition_helpers() {
        let mut composition = CompositionState::default();
        assert!(composition.is_empty());
        composition.palette = Some(theme("kyoto-sakura"));
        composition.typography = Some(theme("starlight-8bit"));
        assert!(!composition.is_empty());
        assert_eq!(composition.referenced().len(), 2);
        composition.clear();
        assert!(composition.is_empty());
    }

    #[test]
    fn preview_begin_never_touches_state_and_abort_restores() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        let original = State::new(theme("nerv-eva"), "2026-11-26T09:15:00Z");
        original.save(&platform, &paths).unwrap();
        original.write_gen(&platform, &paths).unwrap();

        let recovery = PreviewSession::begin(&platform, &paths, &original, theme("kyoto-sakura")).unwrap();
        assert_eq!(recovery.preview_theme.as_str(), "kyoto-sakura");
        assert!(PreviewSession::is_active(&platform, &paths).unwrap());
        // state.json and state.gen are untouched by the preview.
        assert_eq!(State::load(&platform, &paths).unwrap().unwrap(), original);
        assert_eq!(State::read_gen(&platform, &paths).unwrap(), Some(1));

        let aborted = PreviewSession::abort(&platform, &paths).unwrap().unwrap();
        assert_eq!(aborted.previous_state, original);
        assert!(!PreviewSession::is_active(&platform, &paths).unwrap());
    }

    #[test]
    fn crashed_preview_is_recovered_when_the_pid_is_dead() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        let original = State::new(theme("nerv-eva"), "2026-11-26T09:15:00Z");
        original.save(&platform, &paths).unwrap();
        original.write_gen(&platform, &paths).unwrap();

        // Simulate a crash: write the recovery marker with a dead PID.
        let recovery = PreviewRecovery {
            schema_version: STATE_SCHEMA_VERSION,
            pid: 999_999,
            started_at: "2026-11-26T09:16:00Z".to_string(),
            preview_theme: theme("kyoto-sakura"),
            previous_state: original.clone(),
        };
        recovery.save(&platform, &paths).unwrap();
        platform.set_pid_alive(999_999, false);

        let recovered = PreviewSession::recover_stale(&platform, &paths).unwrap().unwrap();
        assert_eq!(recovered.previous_state, original);
        assert!(!PreviewSession::is_active(&platform, &paths).unwrap());
        assert_eq!(State::load(&platform, &paths).unwrap().unwrap(), original);
    }

    #[test]
    fn live_preview_is_left_alone() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        let state = State::new(theme("nerv-eva"), "2026-11-26T09:15:00Z");
        let recovery = PreviewRecovery {
            schema_version: STATE_SCHEMA_VERSION,
            pid: 4242,
            started_at: "2026-11-26T09:16:00Z".to_string(),
            preview_theme: theme("kyoto-sakura"),
            previous_state: state,
        };
        recovery.save(&platform, &paths).unwrap();
        platform.set_pid_alive(4242, true);
        assert!(PreviewSession::recover_stale(&platform, &paths).unwrap().is_none());
        assert!(PreviewSession::is_active(&platform, &paths).unwrap());
    }

    #[cfg(unix)]
    fn make_read_only(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444)).unwrap();
    }

    #[cfg(unix)]
    fn make_writable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn read_only_engine_files_are_a_hard_error() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        let state = State::new(theme("nerv-eva"), "2026-11-26T09:15:00Z");
        state.save(&platform, &paths).unwrap();
        state.write_gen(&platform, &paths).unwrap();
        Config::default().save(&platform, &paths).unwrap();

        make_read_only(&paths.state_file());
        let error = state.save(&platform, &paths).unwrap_err();
        assert!(matches!(error, KisekiError::State(_)), "{error}");
        assert!(error.to_string().contains("read-only"), "{error}");
        make_writable(&paths.state_file());

        make_read_only(&paths.gen_file());
        let error = state.write_gen(&platform, &paths).unwrap_err();
        assert!(matches!(error, KisekiError::State(_)), "{error}");
        make_writable(&paths.gen_file());

        make_read_only(&paths.config_file());
        let error = Config::default().save(&platform, &paths).unwrap_err();
        assert!(matches!(error, KisekiError::State(_)), "{error}");
        make_writable(&paths.config_file());

        // The writable path still works after the check.
        state.write_gen(&platform, &paths).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn commit_fails_on_a_read_only_state_file() {
        let (_dir, platform) = platform();
        let paths = platform.paths().clone();
        let mut state = State::new(theme("nerv-eva"), "2026-11-26T09:15:00Z");
        state.save(&platform, &paths).unwrap();
        make_read_only(&paths.state_file());
        let error = state.commit(&platform, &paths).unwrap_err();
        assert!(matches!(error, KisekiError::State(_)), "{error}");
        // `state.gen` was never advanced, so shells never see a half-commit.
        assert_eq!(State::read_gen(&platform, &paths).unwrap(), None);
        make_writable(&paths.state_file());
    }
}
