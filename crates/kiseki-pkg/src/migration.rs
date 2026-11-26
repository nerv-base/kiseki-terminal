//! `v1 -> v2` migration detection and planning (RFC-0001 §7.3, ADR-014).
//!
//! Detection is conservative: a shim is only deleted when its contents carry
//! the v1 `# kiseki-terminal-shim` marker, and a shell config is only edited
//! when it actually contains the v1 anchor block. Everything is backed up
//! first, so `kiseki migrate` is always reversible.

use std::path::{Path, PathBuf};

use kiseki_compiler::marker::remove_marker_block;
use kiseki_platform::{Platform, PathKind, WriteOutcome};
use kiseki_state::State;
use kiseki_types::{KisekiError, OsKind, RenderingProfile, Result, ThemeId};

/// v1 anchor markers (shell profiles and Ghostty configs use `#`).
pub const V1_BEGIN: &str = "# >>> kiseki-terminal >>>";
/// v1 closing marker.
pub const V1_END: &str = "# <<< kiseki-terminal <<<";
/// v1 anchor markers for Lua configs (WezTerm).
pub const V1_LUA_BEGIN: &str = "-- >>> kiseki-terminal >>>";
/// v1 Lua closing marker.
pub const V1_LUA_END: &str = "-- <<< kiseki-terminal <<<";
/// Marker written at the top of every v1 shim.
pub const V1_SHIM_MARKER: &str = "# kiseki-terminal-shim";
/// The eight unprefixed shims v1 wrote to `$PATH`.
pub const V1_SHIM_NAMES: &[&str] = &[
    "theme", "themes", "live", "motion", "crt", "termbg", "termfont", "fetch",
];

/// Preferences extracted from the v1 directory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct V1Preferences {
    pub theme: Option<String>,
    pub live: bool,
    pub crt: bool,
    pub bg_opacity: Option<f64>,
    /// Font family (`style`, falling back to the v1 `face` key).
    pub font_override: Option<String>,
    /// Point size from `font-override.json`.
    pub font_size_pt: Option<f64>,
    pub update_check: bool,
}

/// One host config carrying a v1 anchor block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V1ConfigHook {
    pub path: PathBuf,
    /// Opening marker, including the comment prefix of the target language.
    pub begin: String,
    /// Closing marker.
    pub end: String,
}

impl V1ConfigHook {
    pub fn hash(path: PathBuf) -> Self {
        Self {
            path,
            begin: V1_BEGIN.to_string(),
            end: V1_END.to_string(),
        }
    }

    pub fn lua(path: PathBuf) -> Self {
        Self {
            path,
            begin: V1_LUA_BEGIN.to_string(),
            end: V1_LUA_END.to_string(),
        }
    }
}

/// What `kiseki migrate` found.
#[derive(Debug, Clone, PartialEq)]
pub struct V1Detection {
    pub legacy_dir: Option<PathBuf>,
    /// Every shell, Ghostty and WezTerm config that still has a v1 anchor.
    pub configs: Vec<V1ConfigHook>,
    pub shims: Vec<PathBuf>,
    pub preferences: V1Preferences,
    pub notes: Vec<String>,
}

impl V1Detection {
    /// True when there is anything to migrate.
    pub fn is_present(&self) -> bool {
        self.legacy_dir.is_some() || !self.configs.is_empty() || !self.shims.is_empty()
    }

    /// Just the paths of the configs that need their v1 block removed.
    pub fn config_paths(&self) -> Vec<&PathBuf> {
        self.configs.iter().map(|hook| &hook.path).collect()
    }
}

/// One reversible migration step.
#[derive(Debug, Clone, PartialEq)]
pub enum MigrationStep {
    /// Copy the whole legacy directory into the backup folder.
    BackupDirectory { source: PathBuf },
    /// Remove the v1 anchor block from a host config (shell, Ghostty, WezTerm).
    RemoveMarkerBlock { target: PathBuf, begin: String, end: String },
    /// Delete a shim whose contents carry the v1 marker.
    DeleteShim { path: PathBuf },
    /// Import v1 preferences into v2 `state.json`.
    ImportPreferences { preferences: V1Preferences },
}

/// A reviewable migration plan.
#[derive(Debug, Clone, PartialEq)]
pub struct MigrationPlan {
    pub detection: V1Detection,
    pub backup_dir: PathBuf,
    pub steps: Vec<MigrationStep>,
}

/// Result of applying a migration.
#[derive(Debug, Clone, PartialEq)]
pub struct MigrationReport {
    pub backup_dir: PathBuf,
    pub removed_marker_blocks: Vec<PathBuf>,
    pub deleted_shims: Vec<PathBuf>,
    /// Every external file copied aside before it was modified or deleted.
    pub backed_up: Vec<PathBuf>,
    pub state: Option<State>,
    pub notes: Vec<String>,
}

impl MigrationPlan {
    pub fn summary(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(dir) = &self.detection.legacy_dir {
            lines.push(format!("back up legacy directory {}", dir.display()));
        }
        for config in &self.detection.configs {
            lines.push(format!("remove v1 hooks from {}", config.path.display()));
        }
        for shim in &self.detection.shims {
            lines.push(format!("delete v1 shim {}", shim.display()));
        }
        lines.push(format!(
            "import preferences: theme={:?} live={} crt={} bg={:?}",
            self.detection.preferences.theme,
            self.detection.preferences.live,
            self.detection.preferences.crt,
            self.detection.preferences.bg_opacity
        ));
        lines.push(format!("backups go to {}", self.backup_dir.display()));
        lines
    }

    /// Executes the plan. Idempotent: running twice is harmless.
    ///
    /// Every external file is copied aside *before* it is touched, so even a
    /// crash mid-migration leaves a complete backup behind.
    pub fn execute(&self, platform: &dyn Platform) -> Result<MigrationReport> {
        self.execute_with_options(platform, false)
    }

    /// Executes the plan, optionally leaving every v1 host hook in place.
    ///
    /// `render_only` still backs up the legacy directory and imports
    /// preferences, but never removes a v1 anchor block or deletes a shim: the
    /// host files are only touched once the user leaves render-only mode.
    pub fn execute_with_options(
        &self,
        platform: &dyn Platform,
        render_only: bool,
    ) -> Result<MigrationReport> {
        platform.create_dir_all(&self.backup_dir)?;
        let mut report = MigrationReport {
            backup_dir: self.backup_dir.clone(),
            removed_marker_blocks: Vec::new(),
            deleted_shims: Vec::new(),
            backed_up: Vec::new(),
            state: None,
            notes: Vec::new(),
        };

        for (index, step) in self.steps.iter().enumerate() {
            match step {
                MigrationStep::BackupDirectory { source } => {
                    let destination = self.backup_dir.join("legacy");
                    copy_tree(source, &destination)?;
                    report
                        .notes
                        .push(format!("backed up {} -> {}", source.display(), destination.display()));
                }
                MigrationStep::RemoveMarkerBlock { target, begin, end } => {
                    if render_only {
                        report.notes.push(format!(
                            "render-only: kept the v1 anchor block in {}",
                            target.display()
                        ));
                        continue;
                    }
                    let Some(original) = platform.read_to_string(target)? else {
                        continue;
                    };
                    if !original.contains(begin.as_str()) {
                        continue;
                    }
                    let backup = backup_file(
                        platform,
                        &self.backup_dir,
                        &self.backup_dir.join("hosts"),
                        target,
                        index + 1,
                    )?;
                    report.backed_up.push(backup);
                    let cleaned = remove_marker_block(&original, begin, end);
                    match platform.write_preserving_symlinks(target, cleaned.as_bytes())? {
                        WriteOutcome::Written { .. } => {}
                        WriteOutcome::SkippedReadOnly { reason, .. } => {
                            return Err(KisekiError::State(format!(
                                "refusing to edit {target_display}: {reason}",
                                target_display = target.display()
                            )));
                        }
                    }
                    report.removed_marker_blocks.push(target.clone());
                }
                MigrationStep::DeleteShim { path } => {
                    if render_only {
                        if platform.exists(path) {
                            report.notes.push(format!(
                                "render-only: kept the v1 shim {}",
                                path.display()
                            ));
                        }
                        continue;
                    }
                    if !platform.exists(path) {
                        continue;
                    }
                    let contents = platform.read_to_string(path)?.unwrap_or_default();
                    if !contents.contains(V1_SHIM_MARKER) {
                        report.notes.push(format!(
                            "kept {}: no v1 shim marker found",
                            path.display()
                        ));
                        continue;
                    }
                    let backup = backup_file(
                        platform,
                        &self.backup_dir,
                        &self.backup_dir.join("shims"),
                        path,
                        index + 1,
                    )?;
                    report.backed_up.push(backup);
                    platform.remove_file(path)?;
                    report.deleted_shims.push(path.clone());
                }
                MigrationStep::ImportPreferences { preferences } => {
                    // Re-running a stale migration plan must never rewind a
                    // newer v2 state (theme switches, `bg`, `font`, ...).
                    if State::load(platform, platform.paths())?.is_some() {
                        report.notes.push(
                            "kept the existing v2 state.json; v1 preferences were not re-applied"
                                .to_string(),
                        );
                        continue;
                    }
                    let available = crate::library::ThemeLibrary::open(platform, &[])
                        .list(platform)
                        .unwrap_or_default();
                    let theme = preferences
                        .theme
                        .as_deref()
                        .and_then(|raw| ThemeId::new(raw).ok())
                        .and_then(|id| {
                            if available.iter().any(|entry| entry.id == id) {
                                Some(id)
                            } else {
                                report.notes.push(format!(
                                    "v1 theme '{id}' is not installed in v2; defaulted to {}",
                                    crate::library::default_theme_id()
                                ));
                                None
                            }
                        })
                        .unwrap_or_else(crate::library::default_theme_id);
                    let mut state = State::new(theme, &platform.now_rfc3339());
                    state.rendering.profile = if preferences.live {
                        RenderingProfile::Balanced
                    } else {
                        RenderingProfile::Static
                    };
                    state.rendering.crt = preferences.crt;
                    state.rendering.bg_opacity_override = preferences.bg_opacity;
                    state.typography.style_override = preferences.font_override.clone();
                    state.typography.size_override_pt = preferences.font_size_pt;
                    state.commit(platform, platform.paths())?;
                    report.state = Some(state);
                }
            }
        }
        Ok(report)
    }
}

/// Looks for a v1 installation.
pub fn detect_v1(platform: &dyn Platform) -> Result<Option<V1Detection>> {
    let legacy_dir = platform
        .env_var("KISEKI_V1_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            let home = platform
                .env_var("HOME")
                .or_else(|| platform.env_var("USERPROFILE"))
                .map(PathBuf::from)?;
            let config_home = platform
                .env_var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"));
            let candidate = config_home.join("kiseki-terminal");
            Some(candidate)
        })
        .filter(|path| platform.exists(path));

    let mut configs = Vec::new();
    for path in candidate_shell_configs(platform) {
        if let Ok(Some(contents)) = platform.read_to_string(&path) {
            if contents.contains(V1_BEGIN) {
                configs.push(V1ConfigHook::hash(path));
            }
        }
    }
    for path in candidate_ghostty_configs(platform) {
        if let Ok(Some(contents)) = platform.read_to_string(&path) {
            if contents.contains(V1_BEGIN) {
                configs.push(V1ConfigHook::hash(path));
            }
        }
    }
    for path in candidate_wezterm_configs(platform) {
        if let Ok(Some(contents)) = platform.read_to_string(&path) {
            if contents.contains(V1_LUA_BEGIN) {
                configs.push(V1ConfigHook::lua(path));
            }
        }
    }

    let mut shims = Vec::new();
    if let Some(legacy) = legacy_dir.as_ref() {
        let installed = legacy.join("installed-shims.txt");
        if let Ok(Some(contents)) = platform.read_to_string(&installed) {
            for line in contents.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let path = PathBuf::from(line);
                if path.exists() {
                    shims.push(path);
                }
            }
        }
    }
    if shims.is_empty() {
        for dir in shim_dirs(platform) {
            for name in V1_SHIM_NAMES {
                let path = dir.join(name);
                if !platform.exists(&path) {
                    continue;
                }
                if let Ok(Some(contents)) = platform.read_to_string(&path) {
                    if contents.contains(V1_SHIM_MARKER) {
                        shims.push(path);
                    }
                }
            }
        }
    }

    let preferences = read_preferences(platform, legacy_dir.as_deref())?;
    let mut notes = Vec::new();
    if legacy_dir.is_some() {
        notes.push("v1 config directory found".to_string());
    }
    if !configs.is_empty() {
        notes.push(format!("{} host config(s) contain v1 hooks", configs.len()));
    }
    if !shims.is_empty() {
        notes.push(format!("{} v1 shim(s) carry the shim marker", shims.len()));
    }

    let detection = V1Detection {
        legacy_dir,
        configs,
        shims,
        preferences,
        notes,
    };
    Ok(detection.is_present().then_some(detection))
}

/// Builds the plan, including the backup location.
pub fn plan_migration(platform: &dyn Platform, detection: V1Detection) -> Result<MigrationPlan> {
    let stamp = platform
        .now_rfc3339()
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    let backup_dir = platform
        .paths()
        .journal_dir()
        .join(format!("v1-migration-{stamp}"));
    let mut steps = Vec::new();
    if let Some(source) = detection.legacy_dir.as_ref() {
        steps.push(MigrationStep::BackupDirectory {
            source: source.clone(),
        });
    }
    for hook in &detection.configs {
        steps.push(MigrationStep::RemoveMarkerBlock {
            target: hook.path.clone(),
            begin: hook.begin.clone(),
            end: hook.end.clone(),
        });
    }
    for shim in &detection.shims {
        steps.push(MigrationStep::DeleteShim { path: shim.clone() });
    }
    steps.push(MigrationStep::ImportPreferences {
        preferences: detection.preferences.clone(),
    });
    Ok(MigrationPlan {
        detection,
        backup_dir,
        steps,
    })
}

/// True when a path looks like a v1 shim (content marker present).
pub fn is_v1_shim(platform: &dyn Platform, path: &Path) -> bool {
    platform
        .read_to_string(path)
        .ok()
        .flatten()
        .is_some_and(|contents| contents.contains(V1_SHIM_MARKER))
}

fn candidate_shell_configs(platform: &dyn Platform) -> Vec<PathBuf> {
    let Some(home) = platform
        .env_var("HOME")
        .or_else(|| platform.env_var("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let zdotdir = platform
        .env_var("ZDOTDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.clone());
    let mut paths = vec![
        zdotdir.join(".zshrc"),
        home.join(".bashrc"),
        home.join(".bash_profile"),
    ];
    match platform.os() {
        OsKind::Windows => {
            paths.push(
                home.join("Documents")
                    .join("PowerShell")
                    .join("Microsoft.PowerShell_profile.ps1"),
            );
            paths.push(
                home.join("Documents")
                    .join("WindowsPowerShell")
                    .join("Microsoft.PowerShell_profile.ps1"),
            );
        }
        OsKind::MacOs | OsKind::Linux => {
            paths.push(
                home.join(".config")
                    .join("powershell")
                    .join("Microsoft.PowerShell_profile.ps1"),
            );
        }
    }
    paths
}

fn shim_dirs(platform: &dyn Platform) -> Vec<PathBuf> {
    let home = platform
        .env_var("HOME")
        .or_else(|| platform.env_var("USERPROFILE"))
        .map(PathBuf::from);
    match home {
        Some(home) => vec![
            home.join(".local").join("bin"),
            home.join(".kiseki-terminal").join("bin"),
            home.join(".config").join("kiseki-terminal").join("bin"),
        ],
        None => Vec::new(),
    }
}

fn read_preferences(platform: &dyn Platform, legacy: Option<&Path>) -> Result<V1Preferences> {
    let Some(legacy) = legacy else {
        return Ok(V1Preferences::default());
    };
    let read = |name: &str| -> Result<Option<String>> {
        platform.read_to_string(&legacy.join(name))
    };
    let theme = read("current-theme.txt")?.map(|value| value.trim().to_string());
    let live = read("live-mode.txt")?
        .map(|value| matches!(value.trim(), "on" | "1" | "true" | "enabled"))
        .unwrap_or(false);
    let crt = read("crt-mode.txt")?
        .map(|value| matches!(value.trim(), "on" | "1" | "true" | "enabled"))
        .unwrap_or(false);
    let bg_opacity = read("bg-opacity.txt")?.and_then(|value| {
        let value = value.trim();
        // v1 stored `off` for "no wallpaper", not a numeric opacity.
        if value.eq_ignore_ascii_case("off") {
            return Some(0.0);
        }
        value.parse::<f64>().ok().map(|value| value.clamp(0.0, 1.0))
    });
    let font_prefs = read("font-override.json")?.and_then(|value| {
        let parsed: serde_json::Value = serde_json::from_str(&value).ok()?;
        let style = parsed
            .get("style")
            .and_then(serde_json::Value::as_str)
            .or_else(|| parsed.get("face").and_then(serde_json::Value::as_str))
            .or_else(|| parsed.as_str())
            .map(expand_font_preset);
        let size = parsed.get("size").and_then(serde_json::Value::as_f64);
        Some((style, size))
    });
    let font_override = font_prefs.as_ref().and_then(|(style, _)| style.clone());
    let font_size_pt = font_prefs
        .as_ref()
        .and_then(|(_, size)| *size)
        .map(|size| size.clamp(6.0, 36.0));
    let update_check = read("update-check.txt")?
        .map(|value| !matches!(value.trim(), "muted" | "off" | "0" | "false"))
        .unwrap_or(false);
    Ok(V1Preferences {
        theme,
        live,
        crt,
        bg_opacity,
        font_override,
        font_size_pt,
        update_check,
    })
}

/// v1 font presets mapped to the v2 catalog family names.
const V1_FONT_PRESETS: &[(&str, &str)] = &[
    ("8bit", "DepartureMono Nerd Font"),
    ("code", "Pixel Code"),
    ("maple", "Maple Mono NF CN"),
    ("modern", "JetBrainsMono NF"),
];

/// Expands a v1 preset (`8bit`, `code`, `maple`, `modern`) to a catalog family.
fn expand_font_preset(style: &str) -> String {
    let normalized = style.trim().to_ascii_lowercase();
    V1_FONT_PRESETS
        .iter()
        .find(|(preset, _)| *preset == normalized)
        .map(|(_, family)| (*family).to_string())
        .unwrap_or_else(|| style.trim().to_string())
}

/// Copies a directory tree into `destination`, rejecting symlinks.
fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(destination).map_err(|error| KisekiError::io(destination, error))?;
    for entry in std::fs::read_dir(source).map_err(|error| KisekiError::io(source, error))? {
        let entry = entry.map_err(|error| KisekiError::io(source, error))?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| KisekiError::io(&path, error))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let target = destination.join(&name);
        if metadata.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            std::fs::copy(&path, &target).map_err(|error| KisekiError::io(&path, error))?;
        }
    }
    Ok(())
}

/// True when a detection would touch a read-only shell config (doctor hint).
pub fn has_read_only_targets(platform: &dyn Platform, detection: &V1Detection) -> bool {
    detection.configs.iter().any(|hook| {
        matches!(
            platform.inspect(&hook.path),
            PathKind::ReadOnlyFile | PathKind::ReadOnlySymlink(_)
        )
    })
}

/// Copies `source` into `dir` under a 1-based step-prefixed name, keeping two configs
/// with the same basename distinct, and records the mapping in
/// `backup-manifest.txt`.
fn backup_file(
    platform: &dyn Platform,
    backup_root: &Path,
    dir: &Path,
    source: &Path,
    step_index: usize,
) -> Result<PathBuf> {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let destination = dir.join(format!("{step_index:02}-{name}"));
    platform.create_dir_all(dir)?;
    if let Some(contents) = platform.read_bytes(source)? {
        match platform.write_preserving_symlinks(&destination, &contents)? {
            WriteOutcome::Written { .. } => {}
            WriteOutcome::SkippedReadOnly { reason, .. } => {
                return Err(KisekiError::State(format!(
                    "cannot back up {}: {reason}",
                    source.display()
                )));
            }
        }
    }

    let manifest_path = backup_root.join("backup-manifest.txt");
    let mut manifest = platform.read_to_string(&manifest_path)?.unwrap_or_default();
    let relative = destination.strip_prefix(backup_root).unwrap_or(&destination);
    manifest.push_str(&format!(
        "{}\t{}\n",
        relative.to_string_lossy().replace('\\', "/"),
        source.display()
    ));
    match platform.write_preserving_symlinks(&manifest_path, manifest.as_bytes())? {
        WriteOutcome::Written { .. } => {}
        WriteOutcome::SkippedReadOnly { reason, .. } => {
            return Err(KisekiError::State(format!(
                "cannot write {}: {reason}",
                manifest_path.display()
            )));
        }
    }
    Ok(destination)
}

/// v1 Ghostty configs in the App Support and XDG locations.
fn candidate_ghostty_configs(platform: &dyn Platform) -> Vec<PathBuf> {
    let Some(home) = platform
        .env_var("HOME")
        .or_else(|| platform.env_var("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let config_home = platform
        .env_var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let mut paths = Vec::new();
    if platform.os() == OsKind::MacOs {
        let app_support = home
            .join("Library")
            .join("Application Support")
            .join("com.mitchellh.ghostty");
        paths.push(app_support.join("config.ghostty"));
        paths.push(app_support.join("config"));
    }
    paths.push(config_home.join("ghostty").join("config.ghostty"));
    paths.push(config_home.join("ghostty").join("config"));
    paths
}

/// v1 WezTerm configs (`~/.wezterm.lua` and the XDG location).
fn candidate_wezterm_configs(platform: &dyn Platform) -> Vec<PathBuf> {
    let Some(home) = platform
        .env_var("HOME")
        .or_else(|| platform.env_var("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let config_home = platform
        .env_var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    vec![
        home.join(".wezterm.lua"),
        config_home.join("wezterm").join("wezterm.lua"),
    ]
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

    fn seed_v1(platform: &MockPlatform, root: &Path) -> PathBuf {
        let legacy = root.join("legacy");
        platform.create_dir_all(&legacy).unwrap();
        platform
            .write_preserving_symlinks(&legacy.join("current-theme.txt"), b"kyoto-sakura\n")
            .unwrap();
        platform
            .write_preserving_symlinks(&legacy.join("live-mode.txt"), b"on\n")
            .unwrap();
        platform
            .write_preserving_symlinks(&legacy.join("crt-mode.txt"), b"on\n")
            .unwrap();
        platform
            .write_preserving_symlinks(&legacy.join("bg-opacity.txt"), b"0.55\n")
            .unwrap();
        platform
            .write_preserving_symlinks(&legacy.join("font-override.json"), br#"{"face":"Maple Mono NF CN"}"#)
            .unwrap();
        platform
            .write_preserving_symlinks(&legacy.join("update-check.txt"), b"on\n")
            .unwrap();
        platform.set_env("KISEKI_V1_HOME", &legacy.to_string_lossy());

        let zshrc = root.join(".zshrc");
        platform
            .write_preserving_symlinks(
                &zshrc,
                format!("# user\nexport X=1\n{V1_BEGIN}\nsource ~/.kiseki-terminal/env.zsh\n{V1_END}\n").as_bytes(),
            )
            .unwrap();
        platform.set_env("KISEKI_ZSHRC", &zshrc.to_string_lossy());
        // Our ZshAdapter reads KISEKI_ZSHRC, but detection scans HOME.
        platform.set_env("HOME", &root.to_string_lossy());
        std::fs::write(
            root.join(".zshrc"),
            format!("# user\nexport X=1\n{V1_BEGIN}\nsource ~/.kiseki-terminal/env.zsh\n{V1_END}\n"),
        )
        .unwrap();

        let shim = root.join("bin").join("theme");
        platform.create_dir_all(shim.parent().unwrap()).unwrap();
        platform
            .write_preserving_symlinks(&shim, format!("#!/bin/sh\n{V1_SHIM_MARKER}\nexec kiseki use \"$@\"\n").as_bytes())
            .unwrap();
        platform
            .write_preserving_symlinks(
                &legacy.join("installed-shims.txt"),
                shim.to_string_lossy().as_bytes(),
            )
            .unwrap();
        legacy
    }

    #[test]
    fn detects_legacy_directory_preferences_and_hooks() {
        let (dir, platform) = platform();
        let legacy = seed_v1(&platform, dir.path());
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        assert_eq!(detection.legacy_dir.as_deref(), Some(legacy.as_path()));
        assert_eq!(detection.preferences.theme.as_deref(), Some("kyoto-sakura"));
        assert!(detection.preferences.live);
        assert!(detection.preferences.crt);
        assert_eq!(detection.preferences.bg_opacity, Some(0.55));
        assert_eq!(
            detection.preferences.font_override.as_deref(),
            Some("Maple Mono NF CN")
        );
        assert!(!detection.configs.is_empty());
        assert_eq!(detection.shims.len(), 1);
    }

    #[test]
    fn nothing_to_migrate_returns_none() {
        let (_dir, platform) = platform();
        assert!(detect_v1(&platform).unwrap().is_none());
    }

    #[test]
    fn plan_is_reviewable_and_executes_reversibly() {
        let (dir, platform) = platform();
        let _legacy = seed_v1(&platform, dir.path());
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        let plan = plan_migration(&platform, detection).unwrap();
        assert!(plan.summary().iter().any(|line| line.contains("back up legacy directory")));
        assert!(plan.backup_dir.starts_with(platform.paths().journal_dir()));

        let report = plan.execute(&platform).unwrap();
        assert_eq!(report.removed_marker_blocks.len(), 1);
        assert_eq!(report.deleted_shims.len(), 1);
        assert!(report.backup_dir.join("legacy").join("current-theme.txt").exists());
        let zshrc = std::fs::read_to_string(dir.path().join(".zshrc")).unwrap();
        assert!(zshrc.contains("export X=1"));
        assert!(!zshrc.contains(V1_BEGIN));
        assert!(!dir.path().join("bin").join("theme").exists());

        let state = report.state.unwrap();
        // `kyoto-sakura` is bundled as an official theme, so migration preserves it.
        assert_eq!(state.active_theme.as_str(), "kyoto-sakura");
        assert_eq!(state.rendering.profile, RenderingProfile::Balanced);
        assert!(state.rendering.crt);
        assert_eq!(state.rendering.bg_opacity_override, Some(0.55));
        let loaded = State::load(&platform, platform.paths()).unwrap().unwrap();
        assert_eq!(loaded.active_theme.as_str(), "kyoto-sakura");
    }

    #[test]
    fn unavailable_v1_theme_falls_back_to_nerv_eva() {
        let (dir, platform) = platform();
        let legacy = seed_v1(&platform, dir.path());
        platform
            .write_preserving_symlinks(&legacy.join("current-theme.txt"), b"retired-v1-theme\n")
            .unwrap();
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        let report = plan_migration(&platform, detection)
            .unwrap()
            .execute(&platform)
            .unwrap();
        let state = report.state.unwrap();
        assert_eq!(state.active_theme.as_str(), "nerv-eva");
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("retired-v1-theme")));
    }

    #[test]
    fn execute_is_idempotent() {
        let (dir, platform) = platform();
        let legacy = seed_v1(&platform, dir.path());
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        let plan = plan_migration(&platform, detection).unwrap();
        plan.execute(&platform).unwrap();
        let second = plan.execute(&platform).unwrap();
        assert!(second.removed_marker_blocks.is_empty());
        assert!(second.deleted_shims.is_empty());
        assert!(legacy.exists(), "backup source is never deleted");
    }

    #[test]
    fn shims_without_marker_are_kept() {
        let (dir, platform) = platform();
        let _legacy = seed_v1(&platform, dir.path());
        let shim = dir.path().join("bin").join("theme");
        platform
            .write_preserving_symlinks(&shim, b"#!/bin/sh\necho user script\n")
            .unwrap();
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        let plan = plan_migration(&platform, detection).unwrap();
        let report = plan.execute(&platform).unwrap();
        assert!(report.deleted_shims.is_empty());
        assert!(shim.exists());
        assert!(report.notes.iter().any(|note| note.contains("no v1 shim marker")));
    }

    #[test]
    fn preferences_are_clamped_and_validated() {
        let (dir, platform) = platform();
        let legacy = seed_v1(&platform, dir.path());
        platform
            .write_preserving_symlinks(&legacy.join("bg-opacity.txt"), b"2.5\n")
            .unwrap();
        platform
            .write_preserving_symlinks(&legacy.join("current-theme.txt"), b"Not A Theme\n")
            .unwrap();
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        assert_eq!(detection.preferences.bg_opacity, Some(1.0));
        let plan = plan_migration(&platform, detection).unwrap();
        let report = plan.execute(&platform).unwrap();
        assert_eq!(report.state.unwrap().active_theme.as_str(), "nerv-eva");
    }

    #[test]
    fn symlinked_legacy_files_are_not_copied() {
        let (dir, platform) = platform();
        let legacy = seed_v1(&platform, dir.path());
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", legacy.join("leak.txt")).unwrap();
        #[cfg(windows)]
        std::fs::write(legacy.join("leak.txt"), "no symlink support").unwrap();
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        let plan = plan_migration(&platform, detection).unwrap();
        let report = plan.execute(&platform).unwrap();
        assert!(!report.backup_dir.join("legacy").join("leak.txt").exists());
    }

    #[test]
    fn v1_shim_names_match_the_documented_eight() {
        assert_eq!(V1_SHIM_NAMES.len(), 8);
        assert!(V1_SHIM_NAMES.contains(&"themes"));
        assert!(V1_SHIM_NAMES.contains(&"fetch"));
    }

    #[test]
    fn v1_ghostty_and_wezterm_hooks_are_detected_and_unhooked() {
        let (dir, platform) = platform();
        let _legacy = seed_v1(&platform, dir.path());
        platform.set_env("HOME", &dir.path().to_string_lossy());

        let ghostty_xdg = dir.path().join(".config/ghostty/config.ghostty");
        platform.create_dir_all(ghostty_xdg.parent().unwrap()).unwrap();
        std::fs::write(
            &ghostty_xdg,
            format!("font-size = 13\n{V1_BEGIN}\nconfig-file = ~/.kiseki-terminal/ghostty.conf\n{V1_END}\n"),
        )
        .unwrap();
        let ghostty_app = dir
            .path()
            .join("Library/Application Support/com.mitchellh.ghostty/config");
        platform.create_dir_all(ghostty_app.parent().unwrap()).unwrap();
        std::fs::write(
            &ghostty_app,
            format!("{V1_BEGIN}\nconfig-file = ~/.kiseki-terminal/ghostty.conf\n{V1_END}\n"),
        )
        .unwrap();
        let wezterm = dir.path().join(".wezterm.lua");
        std::fs::write(
            &wezterm,
            format!(
                "local wezterm = require 'wezterm'\n{V1_LUA_BEGIN}\nreturn dofile('~/.kiseki-terminal/wezterm.lua')\n{V1_LUA_END}\n"
            ),
        )
        .unwrap();
        let wezterm_xdg = dir.path().join(".config/wezterm/wezterm.lua");
        platform.create_dir_all(wezterm_xdg.parent().unwrap()).unwrap();
        std::fs::write(
            &wezterm_xdg,
            format!("{V1_LUA_BEGIN}\nrequire('kiseki')\n{V1_LUA_END}\n"),
        )
        .unwrap();

        let detection = detect_v1(&platform).unwrap().unwrap();
        let paths = detection.config_paths();
        assert!(paths.contains(&&ghostty_xdg), "{paths:?}");
        assert!(paths.contains(&&ghostty_app), "{paths:?}");
        assert!(paths.contains(&&wezterm), "{paths:?}");
        assert!(paths.contains(&&wezterm_xdg), "{paths:?}");

        let plan = plan_migration(&platform, detection).unwrap();
        let report = plan.execute(&platform).unwrap();
        assert!(report.backed_up.iter().any(|path| path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".wezterm.lua"))));
        assert!(report
            .backed_up
            .iter()
            .any(|path| path.parent().is_some_and(|parent| parent.ends_with("hosts"))));
        assert!(report
            .backed_up
            .iter()
            .any(|path| path.parent().is_some_and(|parent| parent.ends_with("shims"))));

        let ghostty = std::fs::read_to_string(&ghostty_xdg).unwrap();
        assert!(ghostty.contains("font-size = 13"));
        assert!(!ghostty.contains(V1_BEGIN));
        assert!(!std::fs::read_to_string(&ghostty_app).unwrap().contains(V1_BEGIN));
        let lua = std::fs::read_to_string(&wezterm).unwrap();
        assert!(lua.contains("require 'wezterm'"));
        assert!(!lua.contains(V1_LUA_BEGIN));
        assert!(!std::fs::read_to_string(&wezterm_xdg).unwrap().contains(V1_LUA_BEGIN));

        // The hook files were preserved byte-for-byte in the backup folder.
        let backup = report
            .backup_dir
            .join("hosts")
            .read_dir()
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".wezterm.lua")))
            .expect("the wezterm config was backed up");
        assert!(std::fs::read_to_string(&backup).unwrap().contains(V1_LUA_BEGIN));
    }

    #[test]
    fn render_only_keeps_v1_host_hooks_but_still_imports_preferences() {
        let (dir, platform) = platform();
        let _legacy = seed_v1(&platform, dir.path());
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        let plan = plan_migration(&platform, detection).unwrap();
        let report = plan.execute_with_options(&platform, true).unwrap();

        assert!(report.removed_marker_blocks.is_empty());
        assert!(report.deleted_shims.is_empty());
        assert!(report.notes.iter().any(|note| note.contains("render-only")));
        let zshrc = std::fs::read_to_string(dir.path().join(".zshrc")).unwrap();
        assert!(zshrc.contains(V1_BEGIN), "render-only must not edit host files");
        assert!(dir.path().join("bin").join("theme").exists());
        // The legacy backup and the preference import still happen.
        assert!(report.backup_dir.join("legacy").join("current-theme.txt").exists());
        let state = report.state.expect("preferences are imported in render-only mode");
        assert_eq!(state.active_theme.as_str(), "kyoto-sakura");
    }

    #[test]
    fn a_second_migration_never_rewinds_a_newer_v2_state() {
        let (dir, platform) = platform();
        let _legacy = seed_v1(&platform, dir.path());
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        let plan = plan_migration(&platform, detection).unwrap();
        plan.execute(&platform).unwrap();

        // A later v2 command changes the state (and bumps the generation).
        let mut state = State::load(&platform, platform.paths()).unwrap().unwrap();
        let generation = state.generation;
        state.rendering.crt = false;
        state.typography.style_override = Some("JetBrainsMono NF".to_string());
        state.commit(&platform, platform.paths()).unwrap();
        let generation_after = generation + 1;

        let detection = detect_v1(&platform).unwrap().unwrap();
        let report = plan_migration(&platform, detection)
            .unwrap()
            .execute(&platform)
            .unwrap();
        assert!(report.state.is_none());
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("existing v2 state.json")));
        let loaded = State::load(&platform, platform.paths()).unwrap().unwrap();
        assert_eq!(loaded.generation, generation_after);
        assert!(!loaded.rendering.crt);
        assert_eq!(
            loaded.typography.style_override.as_deref(),
            Some("JetBrainsMono NF")
        );
    }

    #[test]
    fn colliding_backup_basenames_get_distinct_step_prefixed_names() {
        let (dir, platform) = platform();
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let first = dir.path().join("powershell7").join("Microsoft.PowerShell_profile.ps1");
        let second = dir.path().join("powershell5").join("Microsoft.PowerShell_profile.ps1");
        let shim_first = dir.path().join("one").join("theme");
        let shim_second = dir.path().join("two").join("theme");
        for (path, body) in [
            (&first, format!("# user one\n{V1_BEGIN}\nold one\n{V1_END}\n")),
            (&second, format!("# user two\n{V1_BEGIN}\nold two\n{V1_END}\n")),
            (
                &shim_first,
                format!("#!/bin/sh\n{V1_SHIM_MARKER}\nexec kiseki use \"$@\"\n"),
            ),
            (
                &shim_second,
                format!("#!/bin/sh\n{V1_SHIM_MARKER}\nexec kiseki use \"$@\"\n"),
            ),
        ] {
            platform.create_dir_all(path.parent().unwrap()).unwrap();
            platform.write_preserving_symlinks(path, body.as_bytes()).unwrap();
        }
        let detection = V1Detection {
            legacy_dir: None,
            configs: vec![V1ConfigHook::hash(first.clone()), V1ConfigHook::hash(second.clone())],
            shims: vec![shim_first.clone(), shim_second.clone()],
            preferences: V1Preferences::default(),
            notes: Vec::new(),
        };
        let plan = plan_migration(&platform, detection).unwrap();
        let report = plan.execute(&platform).unwrap();

        // Steps 1/2 are the configs, 3/4 the shims: the prefix keeps every
        // same-basename backup distinct.
        let hosts = report.backup_dir.join("hosts");
        let shims = report.backup_dir.join("shims");
        assert!(hosts.join("01-Microsoft.PowerShell_profile.ps1").is_file());
        assert!(hosts.join("02-Microsoft.PowerShell_profile.ps1").is_file());
        assert!(shims.join("03-theme").is_file());
        assert!(shims.join("04-theme").is_file());
        assert_eq!(report.backed_up.len(), 4);
        assert_ne!(
            std::fs::read(hosts.join("01-Microsoft.PowerShell_profile.ps1")).unwrap(),
            std::fs::read(hosts.join("02-Microsoft.PowerShell_profile.ps1")).unwrap()
        );

        let manifest = std::fs::read_to_string(report.backup_dir.join("backup-manifest.txt")).unwrap();
        assert!(manifest.contains("hosts/01-Microsoft.PowerShell_profile.ps1"), "{manifest}");
        assert!(manifest.contains(&first.display().to_string()), "{manifest}");
        assert!(manifest.contains(&second.display().to_string()), "{manifest}");
        assert!(manifest.contains("shims/03-theme"), "{manifest}");
        assert!(!std::fs::read_to_string(&first).unwrap().contains(V1_BEGIN));
        assert!(!std::fs::read_to_string(&second).unwrap().contains(V1_BEGIN));
        assert!(!shim_first.exists());
        assert!(!shim_second.exists());
    }

    #[test]
    fn a_read_only_host_config_aborts_before_being_edited() {
        let (dir, platform) = platform();
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let config = dir.path().join(".zshrc");
        std::fs::write(
            &config,
            format!("# user\n{V1_BEGIN}\nsource old\n{V1_END}\n"),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&config).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&config, permissions).unwrap();

        let detection = V1Detection {
            legacy_dir: None,
            configs: vec![V1ConfigHook::hash(config.clone())],
            shims: Vec::new(),
            preferences: V1Preferences::default(),
            notes: Vec::new(),
        };
        let plan = plan_migration(&platform, detection).unwrap();
        let error = plan.execute(&platform).unwrap_err();
        assert!(error.to_string().contains("read-only"), "{error}");
        assert!(std::fs::read_to_string(&config).unwrap().contains(V1_BEGIN));
        // The backup was still taken before the abort.
        assert!(plan.backup_dir.join("hosts").join("01-.zshrc").is_file());
    }

    #[test]
    fn bg_opacity_off_and_font_presets_are_expanded() {
        let (dir, platform) = platform();
        let legacy = seed_v1(&platform, dir.path());
        platform
            .write_preserving_symlinks(&legacy.join("bg-opacity.txt"), b"OFF\n")
            .unwrap();
        for (preset, family) in [
            ("8bit", "DepartureMono Nerd Font"),
            ("code", "Pixel Code"),
            ("maple", "Maple Mono NF CN"),
            ("modern", "JetBrainsMono NF"),
        ] {
            platform
                .write_preserving_symlinks(
                    &legacy.join("font-override.json"),
                    format!(r#"{{"style":"{preset}"}}"#).as_bytes(),
                )
                .unwrap();
            platform.set_env("HOME", &dir.path().to_string_lossy());
            let detection = detect_v1(&platform).unwrap().unwrap();
            assert_eq!(detection.preferences.bg_opacity, Some(0.0));
            assert_eq!(
                detection.preferences.font_override.as_deref(),
                Some(family),
                "{preset}"
            );
        }
    }

    #[test]
    fn font_override_reads_style_and_size() {
        let (dir, platform) = platform();
        let legacy = seed_v1(&platform, dir.path());
        platform
            .write_preserving_symlinks(
                &legacy.join("font-override.json"),
                br#"{"style":"Maple Mono NF CN","size":13.5,"face":"Ignored"}"#,
            )
            .unwrap();
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        assert_eq!(
            detection.preferences.font_override.as_deref(),
            Some("Maple Mono NF CN")
        );
        assert_eq!(detection.preferences.font_size_pt, Some(13.5));
        let plan = plan_migration(&platform, detection).unwrap();
        let report = plan.execute(&platform).unwrap();
        let state = report.state.unwrap();
        assert_eq!(state.typography.style_override.as_deref(), Some("Maple Mono NF CN"));
        assert_eq!(state.typography.size_override_pt, Some(13.5));
    }

    #[test]
    fn font_override_falls_back_to_face() {
        let (dir, platform) = platform();
        let _legacy = seed_v1(&platform, dir.path());
        platform.set_env("HOME", &dir.path().to_string_lossy());
        let detection = detect_v1(&platform).unwrap().unwrap();
        assert_eq!(
            detection.preferences.font_override.as_deref(),
            Some("Maple Mono NF CN")
        );
        assert_eq!(detection.preferences.font_size_pt, None);
    }
}
