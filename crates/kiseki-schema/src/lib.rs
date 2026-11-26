//! Theme schema parsing, security validation, L1→L2 derivation and resolution.
//!
//! This crate is a pure function library: it never reads the filesystem or
//! spawns processes. Callers (`kiseki-pkg`, `kiseki-compiler`) feed it bytes.

pub mod derive;
pub mod lint;
pub mod resolve;
pub mod security;
pub mod tokens;
pub mod validate;

pub use derive::{derive_diff, derive_syntax, derive_ui};
pub use lint::{lint_contrast, ContrastCheck, ContrastReport};
pub use resolve::{Composition, ResolvedTheme, UserOverrides};
pub use tokens::{
    AccentTokens, AnsiTokens, AtmosphereTokens, Author, CodeSafeZone, CursorShape, DiffTokens, FastfetchOverrides,
    FastfetchFieldLabels, FontWeight, InteractionTokens, LayerKind, OverrideTokens, Padding, PaletteTokens, PartialDiffTokens,
    PartialSyntaxTokens, PartialUiTokens, PromptOverrides, Provenance, SemanticTokens, ShaderAtmosphere,
    ShaderParamValue, SurfaceTokens, SyntaxTokens, TextTokens, ThemeManifest, ThemePack, ThemeTokens, TypographyTokens,
    UiTokens, WallpaperAlignment, WallpaperAtmosphere, WallpaperFit, WindowAtmosphere, SCHEMA_VERSION,
};
pub use validate::{validate_manifest, validate_pack, validate_tokens, PackAssets, ValidationOutcome};

/// Fixtures shared by unit tests and the integration test suite.
///
/// The `nerv-eva` pack under `themes/` is the canonical example from RFC-0002,
/// so tests that assert on derivation and contrast use the same data a real
/// user would.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    const MANIFEST: &str = include_str!("../../../themes/nerv-eva/manifest.json");
    const TOKENS: &str = include_str!("../../../themes/nerv-eva/tokens.json");

    /// Parsed `nerv-eva` tokens.
    pub fn nerv_eva_tokens() -> ThemeTokens {
        serde_json::from_str(TOKENS).expect("nerv-eva tokens.json must parse")
    }

    /// Parsed `nerv-eva` manifest.
    pub fn nerv_eva_manifest() -> ThemeManifest {
        serde_json::from_str(MANIFEST).expect("nerv-eva manifest.json must parse")
    }

    /// The full `nerv-eva` pack.
    pub fn nerv_eva_pack() -> ThemePack {
        ThemePack {
            manifest: nerv_eva_manifest(),
            tokens: nerv_eva_tokens(),
        }
    }
}
