//! Deterministic L1 → L2 derivation (RFC-0002 §3.2).
//!
//! When a theme omits `syntax`, `diff` or `ui`, the compiler derives every slot
//! from L1 in OKLCH space. The same input always yields the same output, which
//! is what makes the compiler snapshot-testable.

use kiseki_types::Color;

use crate::tokens::{
    DiffTokens, PaletteTokens, PartialDiffTokens, PartialSyntaxTokens, PartialUiTokens, SyntaxTokens, UiTokens,
};

/// Minimum contrast a derived comment color must reach against the canvas.
const COMMENT_MIN_CONTRAST: f64 = 3.0;

/// Derives the complete syntax layer, preferring author-provided values.
pub fn derive_syntax(palette: &PaletteTokens, authored: Option<&PartialSyntaxTokens>) -> SyntaxTokens {
    let authored = authored.cloned().unwrap_or_default();
    let accent = palette.accents.primary;
    let accent2 = palette.accents.secondary;
    let tertiary = palette.accents.tertiary;
    let ansi = &palette.ansi;
    let text = &palette.text;
    let semantic = &palette.semantic;
    let base = palette.surfaces.base;

    let pick = |authored: Option<Color>, derived: Color| authored.unwrap_or(derived);

    SyntaxTokens {
        keyword: pick(authored.keyword, accent2),
        operator: pick(authored.operator, accent),
        function: pick(authored.function, accent),
        method: pick(authored.method, accent.mix_oklch(text.primary, 0.25)),
        type_: pick(authored.r#type, ansi.cyan),
        variable: pick(authored.variable, text.primary),
        parameter: pick(authored.parameter, ansi.yellow),
        property: pick(authored.property, tertiary),
        string: pick(authored.string, ansi.green),
        escape: pick(authored.escape, ansi.bright_green),
        number: pick(authored.number, ansi.yellow),
        boolean: pick(authored.boolean, ansi.yellow),
        constant: pick(authored.constant, ansi.bright_magenta),
        // Comments must stay readable: clamp upward until they clear 3.0:1.
        comment: pick(authored.comment, text.muted.boost_contrast_to(base, COMMENT_MIN_CONTRAST)),
        punctuation: pick(authored.punctuation, palette.surfaces.overlay1),
        tag: pick(authored.tag, accent2),
        attribute: pick(authored.attribute, ansi.yellow),
        macro_: pick(authored.r#macro, semantic.hint),
        invalid: pick(authored.invalid, semantic.error),
    }
}

/// Derives the complete diff layer, preferring author-provided values.
pub fn derive_diff(palette: &PaletteTokens, authored: Option<&PartialDiffTokens>) -> DiffTokens {
    let authored = authored.cloned().unwrap_or_default();
    let base = palette.surfaces.base;
    let semantic = &palette.semantic;
    let pick = |authored: Option<Color>, derived: Color| authored.unwrap_or(derived);

    DiffTokens {
        add_fg: pick(authored.add_fg, semantic.success),
        add_bg: pick(authored.add_bg, base.mix_oklch(semantic.success, 0.16)),
        delete_fg: pick(authored.delete_fg, semantic.error),
        delete_bg: pick(authored.delete_bg, base.mix_oklch(semantic.error, 0.16)),
        change_fg: pick(authored.change_fg, semantic.warning),
        change_bg: pick(authored.change_bg, base.mix_oklch(semantic.warning, 0.16)),
        inline_add_bg: pick(authored.inline_add_bg, base.mix_oklch(semantic.success, 0.28)),
        inline_delete_bg: pick(authored.inline_delete_bg, base.mix_oklch(semantic.error, 0.28)),
    }
}

/// Derives the complete window-chrome layer, preferring author-provided values.
pub fn derive_ui(palette: &PaletteTokens, authored: Option<&PartialUiTokens>) -> UiTokens {
    let authored = authored.cloned().unwrap_or_default();
    let surfaces = &palette.surfaces;
    let text = &palette.text;
    let pick = |authored: Option<Color>, derived: Color| authored.unwrap_or(derived);

    let crust_lightness = surfaces.crust.to_oklch().l;

    UiTokens {
        window_frame: pick(authored.window_frame, surfaces.surface2),
        window_frame_unfocused: pick(authored.window_frame_unfocused, surfaces.surface2.mix_oklch(surfaces.base, 0.5)),
        tab_bar_bg: pick(authored.tab_bar_bg, surfaces.crust),
        tab_bar_unfocused_bg: pick(
            authored.tab_bar_unfocused_bg,
            surfaces.crust.with_oklch_lightness(crust_lightness * 0.8),
        ),
        tab_active_bg: pick(authored.tab_active_bg, surfaces.surface0),
        tab_active_fg: pick(authored.tab_active_fg, text.primary),
        tab_inactive_bg: pick(authored.tab_inactive_bg, surfaces.mantle),
        tab_inactive_fg: pick(authored.tab_inactive_fg, surfaces.overlay0),
        statusline_bg: pick(authored.statusline_bg, surfaces.surface0),
        statusline_fg: pick(authored.statusline_fg, text.primary),
        border: pick(authored.border, surfaces.surface2),
        border_focused: pick(authored.border_focused, palette.accents.primary),
        panel_bg: pick(authored.panel_bg, surfaces.base.mix_oklch(surfaces.mantle, 0.5)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::nerv_eva_tokens;

    #[test]
    fn derivation_matches_rfc_rules() {
        let tokens = nerv_eva_tokens();
        let palette = &tokens.palette;
        // Derive with no authored L2 so the RFC rules themselves are exercised.
        let syntax = derive_syntax(palette, None);

        assert_eq!(syntax.keyword, palette.accents.secondary);
        assert_eq!(syntax.function, palette.accents.primary);
        assert_eq!(syntax.string, palette.ansi.green);
        assert_eq!(syntax.number, palette.ansi.yellow);
        assert_eq!(syntax.boolean, palette.ansi.yellow);
        assert_eq!(syntax.type_, palette.ansi.cyan);
        assert_eq!(syntax.tag, palette.accents.secondary);
        assert_eq!(syntax.invalid, palette.semantic.error);
    }

    #[test]
    fn derived_comment_clears_readability_floor() {
        let tokens = nerv_eva_tokens();
        let syntax = derive_syntax(&tokens.palette, None);
        let ratio = syntax.comment.contrast_ratio(tokens.palette.surfaces.base);
        assert!(ratio >= COMMENT_MIN_CONTRAST, "comment contrast {ratio}");
    }

    #[test]
    fn derivation_is_deterministic() {
        let tokens = nerv_eva_tokens();
        let first = derive_syntax(&tokens.palette, None);
        let second = derive_syntax(&tokens.palette, None);
        assert_eq!(first, second);
    }

    #[test]
    fn authored_values_win_over_derivation() {
        let tokens = nerv_eva_tokens();
        let authored = tokens.syntax.as_ref().unwrap();
        let derived = derive_syntax(&tokens.palette, Some(authored));
        assert_eq!(derived.keyword, authored.keyword.unwrap());
        assert_eq!(derived.comment, authored.comment.unwrap());
    }

    #[test]
    fn diff_backgrounds_are_oklch_blends_of_the_canvas() {
        let tokens = nerv_eva_tokens();
        let diff = derive_diff(&tokens.palette, None);
        let base = tokens.palette.surfaces.base;
        let success_lum = tokens.palette.semantic.success.relative_luminance();
        let add_lum = diff.add_bg.relative_luminance();
        assert!(add_lum > base.relative_luminance());
        assert!(add_lum < success_lum);
        assert_eq!(
            diff.add_bg,
            base.mix_oklch(tokens.palette.semantic.success, 0.16)
        );
    }

    #[test]
    fn ui_chrome_follows_surface_ramp() {
        let tokens = nerv_eva_tokens();
        let ui = derive_ui(&tokens.palette, None);
        assert_eq!(ui.tab_bar_bg, tokens.palette.surfaces.crust);
        assert_eq!(ui.tab_active_bg, tokens.palette.surfaces.surface0);
        assert_eq!(ui.tab_inactive_bg, tokens.palette.surfaces.mantle);
        assert_eq!(ui.border_focused, tokens.palette.accents.primary);
        assert!(ui.tab_bar_unfocused_bg.relative_luminance() < ui.tab_bar_bg.relative_luminance());
    }
}
