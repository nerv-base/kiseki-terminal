//! Complexity budgets that protect against GPU driver hangs (TDR) and
//! reset loops (RFC-0004 §3, gate 3).

use kiseki_types::RenderingProfile;

/// Static limits applied to a compiled shader program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Maximum source size in bytes.
    pub max_source_bytes: usize,
    /// Maximum static texture sample operations.
    pub max_texture_samples: usize,
    /// Maximum loop nesting depth.
    pub max_loop_depth: usize,
    /// Maximum iterations of any single constant-bounded loop.
    pub max_loop_iterations: u32,
}

impl Budget {
    /// Budget for a rendering profile.
    ///
    /// `static` and `accessibility` never mount a shader, so they get the
    /// strictest budget available; any shader reaching validation at those
    /// profiles is a bug worth rejecting early.
    pub const fn for_profile(profile: RenderingProfile) -> Self {
        match profile {
            RenderingProfile::Static | RenderingProfile::Accessibility => Self::ECO,
            RenderingProfile::Eco => Self::ECO,
            RenderingProfile::Balanced | RenderingProfile::Ultra => Self::HIGH,
        }
    }

    /// Eco-tier limits: 32 KiB source, 4 samples, 1 loop level, 8 iterations.
    pub const ECO: Self = Self {
        max_source_bytes: 32 * 1024,
        max_texture_samples: 4,
        max_loop_depth: 1,
        max_loop_iterations: 8,
    };

    /// Balanced/ultra limits: 64 KiB source, 16 samples, 2 levels, 32 iterations.
    pub const HIGH: Self = Self {
        max_source_bytes: 64 * 1024,
        max_texture_samples: 16,
        max_loop_depth: 2,
        max_loop_iterations: 32,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_budgets_match_rfc_table() {
        assert_eq!(Budget::for_profile(RenderingProfile::Eco), Budget::ECO);
        assert_eq!(Budget::for_profile(RenderingProfile::Balanced), Budget::HIGH);
        assert_eq!(Budget::for_profile(RenderingProfile::Ultra), Budget::HIGH);
        assert_eq!(Budget::for_profile(RenderingProfile::Static), Budget::ECO);
        assert_eq!(Budget::for_profile(RenderingProfile::Accessibility), Budget::ECO);
    }

    #[test]
    fn eco_is_stricter_than_high_on_every_axis() {
        const { assert!(Budget::ECO.max_source_bytes < Budget::HIGH.max_source_bytes) };
        const { assert!(Budget::ECO.max_texture_samples < Budget::HIGH.max_texture_samples) };
        const { assert!(Budget::ECO.max_loop_depth < Budget::HIGH.max_loop_depth) };
        const { assert!(Budget::ECO.max_loop_iterations < Budget::HIGH.max_loop_iterations) };
    }
}
