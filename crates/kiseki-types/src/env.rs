//! Environment context shared with every adapter (RFC-0003 §2).

use serde::{Deserialize, Serialize};

/// Host operating system family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OsKind {
    MacOs,
    Windows,
    Linux,
}

impl OsKind {
    /// Detects the host OS at compile time.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacOs => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }

    /// True on Unix hosts, where `SIGUSR2` reloads are possible.
    pub const fn is_unix(self) -> bool {
        matches!(self, Self::MacOs | Self::Linux)
    }
}

impl std::fmt::Display for OsKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Terminal multiplexer the current session runs inside, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Multiplexer {
    Tmux,
    Zellij,
    Screen,
}

impl Multiplexer {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tmux => "tmux",
            Self::Zellij => "zellij",
            Self::Screen => "screen",
        }
    }

    /// True when the multiplexer needs DCS passthrough for OSC sequences.
    pub const fn needs_passthrough(self) -> bool {
        matches!(self, Self::Tmux | Self::Screen)
    }
}

impl std::fmt::Display for Multiplexer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Power source, used to recommend lower rendering profiles on battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PowerSource {
    Ac,
    Battery,
    #[default]
    Unknown,
}

impl PowerSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ac => "ac",
            Self::Battery => "battery",
            Self::Unknown => "unknown",
        }
    }

    /// True when the machine is known to be on battery power.
    pub const fn is_battery(self) -> bool {
        matches!(self, Self::Battery)
    }
}

/// Dark/light appearance declared by a theme or reported by the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

impl Appearance {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// The opposite appearance, for light/dark theme pairing.
    pub const fn opposite(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }
}

impl std::fmt::Display for Appearance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Appearance {
    type Err = crate::error::KisekiError;

    fn from_str(s: &str) -> crate::error::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "dark" => Ok(Self::Dark),
            "light" => Ok(Self::Light),
            other => Err(crate::error::KisekiError::invalid_appearance(other.to_string())),
        }
    }
}

/// Everything adapters need to know about the host session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvContext {
    pub os: OsKind,
    /// Detected terminal emulator identifier (`ghostty`, `windows-terminal`, ...).
    pub current_terminal: Option<String>,
    pub is_ssh: bool,
    pub is_wsl: bool,
    pub multiplexer: Option<Multiplexer>,
    pub power_source: PowerSource,
    pub system_appearance: Appearance,
    /// True when the user (or a flag) requested artifacts-only rendering.
    pub render_only: bool,
}

impl Default for EnvContext {
    fn default() -> Self {
        Self {
            os: OsKind::current(),
            current_terminal: None,
            is_ssh: false,
            is_wsl: false,
            multiplexer: None,
            power_source: PowerSource::Unknown,
            system_appearance: Appearance::Dark,
            render_only: false,
        }
    }
}

impl EnvContext {
    /// A minimal context for the current OS with no detected session details.
    pub fn local() -> Self {
        Self::default()
    }

    /// True when host config writes must be suppressed.
    ///
    /// SSH sessions never rewrite the local desktop terminal, and an explicit
    /// `--render-only` always wins.
    pub fn should_suppress_host_writes(&self) -> bool {
        self.render_only || self.is_ssh
    }

    /// True when OSC sequences must be wrapped in DCS passthrough.
    pub fn needs_osc_passthrough(&self) -> bool {
        self.multiplexer.is_some_and(Multiplexer::needs_passthrough)
    }

    /// Returns the current terminal identifier when it matches `id`.
    pub fn terminal_is(&self, id: &str) -> bool {
        self.current_terminal
            .as_deref()
            .is_some_and(|current| current.eq_ignore_ascii_case(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_opposite_and_parsing() {
        assert_eq!(Appearance::Dark.opposite(), Appearance::Light);
        assert_eq!(Appearance::Light.opposite(), Appearance::Dark);
        assert_eq!("LIGHT".parse::<Appearance>().unwrap(), Appearance::Light);
        assert!("dim".parse::<Appearance>().is_err());
    }

    #[test]
    fn ssh_and_render_only_suppress_writes() {
        let mut env = EnvContext::local();
        assert!(!env.should_suppress_host_writes());
        env.is_ssh = true;
        assert!(env.should_suppress_host_writes());

        let render_only = EnvContext {
            render_only: true,
            ..EnvContext::local()
        };
        assert!(render_only.should_suppress_host_writes());
    }

    #[test]
    fn multiplexer_passthrough_rules() {
        let mut env = EnvContext::local();
        assert!(!env.needs_osc_passthrough());
        env.multiplexer = Some(Multiplexer::Zellij);
        assert!(!env.needs_osc_passthrough());
        env.multiplexer = Some(Multiplexer::Tmux);
        assert!(env.needs_osc_passthrough());
    }

    #[test]
    fn terminal_matching_is_case_insensitive() {
        let env = EnvContext {
            current_terminal: Some("Ghostty".to_string()),
            ..EnvContext::local()
        };
        assert!(env.terminal_is("ghostty"));
        assert!(!env.terminal_is("kitty"));
    }

    #[test]
    fn serde_roundtrip() {
        let env = EnvContext {
            os: OsKind::Linux,
            current_terminal: Some("ghostty".into()),
            is_ssh: false,
            is_wsl: true,
            multiplexer: Some(Multiplexer::Tmux),
            power_source: PowerSource::Battery,
            system_appearance: Appearance::Light,
            render_only: false,
        };
        let json = serde_json::to_string(&env).unwrap();
        let back: EnvContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back, env);
    }
}
