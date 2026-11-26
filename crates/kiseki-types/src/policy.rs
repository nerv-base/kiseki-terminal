//! Prompt integration policy (RFC-0003 §4, ADR-006).
//!
//! Detecting a user-authored Oh My Posh or Starship config must never mean
//! silently replacing it. The user explicitly chooses one of these three
//! behaviours during `kiseki init`, and every later command honours it.

use serde::{Deserialize, Serialize};

use crate::error::{KisekiError, Result};

/// How the engine integrates with an existing prompt engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PromptPolicy {
    /// Emit and reference a complete, safe Kiseki prompt template.
    #[default]
    ManagedTemplate,
    /// Never touch the user's prompt layout; only expose the palette so the
    /// user can reference it from their own config.
    PaletteOnly,
    /// Do not emit prompt artefacts at all.
    Disabled,
}

impl PromptPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ManagedTemplate => "managed-template",
            Self::PaletteOnly => "palette-only",
            Self::Disabled => "disabled",
        }
    }

    /// True when `generated/prompt.omp.json` should be produced.
    pub const fn emits_managed_template(self) -> bool {
        matches!(self, Self::ManagedTemplate)
    }

    /// True when any prompt artefact should be produced.
    pub const fn emits_any(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

impl std::fmt::Display for PromptPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for PromptPolicy {
    type Err = KisekiError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "managed-template" | "managed" | "template" => Ok(Self::ManagedTemplate),
            "palette-only" | "palette" => Ok(Self::PaletteOnly),
            "disabled" | "none" | "off" => Ok(Self::Disabled),
            other => Err(KisekiError::other(format!(
                "invalid prompt policy {other:?}: expected managed-template|palette-only|disabled"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_parsing_and_rendering() {
        assert_eq!(
            "managed-template".parse::<PromptPolicy>().unwrap(),
            PromptPolicy::ManagedTemplate
        );
        assert_eq!("Palette".parse::<PromptPolicy>().unwrap(), PromptPolicy::PaletteOnly);
        assert_eq!("off".parse::<PromptPolicy>().unwrap(), PromptPolicy::Disabled);
        assert!("maybe".parse::<PromptPolicy>().is_err());
        assert_eq!(PromptPolicy::default(), PromptPolicy::ManagedTemplate);
    }

    #[test]
    fn emission_predicates() {
        assert!(PromptPolicy::ManagedTemplate.emits_managed_template());
        assert!(PromptPolicy::ManagedTemplate.emits_any());
        assert!(!PromptPolicy::PaletteOnly.emits_managed_template());
        assert!(PromptPolicy::PaletteOnly.emits_any());
        assert!(!PromptPolicy::Disabled.emits_any());
    }

    #[test]
    fn serde_is_kebab_case() {
        assert_eq!(
            serde_json::to_string(&PromptPolicy::PaletteOnly).unwrap(),
            "\"palette-only\""
        );
        let parsed: PromptPolicy = serde_json::from_str("\"disabled\"").unwrap();
        assert_eq!(parsed, PromptPolicy::Disabled);
    }
}
