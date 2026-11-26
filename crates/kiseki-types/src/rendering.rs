//! Rendering profiles (RFC-0004 §2).

use serde::{Deserialize, Serialize};

/// The dynamic-effect quality tier a theme is rendered at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RenderingProfile {
    /// Static wallpaper and palette only, no shader pass at all.
    Static,
    /// Single texture sample, no UV distortion, minimal particles.
    Eco,
    /// Full core animation with text-protection masking.
    #[default]
    Balanced,
    /// Multi-layer parallax particles and full bloom sampling.
    Ultra,
    /// Wallpaper, blur and animation disabled; contrast forced to WCAG AAA.
    Accessibility,
}

impl RenderingProfile {
    /// The `KISEKI_QUALITY_LEVEL` macro value injected into shader headers.
    pub const fn quality_level(self) -> u8 {
        match self {
            Self::Static | Self::Accessibility => 0,
            Self::Eco => 1,
            Self::Balanced => 2,
            Self::Ultra => 3,
        }
    }

    /// Stable lowercase name used in CLI output and `state.json`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Eco => "eco",
            Self::Balanced => "balanced",
            Self::Ultra => "ultra",
            Self::Accessibility => "accessibility",
        }
    }

    /// True when the profile renders any dynamic shader.
    pub const fn is_dynamic(self) -> bool {
        matches!(self, Self::Eco | Self::Balanced | Self::Ultra)
    }

    /// True when the profile forces the high-contrast accessibility transform.
    pub const fn is_accessibility(self) -> bool {
        matches!(self, Self::Accessibility)
    }

    /// All profiles, in increasing order of visual fidelity.
    pub const ALL: [Self; 5] = [
        Self::Static,
        Self::Eco,
        Self::Balanced,
        Self::Ultra,
        Self::Accessibility,
    ];
}

impl std::fmt::Display for RenderingProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for RenderingProfile {
    type Err = crate::error::KisekiError;

    fn from_str(s: &str) -> crate::error::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "static" => Ok(Self::Static),
            "eco" => Ok(Self::Eco),
            "balanced" => Ok(Self::Balanced),
            "ultra" => Ok(Self::Ultra),
            "accessibility" => Ok(Self::Accessibility),
            other => Err(crate::error::KisekiError::invalid_profile(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_levels_match_rfc_table() {
        assert_eq!(RenderingProfile::Static.quality_level(), 0);
        assert_eq!(RenderingProfile::Eco.quality_level(), 1);
        assert_eq!(RenderingProfile::Balanced.quality_level(), 2);
        assert_eq!(RenderingProfile::Ultra.quality_level(), 3);
        assert_eq!(RenderingProfile::Accessibility.quality_level(), 0);
    }

    #[test]
    fn dynamic_and_accessibility_predicates() {
        assert!(!RenderingProfile::Static.is_dynamic());
        assert!(RenderingProfile::Eco.is_dynamic());
        assert!(RenderingProfile::Balanced.is_dynamic());
        assert!(RenderingProfile::Ultra.is_dynamic());
        assert!(!RenderingProfile::Accessibility.is_dynamic());
        assert!(RenderingProfile::Accessibility.is_accessibility());
    }

    #[test]
    fn parses_from_str_case_insensitively() {
        assert_eq!("Balanced".parse::<RenderingProfile>().unwrap(), RenderingProfile::Balanced);
        assert_eq!(" eco ".parse::<RenderingProfile>().unwrap(), RenderingProfile::Eco);
        assert!("turbo".parse::<RenderingProfile>().is_err());
    }

    #[test]
    fn serde_is_lowercase() {
        assert_eq!(serde_json::to_string(&RenderingProfile::Ultra).unwrap(), "\"ultra\"");
        let parsed: RenderingProfile = serde_json::from_str("\"accessibility\"").unwrap();
        assert_eq!(parsed, RenderingProfile::Accessibility);
    }

    #[test]
    fn default_is_balanced() {
        assert_eq!(RenderingProfile::default(), RenderingProfile::Balanced);
        assert_eq!(RenderingProfile::ALL.len(), 5);
    }
}
