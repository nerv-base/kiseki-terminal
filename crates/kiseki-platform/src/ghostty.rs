//! Ghostty helper integration (RFC-0003 §4).
//!
//! Ghostty 1.3+ reloads configuration on `SIGUSR2` and ships a config
//! validator. Both are invoked through the [`Platform`] trait so tests can
//! assert exactly which command would run without spawning anything.

use std::path::{Path, PathBuf};

use kiseki_types::{Diagnostic, KisekiError, OsKind, Result};

use crate::platform::Platform;

/// What happened when a reload was attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// A signal or native reload request was delivered.
    Signalled { detail: String },
    /// The host watches its config files and needs no signal.
    AutoReload { detail: String },
    /// The host is not running or not installed; nothing to do.
    NotRunning { detail: String },
    /// The host cannot be asked to reload; this is a non-fatal skip.
    Skipped { detail: String },
    /// The reload was attempted and failed.
    Failed { detail: String },
}

impl ReloadOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(
            self,
            Self::Signalled { .. } | Self::AutoReload { .. } | Self::NotRunning { .. } | Self::Skipped { .. }
        )
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Signalled { detail }
            | Self::AutoReload { detail }
            | Self::NotRunning { detail }
            | Self::Skipped { detail }
            | Self::Failed { detail } => detail,
        }
    }
}

/// Result of `ghostty +validate-config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationOutcome {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Finds the Ghostty executable (`KISEKI_GHOSTTY_BIN` override or `platform.which("ghostty")`,
/// which on macOS also probes `/Applications/Ghostty.app/Contents/MacOS/ghostty`).
pub fn find_binary(platform: &dyn Platform) -> Option<PathBuf> {
    if let Some(explicit) = platform.env_var("KISEKI_GHOSTTY_BIN") {
        let path = PathBuf::from(explicit);
        if platform.exists(&path) {
            return Some(path);
        }
    }
    platform.which("ghostty")
}

/// Runs `ghostty +validate-config --config-file=<path>`.
///
/// Ghostty 1.4.0 rejects two separate arguments, so the flag and its value must
/// be passed as a single `--config-file=<path>` token.
///
/// Returns `Ok(None)` when the binary is not installed, which is not an error:
/// `doctor` reports the skip instead.
pub fn validate_config(platform: &dyn Platform, config_path: &Path) -> Result<Option<ValidationOutcome>> {
    let Some(binary) = find_binary(platform) else {
        return Ok(None);
    };
    let config = config_path.to_string_lossy().into_owned();
    let argument = format!("--config-file={config}");
    let output = platform.run(&binary.to_string_lossy(), &["+validate-config", &argument])?;
    Ok(Some(ValidationOutcome {
        ok: output.success(),
        stdout: output.stdout,
        stderr: output.stderr,
    }))
}

const GHOSTTY_VERSION_CACHE_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedGhosttyVersion {
    version: u32,
    path: String,
    len: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    major: u64,
    minor: u64,
}

fn binary_stamp(path: &Path) -> Option<(u64, u64, u32)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some((metadata.len(), duration.as_secs(), duration.subsec_nanos()))
}

/// Parses `ghostty --version` into `(major, minor)`.
///
/// Accepts the shapes Ghostty actually prints: `Ghostty 1.3.1`, `ghostty
/// 1.4.0-dev+...`, and a bare `1.2.3`. Returns `None` when the binary is
/// unavailable or its version cannot be understood.
pub fn version_of(platform: &dyn Platform) -> Option<(u64, u64)> {
    let binary = find_binary(platform)?;
    let use_cache = !platform.is_mock();
    let stamp = use_cache.then(|| binary_stamp(&binary)).flatten();
    let cache_path = use_cache.then(|| platform.paths().cache_dir().join("ghostty-version.json"));
    let binary_key = binary.to_string_lossy();
    if let (Some((len, mtime_secs, mtime_nanos)), Some(path)) = (stamp, cache_path.as_deref()) {
        if let Some(cached) = std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str::<CachedGhosttyVersion>(&raw).ok())
        {
            if cached.version == GHOSTTY_VERSION_CACHE_VERSION
                && cached.path == binary_key
                && cached.len == len
                && cached.mtime_secs == mtime_secs
                && cached.mtime_nanos == mtime_nanos
            {
                return Some((cached.major, cached.minor));
            }
        }
    }
    let output = platform.run(&binary_key, &["--version"]).ok()?;
    if !output.success() {
        return None;
    }
    let parsed = parse_version(&output.stdout).or_else(|| parse_version(&output.stderr))?;
    if let (Some((len, mtime_secs, mtime_nanos)), Some(path)) = (stamp, cache_path.as_deref()) {
        let entry = CachedGhosttyVersion {
            version: GHOSTTY_VERSION_CACHE_VERSION,
            path: binary_key.into_owned(),
            len,
            mtime_secs,
            mtime_nanos,
            major: parsed.0,
            minor: parsed.1,
        };
        if let Ok(encoded) = serde_json::to_vec(&entry) {
            let _ = crate::fs::atomic_write_with_sync(
                path,
                &encoded,
                crate::fs::SyncMode::AtomicNoSync,
            );
        }
    }
    Some(parsed)
}

/// Extracts the first `major.minor` pair from a version banner.
fn parse_version(raw: &str) -> Option<(u64, u64)> {
    for token in raw.split(|ch: char| !(ch.is_ascii_digit() || ch == '.')) {
        let mut parts = token.split('.');
        let (Some(major), Some(minor)) = (parts.next(), parts.next()) else {
            continue;
        };
        if major.is_empty() || minor.is_empty() {
            continue;
        }
        if let (Ok(major), Ok(minor)) = (major.parse::<u64>(), minor.parse::<u64>()) {
            return Some((major, minor));
        }
    }
    None
}

/// The first Ghostty release whose macOS build reloads on `SIGUSR2`.
pub const MACOS_SIGNAL_RELOAD_VERSION: (u64, u64) = (1, 3);

/// Sends `SIGUSR2` to every running Ghostty process.
///
/// `pkill` is used instead of `libc::kill` because it can match every running
/// instance, and because it keeps this crate free of `unsafe`. macOS builds
/// before 1.3 ignore the signal, so it is only sent when `ghostty --version`
/// proves the host understands it.
pub fn reload(platform: &dyn Platform) -> ReloadOutcome {
    if platform.os() == OsKind::Windows {
        return ReloadOutcome::AutoReload {
            detail: "Ghostty on Windows reloads configuration automatically".to_string(),
        };
    }
    if platform.os() == OsKind::MacOs {
        match version_of(platform) {
            Some(version) if version >= MACOS_SIGNAL_RELOAD_VERSION => {}
            Some((major, minor)) => {
                return ReloadOutcome::Skipped {
                    detail: format!(
                        "ghostty {major}.{minor} predates SIGUSR2 reload support ({}.{}); config will load on next launch",
                        MACOS_SIGNAL_RELOAD_VERSION.0, MACOS_SIGNAL_RELOAD_VERSION.1
                    ),
                }
            }
            None => {
                return ReloadOutcome::Skipped {
                    detail: "ghostty version is unknown; no reload signal was sent".to_string(),
                }
            }
        }
    } else if platform.which("pkill").is_none() {
        return ReloadOutcome::NotRunning {
            detail: "pkill is unavailable, so no reload signal was sent".to_string(),
        };
    }
    // On macOS, BSD `pkill` excludes its own ancestor processes by default.
    // Because users typically run `kiseki` *inside* Ghostty, `-a` is required
    // so Ghostty itself receives `SIGUSR2`. Linux procps `pkill` does not
    // exclude ancestors and uses a different meaning for `-a`.
    let args: &[&str] = if platform.os() == OsKind::MacOs {
        &["-USR2", "-a", "-x", "ghostty"]
    } else {
        &["-USR2", "-x", "ghostty"]
    };
    match platform.run("pkill", args) {
        Ok(output) if output.success() => ReloadOutcome::Signalled {
            detail: "sent SIGUSR2 to ghostty".to_string(),
        },
        Ok(output) if output.status == 1 => ReloadOutcome::NotRunning {
            detail: "no running ghostty process found".to_string(),
        },
        Ok(output) => ReloadOutcome::Failed {
            detail: format!(
                "pkill exited with {}: {}",
                output.status,
                output.stderr.trim()
            ),
        },
        Err(error) => ReloadOutcome::Failed {
            detail: error.to_string(),
        },
    }
}

/// The conventional Ghostty config location for the host.
///
/// Ghostty 1.4 reads `config.ghostty` next to `config`, so the probe follows the
/// v1 precedence order and prefers a file that already exists:
///
/// macOS: `<App Support>/config.ghostty`, `<App Support>/config`,
///        `$XDG_CONFIG_HOME/ghostty/config.ghostty`, `.../config`
/// Linux: `$XDG_CONFIG_HOME/ghostty/config.ghostty`, `.../config`
///
/// The first existing *non-empty* file wins, then the first existing file, and
/// finally the `config.ghostty` default.
pub fn default_config_path(platform: &dyn Platform) -> Option<PathBuf> {
    if let Some(explicit) = platform.env_var("KISEKI_GHOSTTY_CONFIG") {
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
    let candidates = match platform.os() {
        OsKind::MacOs => {
            let app_support = home
                .join("Library")
                .join("Application Support")
                .join("com.mitchellh.ghostty");
            vec![
                app_support.join("config.ghostty"),
                app_support.join("config"),
                config_home.join("ghostty").join("config.ghostty"),
                config_home.join("ghostty").join("config"),
            ]
        }
        OsKind::Linux => vec![
            config_home.join("ghostty").join("config.ghostty"),
            config_home.join("ghostty").join("config"),
        ],
        OsKind::Windows => return None,
    };

    let non_empty = |path: &PathBuf| {
        platform
            .read_to_string(path)
            .ok()
            .flatten()
            .is_some_and(|text| !text.trim().is_empty())
    };
    for path in &candidates {
        if non_empty(path) {
            return Some(path.clone());
        }
    }
    for path in &candidates {
        if platform.exists(path) {
            return Some(path.clone());
        }
    }
    candidates.into_iter().next()
}

/// Converts a failed validation into a blocking diagnostic.
pub fn validation_diagnostic(config_path: &Path, outcome: &ValidationOutcome) -> Option<Diagnostic> {
    if outcome.ok {
        return None;
    }
    Some(
        Diagnostic::error(
            "adapter.ghostty.invalid-config",
            format!(
                "ghostty rejected {}: {}",
                config_path.display(),
                outcome.stderr.trim()
            ),
        )
        .with_hint("run `ghostty +validate-config --config-file=<path>` to see the full report"),
    )
}

/// Builds the one-line include that mounts the generated config.
///
/// `config-file = ?` makes the include optional so a missing generated file
/// cannot make Ghostty refuse to start.
pub fn include_line(generated_config: &Path) -> Result<String> {
    let value = generated_config.to_string_lossy();
    if value.contains('\n') || value.contains('\r') || value.contains('\0') {
        return Err(KisekiError::compiler("ghostty include path contains a newline"));
    }
    Ok(format!("config-file = ?{value}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{CommandOutput, MockPlatform};

    const GHOSTTY_BIN: &str = "/opt/homebrew/bin/ghostty";

    fn platform() -> (tempfile::TempDir, MockPlatform) {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        (dir, platform)
    }

    #[test]
    fn validate_is_skipped_when_ghostty_is_absent() {
        let (_dir, platform) = platform();
        assert_eq!(
            validate_config(&platform, Path::new("/tmp/config")).unwrap(),
            None
        );
        assert!(platform.commands().is_empty());
    }

    #[test]
    fn validate_invokes_the_documented_command() {
        let (_dir, platform) = platform();
        platform.set_env("KISEKI_WHICH_ghostty", "/opt/homebrew/bin/ghostty");
        platform.set_command_output("ghostty", CommandOutput::new(0, "", ""));
        let outcome = validate_config(&platform, Path::new("/tmp/ghostty.conf"))
            .unwrap()
            .unwrap();
        assert!(outcome.ok);
        let commands = platform.commands();
        assert_eq!(commands.len(), 1);
        // Ghostty 1.4.0 rejects `--config-file <path>`; it must be one token.
        assert!(commands[0].matches(
            "/opt/homebrew/bin/ghostty",
            &["+validate-config", "--config-file=/tmp/ghostty.conf"]
        ));
    }

    #[test]
    fn reload_sends_sigusr2_and_maps_exit_codes() {
        let (_dir, platform) = platform();
        platform.set_os(OsKind::Linux);
        platform.set_env("KISEKI_WHICH_pkill", "/usr/bin/pkill");
        platform.set_command_output("pkill", CommandOutput::new(0, "", ""));
        assert!(matches!(reload(&platform), ReloadOutcome::Signalled { .. }));
        assert!(platform.commands()[0].matches("pkill", &["-USR2", "-x", "ghostty"]));

        platform.clear_commands();
        platform.set_command_output("pkill", CommandOutput::new(1, "", ""));
        assert!(matches!(reload(&platform), ReloadOutcome::NotRunning { .. }));

        platform.set_command_output("pkill", CommandOutput::new(2, "", "boom"));
        assert!(matches!(reload(&platform), ReloadOutcome::Failed { .. }));
    }

    #[test]
    fn macos_reload_requires_ghostty_1_3_or_newer() {
        let (_dir, platform) = platform();
        platform.set_os(OsKind::MacOs);
        platform.set_env("KISEKI_WHICH_ghostty", GHOSTTY_BIN);
        platform.set_env("KISEKI_WHICH_pkill", "/usr/bin/pkill");
        platform.set_command_output("pkill", CommandOutput::new(0, "", ""));

        for version in ["Ghostty 1.3.0", "ghostty 1.4.0-dev+abc", "Ghostty 2.0.1"] {
            platform.clear_commands();
            platform.set_command_output(GHOSTTY_BIN, CommandOutput::new(0, version, ""));
            assert!(
                matches!(reload(&platform), ReloadOutcome::Signalled { .. }),
                "{version} must signal"
            );
            assert!(platform
                .commands()
                .iter()
                .any(|command| command.matches("pkill", &["-USR2", "-a", "-x", "ghostty"])));
        }

        for version in ["Ghostty 1.2.3", "Ghostty 1.0.0", "ghostty 0.9.9"] {
            platform.clear_commands();
            platform.set_command_output(GHOSTTY_BIN, CommandOutput::new(0, version, ""));
            let outcome = reload(&platform);
            assert!(
                matches!(outcome, ReloadOutcome::Skipped { .. }),
                "{version} -> {outcome:?}"
            );
            assert!(outcome.is_ok());
            assert!(!platform.commands().iter().any(|command| command.program == "pkill"));
        }

        // Unknown version: refuse to signal.
        platform.clear_commands();
        platform.set_command_output(GHOSTTY_BIN, CommandOutput::new(1, "", "boom"));
        assert!(matches!(reload(&platform), ReloadOutcome::Skipped { .. }));
        assert!(!platform.commands().iter().any(|command| command.program == "pkill"));
    }

    #[test]
    fn macos_config_probe_prefers_the_v1_order() {
        let (dir, platform) = platform();
        platform.set_os(OsKind::MacOs);
        let home = dir.path().join("home");
        platform.set_env("HOME", &home.to_string_lossy());
        platform.set_env(
            "XDG_CONFIG_HOME",
            &home.join(".config").to_string_lossy(),
        );
        let app_support = home
            .join("Library")
            .join("Application Support")
            .join("com.mitchellh.ghostty");
        let xdg = home.join(".config").join("ghostty");

        // Nothing exists: default to the app-support `config.ghostty`.
        assert_eq!(
            default_config_path(&platform),
            Some(app_support.join("config.ghostty"))
        );

        let write = |path: &PathBuf, contents: &str| {
            platform.create_dir_all(path.parent().unwrap()).unwrap();
            platform
                .write_preserving_symlinks(path, contents.as_bytes())
                .unwrap();
        };
        // Empty files are ignored in favour of non-empty ones.
        write(&app_support.join("config.ghostty"), "\n");
        write(&app_support.join("config"), "font-size = 13\n");
        assert_eq!(default_config_path(&platform), Some(app_support.join("config")));

        // A non-empty XDG file outranks nothing else here, but app support is
        // earlier in the order, so it still wins.
        write(&xdg.join("config.ghostty"), "font-size = 14\n");
        assert_eq!(default_config_path(&platform), Some(app_support.join("config")));

        // Once app support is empty-but-existing only, the non-empty XDG file wins.
        write(&app_support.join("config"), "\n");
        write(&app_support.join("config.ghostty"), "\n");
        assert_eq!(default_config_path(&platform), Some(xdg.join("config.ghostty")));
    }

    #[test]
    fn linux_config_probe_checks_config_ghostty_first() {
        let (dir, platform) = platform();
        platform.set_os(OsKind::Linux);
        let home = dir.path().join("home");
        platform.set_env("HOME", &home.to_string_lossy());
        let xdg = home.join(".config").join("ghostty");
        assert_eq!(
            default_config_path(&platform),
            Some(xdg.join("config.ghostty"))
        );
        platform.create_dir_all(&xdg).unwrap();
        platform
            .write_preserving_symlinks(&xdg.join("config"), b"font-size = 12\n")
            .unwrap();
        assert_eq!(default_config_path(&platform), Some(xdg.join("config")));
        platform
            .write_preserving_symlinks(&xdg.join("config.ghostty"), b"font-size = 13\n")
            .unwrap();
        assert_eq!(
            default_config_path(&platform),
            Some(xdg.join("config.ghostty"))
        );
    }

    #[test]
    fn explicit_env_override_beats_every_probe() {
        let (_dir, platform) = platform();
        platform.set_os(OsKind::MacOs);
        platform.set_env("HOME", "/Users/zz");
        platform.set_env("KISEKI_GHOSTTY_CONFIG", "/tmp/explicit-config");
        assert_eq!(
            default_config_path(&platform),
            Some(PathBuf::from("/tmp/explicit-config"))
        );
    }

    #[test]
    fn version_banners_are_parsed() {
        assert_eq!(parse_version("Ghostty 1.3.1"), Some((1, 3)));
        assert_eq!(parse_version("ghostty 1.4.0-dev+20250101"), Some((1, 4)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2)));
        assert_eq!(parse_version("Ghostty tip"), None);
    }

    #[test]
    fn invalid_config_produces_a_blocking_diagnostic() {
        let diagnostic = validation_diagnostic(
            Path::new("/tmp/ghostty.conf"),
            &ValidationOutcome {
                ok: false,
                stdout: String::new(),
                stderr: "unknown key: foo".to_string(),
            },
        )
        .unwrap();
        assert!(diagnostic.is_fatal());
        assert!(diagnostic.message.contains("unknown key: foo"));
        assert!(validation_diagnostic(
            Path::new("/tmp/x"),
            &ValidationOutcome {
                ok: true,
                stdout: String::new(),
                stderr: String::new(),
            }
        )
        .is_none());
    }

    #[test]
    fn include_line_is_optional_and_safe() {
        assert_eq!(
            include_line(Path::new("/home/zz/.config/kiseki/generated/ghostty.conf")).unwrap(),
            "config-file = ?/home/zz/.config/kiseki/generated/ghostty.conf"
        );
        assert!(include_line(Path::new("/tmp/a\nb")).is_err());
    }
}
