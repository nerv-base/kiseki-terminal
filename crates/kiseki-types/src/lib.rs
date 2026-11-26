//! Core domain types shared by every Kiseki crate.
//!
//! This crate is intentionally dependency-light and side-effect free: it holds
//! the vocabulary (colors, appearance, rendering profiles, capabilities,
//! environment context, diagnostics) that the rest of the workspace speaks.

pub mod capability;
pub mod color;
pub mod diagnostic;
pub mod env;
pub mod error;
pub mod policy;
pub mod rendering;

pub use capability::CapabilityFlags;
pub use color::{Color, Oklch};
pub use diagnostic::{Diagnostic, DiagnosticReport, DiagnosticSeverity};
pub use env::{Appearance, EnvContext, Multiplexer, OsKind, PowerSource};
pub use error::{KisekiError, Result};
pub use policy::PromptPolicy;
pub use rendering::RenderingProfile;

/// A validated lowercase kebab-case theme identifier (`^[a-z0-9][a-z0-9-]{1,47}$`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThemeId(String);

impl ThemeId {
    /// Maximum accepted length of a theme identifier.
    pub const MAX_LEN: usize = 48;

    /// Validates and wraps a theme identifier.
    pub fn new(raw: impl Into<String>) -> Result<Self> {
        let raw = raw.into();
        let bytes = raw.as_bytes();
        let valid_len = (2..=Self::MAX_LEN).contains(&bytes.len());
        let valid_chars = bytes.iter().enumerate().all(|(index, byte)| {
            let ok = byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-';
            if index == 0 {
                byte.is_ascii_lowercase() || byte.is_ascii_digit()
            } else {
                ok
            }
        });
        if valid_len && valid_chars && !raw.starts_with('-') && !raw.ends_with('-') {
            Ok(Self(raw))
        } else {
            Err(KisekiError::invalid_theme_id(raw))
        }
    }

    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the identifier, returning the inner `String`.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for ThemeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ThemeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for ThemeId {
    type Err = KisekiError;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl serde::Serialize for ThemeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ThemeId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_theme_ids() {
        for id in ["nerv-eva", "a1", "starlight-8bit", "kyoto-sakura"] {
            assert!(ThemeId::new(id).is_ok(), "{id} should be valid");
        }
    }

    #[test]
    fn rejects_invalid_theme_ids() {
        for id in ["", "a", "Nerv-Eva", "-nerv", "nerv-", "nerv eva", "nerv_eva", "nerv/eva"] {
            assert!(ThemeId::new(id).is_err(), "{id:?} should be rejected");
        }
        let long = "a".repeat(ThemeId::MAX_LEN + 1);
        assert!(ThemeId::new(long).is_err());
    }
}
