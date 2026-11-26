//! Host environment detection (RFC-0003 §2).
//!
//! Detection is a pure function over an environment snapshot plus a couple of
//! well-known files, which keeps it unit-testable and, more importantly, keeps
//! `kiseki status` fast: no subprocesses, no AppleScript, no shelling out.

use std::collections::BTreeMap;
use std::path::Path;

use kiseki_types::{Appearance, EnvContext, Multiplexer, OsKind, PowerSource};

use crate::platform::Platform;

/// A snapshot of the environment variables detection cares about.
pub type EnvSnapshot = BTreeMap<String, String>;

/// Captures the real process environment.
pub fn capture_env() -> EnvSnapshot {
    std::env::vars().collect()
}

/// Reads the environment through a [`Platform`].
pub fn snapshot(platform: &dyn Platform, keys: &[&str]) -> EnvSnapshot {
    keys.iter()
        .filter_map(|key| platform.env_var(key).map(|value| ((*key).to_string(), value)))
        .collect()
}

/// Detects the terminal emulator from the environment.
///
/// Order matters: a nested session (tmux inside Ghostty) still reports
/// `ghostty` because the terminal is what owns the config files.
pub fn detect_terminal(env: &EnvSnapshot) -> Option<String> {
    if let Some(explicit) = env.get("KISEKI_TERMINAL").filter(|value| !value.is_empty()) {
        return Some(explicit.to_ascii_lowercase());
    }
    if env.contains_key("WT_SESSION") {
        return Some("windows-terminal".to_string());
    }
    if env.get("TERM_PROGRAM").is_some_and(|value| value.eq_ignore_ascii_case("ghostty"))
        || env.get("GHOSTTY_RESOURCES_DIR").is_some()
        || env.get("GHOSTTY_BIN_DIR").is_some()
    {
        return Some("ghostty".to_string());
    }
    if env.get("KITTY_WINDOW_ID").is_some() || env.get("TERM").is_some_and(|term| term.contains("kitty")) {
        return Some("kitty".to_string());
    }
    if env.get("WEZTERM_PANE").is_some() || env.get("TERM_PROGRAM").is_some_and(|v| v.eq_ignore_ascii_case("wezterm")) {
        return Some("wezterm".to_string());
    }
    if env.get("ALACRITTY_WINDOW_ID").is_some() {
        return Some("alacritty".to_string());
    }
    if env.get("ITERM_SESSION_ID").is_some() || env.get("TERM_PROGRAM").is_some_and(|v| v.eq_ignore_ascii_case("iterm.app")) {
        return Some("iterm2".to_string());
    }
    if env.get("TERM_PROGRAM").is_some_and(|v| v.eq_ignore_ascii_case("apple_terminal")) {
        return Some("apple-terminal".to_string());
    }
    if env.get("TERM_PROGRAM").is_some_and(|v| v.eq_ignore_ascii_case("vscode")) {
        return Some("vscode".to_string());
    }
    if env.get("WARP_SESSION_ID").is_some() {
        return Some("warp".to_string());
    }
    None
}

/// Detects the multiplexer wrapping the session.
pub fn detect_multiplexer(env: &EnvSnapshot) -> Option<Multiplexer> {
    if env.get("ZELLIJ").is_some() || env.get("ZELLIJ_SESSION_NAME").is_some() {
        Some(Multiplexer::Zellij)
    } else if env.get("TMUX").is_some() {
        Some(Multiplexer::Tmux)
    } else if env.get("STY").is_some() {
        Some(Multiplexer::Screen)
    } else {
        None
    }
}

/// Detects SSH by the three variables OpenSSH exports.
pub fn detect_ssh(env: &EnvSnapshot) -> bool {
    env.contains_key("SSH_CONNECTION") || env.contains_key("SSH_TTY") || env.contains_key("SSH_CLIENT")
}

/// Detects WSL from environment variables.
pub fn detect_wsl(env: &EnvSnapshot) -> bool {
    env.contains_key("WSL_DISTRO_NAME") || env.contains_key("WSL_INTEROP")
}

/// Reads the system appearance from an explicit override or the OS.
///
/// macOS `AppleInterfaceStyle` and Windows `AppsUseLightTheme` are normally
/// read via platform APIs. Milestone 1 honours an explicit override and falls
/// back to dark, avoiding a fork on the hot path.
pub fn detect_appearance(platform: &dyn Platform, env: &EnvSnapshot) -> Appearance {
    if let Some(explicit) = env.get("KISEKI_APPEARANCE") {
        if let Ok(appearance) = explicit.parse() {
            return appearance;
        }
    }
    // macOS: the global preferences plist is a binary blob, so use the
    // user-facing override file Kiseki writes during `init`.
    let override_file = platform.paths().config_dir().join("appearance");
    if let Ok(Some(value)) = platform.read_to_string(&override_file) {
        if let Ok(appearance) = value.trim().parse() {
            return appearance;
        }
    }
    Appearance::Dark
}

/// Detects whether the machine is on battery.
///
/// Linux exposes this as a plain file, so no subprocess is needed. macOS and
/// Windows use an explicit environment hint until the native probe lands.
pub fn detect_power_source(platform: &dyn Platform, env: &EnvSnapshot) -> PowerSource {
    if let Some(explicit) = env.get("KISEKI_POWER_SOURCE") {
        return match explicit.to_ascii_lowercase().as_str() {
            "ac" => PowerSource::Ac,
            "battery" => PowerSource::Battery,
            _ => PowerSource::Unknown,
        };
    }
    if platform.os() == OsKind::Linux {
        let base = Path::new("/sys/class/power_supply");
        if let Ok(entries) = platform.list_dir(base) {
            for entry in &entries {
                let name = entry.file_name().and_then(|name| name.to_str()).unwrap_or("");
                if name.starts_with("AC") || name.starts_with("ADP") {
                    if let Ok(Some(value)) = platform.read_to_string(&entry.join("online")) {
                        if value.trim() == "1" {
                            return PowerSource::Ac;
                        }
                    }
                }
            }
            if entries.iter().any(|entry| {
                entry
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("BAT"))
            }) {
                return PowerSource::Battery;
            }
        }
    }
    PowerSource::Unknown
}

/// Builds an [`EnvContext`] from an environment snapshot.
pub fn detect(platform: &dyn Platform, env: &EnvSnapshot) -> EnvContext {
    EnvContext {
        os: platform.os(),
        current_terminal: detect_terminal(env),
        is_ssh: detect_ssh(env),
        is_wsl: detect_wsl(env),
        multiplexer: detect_multiplexer(env),
        power_source: detect_power_source(platform, env),
        system_appearance: detect_appearance(platform, env),
        render_only: env
            .get("KISEKI_RENDER_ONLY")
            .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes")),
    }
}

/// Detects the environment for the current process.
pub fn detect_current() -> EnvContext {
    let platform = crate::platform::SystemPlatform::detect();
    let env = capture_env();
    detect(&platform, &env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::MockPlatform;

    fn snapshot_of(pairs: &[(&str, &str)]) -> EnvSnapshot {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn platform() -> (tempfile::TempDir, MockPlatform) {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        (dir, platform)
    }

    #[test]
    fn terminal_detection_prefers_explicit_override() {
        let env = snapshot_of(&[
            ("KISEKI_TERMINAL", "ghostty"),
            ("WT_SESSION", "abc"),
            ("TERM_PROGRAM", "iTerm.app"),
        ]);
        assert_eq!(detect_terminal(&env).as_deref(), Some("ghostty"));
    }

    #[test]
    fn terminal_detection_covers_the_milestone_hosts() {
        for (pairs, expected) in [
            (vec![("WT_SESSION", "x")], "windows-terminal"),
            (vec![("TERM_PROGRAM", "ghostty")], "ghostty"),
            (vec![("GHOSTTY_RESOURCES_DIR", "/x")], "ghostty"),
            (vec![("KITTY_WINDOW_ID", "1")], "kitty"),
            (vec![("WEZTERM_PANE", "1")], "wezterm"),
            (vec![("ALACRITTY_WINDOW_ID", "1")], "alacritty"),
            (vec![("ITERM_SESSION_ID", "x")], "iterm2"),
            (vec![("TERM_PROGRAM", "Apple_Terminal")], "apple-terminal"),
        ] {
            let env = snapshot_of(&pairs);
            assert_eq!(detect_terminal(&env).as_deref(), Some(expected), "{pairs:?}");
        }
        assert_eq!(detect_terminal(&snapshot_of(&[])), None);
    }

    #[test]
    fn ssh_wsl_and_multiplexer_detection() {
        assert!(detect_ssh(&snapshot_of(&[("SSH_CONNECTION", "x")])));
        assert!(!detect_ssh(&snapshot_of(&[])));
        assert!(detect_wsl(&snapshot_of(&[("WSL_DISTRO_NAME", "Ubuntu")])));
        assert!(!detect_wsl(&snapshot_of(&[])));
        assert_eq!(
            detect_multiplexer(&snapshot_of(&[("ZELLIJ", "0")])),
            Some(Multiplexer::Zellij)
        );
        assert_eq!(
            detect_multiplexer(&snapshot_of(&[("TMUX", "/tmp/tmux")])),
            Some(Multiplexer::Tmux)
        );
        assert_eq!(
            detect_multiplexer(&snapshot_of(&[("STY", "1.term")])),
            Some(Multiplexer::Screen)
        );
        assert_eq!(detect_multiplexer(&snapshot_of(&[])), None);
    }

    #[test]
    fn full_context_is_assembled() {
        let (_dir, platform) = platform();
        let env = snapshot_of(&[
            ("TERM_PROGRAM", "ghostty"),
            ("SSH_TTY", "/dev/ttys001"),
            ("TMUX", "/tmp/tmux"),
            ("KISEKI_POWER_SOURCE", "battery"),
            ("KISEKI_APPEARANCE", "light"),
            ("KISEKI_RENDER_ONLY", "1"),
        ]);
        let context = detect(&platform, &env);
        assert_eq!(context.current_terminal.as_deref(), Some("ghostty"));
        assert!(context.is_ssh);
        assert_eq!(context.multiplexer, Some(Multiplexer::Tmux));
        assert_eq!(context.power_source, PowerSource::Battery);
        assert_eq!(context.system_appearance, Appearance::Light);
        assert!(context.render_only);
        assert!(context.should_suppress_host_writes());
    }

    #[test]
    fn appearance_override_file_is_honoured() {
        let (_dir, platform) = platform();
        let path = platform.paths().config_dir().join("appearance");
        platform.write_preserving_symlinks(&path, b"light\n").unwrap();
        let context = detect(&platform, &snapshot_of(&[]));
        assert_eq!(context.system_appearance, Appearance::Light);
    }
}
