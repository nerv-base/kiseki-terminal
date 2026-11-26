//! Capability model (RFC-0003 §2).
//!
//! Adapters report what the host can actually do instead of relying on
//! hard-coded `if terminal == ...` branches scattered through the engine.

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    /// Set of host capabilities negotiated per adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct CapabilityFlags: u32 {
        // Color and visuals
        const PALETTE_16           = 1 << 0;
        const TRUECOLOR            = 1 << 1;
        const WINDOW_OPACITY       = 1 << 2;
        const WINDOW_BLUR          = 1 << 3;
        const WALLPAPER_STATIC     = 1 << 4;
        const WALLPAPER_ALIGNMENT  = 1 << 5;
        const FONT_STACK_FALLBACK  = 1 << 6;
        const FONT_SINGLE_ONLY     = 1 << 7;
        const UI_CHROME            = 1 << 8;

        // Shader rendering models (must stay distinct: they are not equivalent)
        const SHADER_LAYERED_BG    = 1 << 9;
        const SHADER_POSTPROCESS   = 1 << 10;

        // Hot reload and scope
        const HOT_RELOAD_SIGNAL    = 1 << 11;
        const HOT_RELOAD_WATCH     = 1 << 12;
        const SESSION_LIVE_OSC     = 1 << 13;

        // Configuration integration
        const SUPPORTS_INCLUDE     = 1 << 14;
        const REQUIRES_JSONC_PATCH = 1 << 15;
        const PARENT_INHERITANCE   = 1 << 16;
    }
}

impl CapabilityFlags {
    /// True when the host can run any dynamic shader at all.
    pub fn supports_dynamic_shader(self) -> bool {
        self.intersects(Self::SHADER_LAYERED_BG | Self::SHADER_POSTPROCESS)
    }

    /// True when the host reloads configuration without a restart.
    pub fn supports_hot_reload(self) -> bool {
        self.intersects(Self::HOT_RELOAD_SIGNAL | Self::HOT_RELOAD_WATCH)
    }

    /// True when the host can render a static wallpaper image.
    pub fn supports_wallpaper(self) -> bool {
        self.contains(Self::WALLPAPER_STATIC)
    }

    /// True when the host honours window transparency.
    pub fn supports_transparency(self) -> bool {
        self.contains(Self::WINDOW_OPACITY)
    }

    /// Renders the set as a stable, sorted `snake_case` list for `--json` output.
    pub fn to_names(self) -> Vec<&'static str> {
        const NAMES: &[(CapabilityFlags, &str)] = &[
            (CapabilityFlags::PALETTE_16, "palette_16"),
            (CapabilityFlags::TRUECOLOR, "truecolor"),
            (CapabilityFlags::WINDOW_OPACITY, "window_opacity"),
            (CapabilityFlags::WINDOW_BLUR, "window_blur"),
            (CapabilityFlags::WALLPAPER_STATIC, "wallpaper_static"),
            (CapabilityFlags::WALLPAPER_ALIGNMENT, "wallpaper_alignment"),
            (CapabilityFlags::FONT_STACK_FALLBACK, "font_stack_fallback"),
            (CapabilityFlags::FONT_SINGLE_ONLY, "font_single_only"),
            (CapabilityFlags::UI_CHROME, "ui_chrome"),
            (CapabilityFlags::SHADER_LAYERED_BG, "shader_layered_bg"),
            (CapabilityFlags::SHADER_POSTPROCESS, "shader_postprocess"),
            (CapabilityFlags::HOT_RELOAD_SIGNAL, "hot_reload_signal"),
            (CapabilityFlags::HOT_RELOAD_WATCH, "hot_reload_watch"),
            (CapabilityFlags::SESSION_LIVE_OSC, "session_live_osc"),
            (CapabilityFlags::SUPPORTS_INCLUDE, "supports_include"),
            (CapabilityFlags::REQUIRES_JSONC_PATCH, "requires_jsonc_patch"),
            (CapabilityFlags::PARENT_INHERITANCE, "parent_inheritance"),
        ];
        NAMES
            .iter()
            .filter(|(flag, _)| self.contains(*flag))
            .map(|(_, name)| *name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_predicates_track_flags() {
        let ghostty = CapabilityFlags::SHADER_POSTPROCESS
            | CapabilityFlags::HOT_RELOAD_SIGNAL
            | CapabilityFlags::WALLPAPER_STATIC
            | CapabilityFlags::WINDOW_OPACITY;
        assert!(ghostty.supports_dynamic_shader());
        assert!(ghostty.supports_hot_reload());
        assert!(ghostty.supports_wallpaper());
        assert!(ghostty.supports_transparency());

        let alacritty = CapabilityFlags::TRUECOLOR | CapabilityFlags::WINDOW_OPACITY;
        assert!(!alacritty.supports_dynamic_shader());
        assert!(!alacritty.supports_wallpaper());
        assert!(alacritty.supports_transparency());
    }

    #[test]
    fn names_are_stable_and_ordered() {
        let flags = CapabilityFlags::TRUECOLOR | CapabilityFlags::SHADER_LAYERED_BG;
        assert_eq!(flags.to_names(), vec!["truecolor", "shader_layered_bg"]);
        assert!(CapabilityFlags::empty().to_names().is_empty());
    }

    #[test]
    fn serde_roundtrip_preserves_bits() {
        let flags = CapabilityFlags::SHADER_POSTPROCESS | CapabilityFlags::HOT_RELOAD_SIGNAL;
        let json = serde_json::to_string(&flags).unwrap();
        let back: CapabilityFlags = serde_json::from_str(&json).unwrap();
        assert_eq!(flags, back);
    }
}
