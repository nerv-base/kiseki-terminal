//! Adapter registry (RFC-0003 §3).
//!
//! Adapters are statically registered at compile time. There is no dynamic
//! plugin loading, which is what makes the "no third-party code execution"
//! guarantee enforceable.

use kiseki_platform::Platform;
use kiseki_types::{EnvContext, Result};

use crate::{Adapter, DetectionReport, PlanOptions};

/// The set of adapters this build knows about.
pub struct AdapterRegistry {
    adapters: Vec<Box<dyn Adapter>>,
}

impl AdapterRegistry {
    /// Registers every built-in Tier 1 and Tier 2 adapter (RFC-0003 §4).
    pub fn standard() -> Self {
        Self {
            adapters: vec![
                // Tier 1 Terminals
                Box::new(crate::GhosttyAdapter),
                Box::new(crate::WindowsTerminalAdapter),
                Box::new(crate::KittyAdapter),
                Box::new(crate::WezTermAdapter),
                Box::new(crate::ITerm2Adapter),
                Box::new(crate::AlacrittyAdapter),
                Box::new(crate::AppleTerminalAdapter),
                // Tier 1 Shells
                Box::new(crate::ZshAdapter),
                Box::new(crate::BashAdapter),
                Box::new(crate::FishAdapter),
                Box::new(crate::PowerShellAdapter),
                // Tier 1 Prompt & System Card
                Box::new(crate::PromptAdapter),
                Box::new(crate::FastfetchAdapter),
                // Tier 2 Opt-In Ecosystem Tools
                Box::new(crate::NeovimAdapter),
                Box::new(crate::VimAdapter),
                Box::new(crate::HelixAdapter),
                Box::new(crate::TmuxAdapter),
                Box::new(crate::ZellijAdapter),
                Box::new(crate::OpenCodeAdapter),
                Box::new(crate::BatAdapter),
                Box::new(crate::DeltaAdapter),
                Box::new(crate::LazygitAdapter),
                Box::new(crate::BtopAdapter),
            ],
        }
    }

    /// Registers the Milestone 1 vertical slice:
    /// Ghostty + Windows Terminal + Zsh + PowerShell + Prompt + Fastfetch.
    pub fn milestone1() -> Self {
        Self {
            adapters: vec![
                Box::new(crate::GhosttyAdapter),
                Box::new(crate::WindowsTerminalAdapter),
                Box::new(crate::ZshAdapter),
                Box::new(crate::PowerShellAdapter),
                Box::new(crate::PromptAdapter),
                Box::new(crate::FastfetchAdapter),
            ],
        }
    }

    /// Builds a registry from an explicit adapter list (tests).
    pub fn from_adapters(adapters: Vec<Box<dyn Adapter>>) -> Self {
        Self { adapters }
    }

    pub fn all(&self) -> &[Box<dyn Adapter>] {
        &self.adapters
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Adapter> {
        self.adapters.iter().map(|adapter| adapter.as_ref())
    }

    pub fn by_id(&self, id: &str) -> Option<&dyn Adapter> {
        self.iter().find(|adapter| adapter.id() == id)
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.iter().map(Adapter::id).collect()
    }

    /// Detects every adapter allowed by `options.only`.
    pub fn detect_all(
        &self,
        env: &EnvContext,
        platform: &dyn Platform,
        options: &PlanOptions,
    ) -> Vec<(String, Result<DetectionReport>)> {
        self.iter()
            .filter(|adapter| options.allows(adapter.id()))
            .map(|adapter| (adapter.id().to_string(), adapter.detect(env, platform)))
            .collect()
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestone_one_registers_the_vertical_slice() {
        let registry = AdapterRegistry::milestone1();
        assert_eq!(
            registry.ids(),
            vec![
                "ghostty",
                "windows-terminal",
                "zsh",
                "pwsh",
                "prompt",
                "fastfetch"
            ]
        );
        assert!(registry.by_id("ghostty").is_some());
        assert!(registry.by_id("nope").is_none());
    }

    #[test]
    fn only_filter_selects_adapters() {
        let registry = AdapterRegistry::milestone1();
        let options = PlanOptions {
            only: vec!["zsh".to_string(), "pwsh".to_string()],
            ..PlanOptions::default()
        };
        let selected: Vec<&str> = registry
            .iter()
            .filter(|adapter| options.allows(adapter.id()))
            .map(|adapter| adapter.id())
            .collect();
        assert_eq!(selected, vec!["zsh", "pwsh"]);
    }
}
