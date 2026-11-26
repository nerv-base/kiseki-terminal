//! `ResolvedTheme` construction (RFC-0002 §5).
//!
//! Resolution order, later winning:
//! 1. base theme (L2 derived from L1 where omitted)
//! 2. per-layer composition overrides
//! 3. rendering-profile transform
//! 4. explicit user overrides
//! 5. capability negotiation (per adapter, not modelled here)
//!
//! The result is a fully concrete value with no optional L2 fields, so the
//! compiler never needs to know about authoring-time optionality.

use kiseki_types::{Appearance, Color, Oklch, RenderingProfile, ThemeId};

use crate::derive::{derive_diff, derive_syntax, derive_ui};
use crate::lint::{lint_contrast, ContrastReport};
use crate::tokens::{
    AtmosphereTokens, DiffTokens, OverrideTokens, PaletteTokens, SyntaxTokens, ThemePack, TypographyTokens, UiTokens,
};

/// Accessibility lightness target, per RFC-0002 §4.
const ACCESSIBILITY_CONTRAST: f64 = 7.0;

/// Per-layer composition overrides (`kiseki compose`).
///
/// Each field names a *source theme* whose corresponding layer is spliced in.
/// `palette` is special: when it changes without an explicit `syntax` override,
/// L2 is re-derived from the new palette.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Composition {
    pub palette: Option<ThemeId>,
    pub syntax: Option<ThemeId>,
    pub atmosphere: Option<ThemeId>,
    pub typography: Option<ThemeId>,
    pub overrides: Option<ThemeId>,
}

impl Composition {
    /// True when nothing is overridden.
    pub fn is_empty(&self) -> bool {
        self.palette.is_none()
            && self.syntax.is_none()
            && self.atmosphere.is_none()
            && self.typography.is_none()
            && self.overrides.is_none()
    }

    /// Every theme ID referenced by this composition, in a stable order.
    pub fn referenced_themes(&self) -> Vec<&ThemeId> {
        [
            self.palette.as_ref(),
            self.syntax.as_ref(),
            self.atmosphere.as_ref(),
            self.typography.as_ref(),
            self.overrides.as_ref(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// Explicit user overrides applied after the profile transform.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserOverrides {
    /// `kiseki bg <0.0..1.0>` — overrides wallpaper and shader opacity.
    pub bg_opacity: Option<f64>,
    /// `kiseki crt on|off`.
    pub crt: Option<bool>,
    /// When true, a theme switch must not replace the user's font.
    ///
    /// It never suppresses `font_style`/`font_size_pt`: those overrides *are*
    /// the locked choice, so applying them is the whole point of the lock.
    pub font_locked: bool,
    /// `kiseki font <style>`.
    pub font_style: Option<String>,
    /// `kiseki font <style> <size>`.
    pub font_size_pt: Option<f64>,
}

/// Authoring-time L1/L2 bundle passed to the internal builder.
type AuthoredLayers = (
    PaletteTokens,
    Option<crate::tokens::PartialSyntaxTokens>,
    Option<crate::tokens::PartialDiffTokens>,
    Option<crate::tokens::PartialUiTokens>,
);

/// A fully resolved theme: every layer concrete, ready for the compiler.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTheme {
    pub id: ThemeId,
    pub name: String,
    pub appearance: Appearance,
    pub profile: RenderingProfile,
    pub palette: PaletteTokens,
    pub syntax: SyntaxTokens,
    pub diff: DiffTokens,
    pub ui: UiTokens,
    pub atmosphere: AtmosphereTokens,
    pub typography: TypographyTokens,
    /// `None` when the theme authors no L5 overrides.
    pub overrides: Option<OverrideTokens>,
    /// True when the accessibility transform repaired palette colors.
    pub accessibility_adjusted: bool,
    /// True when a dynamic shader should be mounted.
    pub shader_enabled: bool,
    /// True when CRT post-processing is layered on.
    pub crt_enabled: bool,
    /// Effective wallpaper opacity after profile and user overrides.
    pub wallpaper_opacity: Option<f64>,
    /// Effective window opacity after profile and user overrides.
    pub window_opacity: f64,
}

impl ResolvedTheme {
    /// Resolves a single theme at a profile with no composition or overrides.
    pub fn from_pack(pack: &ThemePack, profile: RenderingProfile) -> Self {
        Self::build(pack, profile, &Composition::default(), &UserOverrides::default(), None, None, None)
    }

    /// Full resolution with composition, profile, user overrides and lint.
    ///
    /// `composition_sources` supplies the packs referenced by `composition`;
    /// the builder reports a diagnostic-friendly error string when one is
    /// missing.
    pub fn resolve(
        pack: &ThemePack,
        profile: RenderingProfile,
        composition: &Composition,
        user: &UserOverrides,
        composition_sources: &[ThemePack],
    ) -> Result<Self, String> {
        let lookup = |id: &ThemeId| {
            composition_sources
                .iter()
                .find(|candidate| &candidate.manifest.id == id)
                .ok_or_else(|| format!("composition references unknown theme '{id}'"))
        };

        let palette_pack = match &composition.palette {
            Some(id) => lookup(id)?,
            None => pack,
        };
        let syntax_pack = match &composition.syntax {
            Some(id) => lookup(id)?,
            None => palette_pack,
        };
        let atmosphere_pack = match &composition.atmosphere {
            Some(id) => lookup(id)?,
            None => pack,
        };
        let typography_pack = match &composition.typography {
            Some(id) => lookup(id)?,
            None => pack,
        };
        let overrides_pack = match &composition.overrides {
            Some(id) => lookup(id)?,
            None => pack,
        };

        let palette = palette_pack.tokens.palette.clone();
        // When the palette layer changes and no explicit syntax layer is
        // composed, derive L2 from the *new* palette (RFC-0002 §5 rule 2).
        let authored_syntax = if composition.syntax.is_none() && composition.palette.is_some() {
            palette_pack.tokens.syntax.as_ref()
        } else {
            syntax_pack.tokens.syntax.as_ref()
        };
        let authored_diff = if composition.syntax.is_none() && composition.palette.is_some() {
            palette_pack.tokens.diff.as_ref()
        } else {
            syntax_pack.tokens.diff.as_ref()
        };
        let authored_ui = if composition.syntax.is_none() && composition.palette.is_some() {
            palette_pack.tokens.ui.as_ref()
        } else {
            syntax_pack.tokens.ui.as_ref()
        };

        let atmosphere = atmosphere_pack.tokens.atmosphere.clone().unwrap_or_default();
        let typography = typography_pack
            .tokens
            .typography
            .clone()
            .unwrap_or_else(default_typography);
        let overrides = overrides_pack.tokens.overrides.clone();

        Ok(Self::build(
            pack,
            profile,
            composition,
            user,
            Some((palette, authored_syntax.cloned(), authored_diff.cloned(), authored_ui.cloned())),
            Some(atmosphere),
            Some((typography, overrides)),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        pack: &ThemePack,
        profile: RenderingProfile,
        _composition: &Composition,
        user: &UserOverrides,
        layers: Option<AuthoredLayers>,
        atmosphere: Option<AtmosphereTokens>,
        extras: Option<(TypographyTokens, Option<OverrideTokens>)>,
    ) -> Self {
        let (palette, authored_syntax, authored_diff, authored_ui) = layers.unwrap_or_else(|| {
            (
                pack.tokens.palette.clone(),
                pack.tokens.syntax.clone(),
                pack.tokens.diff.clone(),
                pack.tokens.ui.clone(),
            )
        });
        let atmosphere = atmosphere.unwrap_or_else(|| pack.tokens.atmosphere.clone().unwrap_or_default());
        let (typography, overrides) = extras.unwrap_or_else(|| {
            (
                pack.tokens.typography.clone().unwrap_or_else(default_typography),
                pack.tokens.overrides.clone(),
            )
        });

        let mut syntax = derive_syntax(&palette, authored_syntax.as_ref());
        let mut diff = derive_diff(&palette, authored_diff.as_ref());
        let mut ui = derive_ui(&palette, authored_ui.as_ref());
        let mut palette = palette;
        let mut atmosphere = atmosphere;
        let mut typography = typography;
        let mut accessibility_adjusted = false;

        // --- 3. Rendering-profile transform ---------------------------------
        let mut shader_enabled = profile.is_dynamic();
        let mut crt_enabled = user.crt.unwrap_or(false);

        if profile.is_accessibility() {
            // Force every required pair to WCAG AAA by lifting OKLCH lightness.
            let adjusted = repair_palette_for_aaa(&mut palette);
            accessibility_adjusted = adjusted;
            syntax = derive_syntax(&palette, authored_syntax.as_ref());
            diff = derive_diff(&palette, authored_diff.as_ref());
            ui = derive_ui(&palette, authored_ui.as_ref());
            // Authored L2 colors are repaired too: a theme's hand-picked
            // syntax color must not be the one thing that stays illegible.
            accessibility_adjusted |= repair_layer_colors(&palette, &mut syntax, &mut diff, &mut ui);
            shader_enabled = false;
            crt_enabled = false;
            // No wallpaper, blur or transparency at all.
            atmosphere.wallpaper = None;
            if let Some(window) = atmosphere.window.as_mut() {
                window.blur = false;
                window.use_mica = false;
                window.opacity = 1.0;
                window.unfocused_opacity = None;
            }
            atmosphere.shader = None;
        } else if !shader_enabled {
            // `static`: keep the wallpaper, drop the shader.
            atmosphere.shader = None;
            crt_enabled = false;
        }

        // --- 4. Explicit user overrides -------------------------------------
        let mut wallpaper_opacity = atmosphere
            .wallpaper
            .as_ref()
            .and_then(|wallpaper| wallpaper.opacity);
        let mut window_opacity = atmosphere
            .window
            .as_ref()
            .map(|window| window.opacity)
            .unwrap_or(1.0);

        if let Some(opacity) = user.bg_opacity {
            let opacity = opacity.clamp(0.0, 1.0);
            if let Some(wallpaper) = atmosphere.wallpaper.as_mut() {
                wallpaper.opacity = Some(opacity);
                wallpaper.unfocused_opacity = Some(opacity * 0.6);
            }
            wallpaper_opacity = Some(opacity);
            // Keep the shader's compositing in sync with the visible wallpaper.
            window_opacity = window_opacity.max(opacity);
        }
        if let Some(crt) = user.crt {
            crt_enabled = crt && shader_enabled;
        }
        // A font lock pins the user's choice: it means "do not let a theme
        // switch replace my font", never "ignore my own override". The
        // overrides below therefore always win over the theme's typography.
        if let Some(style) = &user.font_style {
            typography.families = vec![style.clone()];
        }
        if let Some(size) = user.font_size_pt {
            typography.size_pt = Some(size.clamp(6.0, 36.0));
        }

        // A `static` profile still honours an explicit CRT request only when a
        // shader is mounted; `recover --safe` relies on that being impossible.
        if !shader_enabled {
            crt_enabled = false;
        }

        Self {
            id: pack.manifest.id.clone(),
            name: pack.manifest.name.clone(),
            appearance: pack.manifest.appearance,
            profile,
            palette,
            syntax,
            diff,
            ui,
            atmosphere,
            typography,
            overrides,
            accessibility_adjusted,
            shader_enabled,
            crt_enabled,
            wallpaper_opacity,
            window_opacity,
        }
    }

    /// The wallpaper asset path, if the profile still renders one.
    pub fn wallpaper_path(&self) -> Option<&str> {
        self.atmosphere.wallpaper.as_ref().map(|wallpaper| wallpaper.path.as_str())
    }

    /// The GLSL template path, if a shader is mounted.
    pub fn glsl_path(&self) -> Option<&str> {
        self.atmosphere
            .shader
            .as_ref()
            .and_then(|shader| shader.glsl.as_deref())
    }

    /// The HLSL template path, if a shader is mounted.
    pub fn hlsl_path(&self) -> Option<&str> {
        self.atmosphere
            .shader
            .as_ref()
            .and_then(|shader| shader.hlsl.as_deref())
    }

    /// WCAG report for the resolved palette.
    pub fn contrast_report(&self) -> ContrastReport {
        lint_contrast(&self.palette, Some(&self.syntax))
    }

    /// True when every WCAG error threshold is met.
    pub fn meets_wcag(&self) -> bool {
        self.contrast_report().is_compliant()
    }

    /// The base canvas color used by shader macros.
    pub fn base_bg(&self) -> Color {
        self.palette.surfaces.base
    }

    /// End of the code-safe zone as a `0.0..=1.0` fraction.
    pub fn safe_zone_end_x(&self) -> f64 {
        self.atmosphere
            .wallpaper
            .as_ref()
            .and_then(|wallpaper| wallpaper.code_safe_zone)
            .map(|zone| zone.end_x)
            .unwrap_or(0.56)
    }

    /// Narrow-window fade threshold from `shader.params`, defaulting to 1.15.
    pub fn narrow_fade_aspect(&self) -> f64 {
        self.atmosphere
            .shader
            .as_ref()
            .and_then(|shader| shader.params.get("narrowFadeAspect"))
            .and_then(|value| match value {
                crate::tokens::ShaderParamValue::Number(number) => Some(*number),
                _ => None,
            })
            .unwrap_or(1.15)
    }
}

/// Lifts any L2 foreground color that fails AAA against the canvas.
///
/// Diff backgrounds are deliberately left alone: they are surfaces, not text,
/// and lifting them would erase the add/delete affordance.
fn repair_layer_colors(
    palette: &PaletteTokens,
    syntax: &mut SyntaxTokens,
    diff: &mut DiffTokens,
    ui: &mut UiTokens,
) -> bool {
    let base = palette.surfaces.base;
    let mut changed = false;
    let mut lift = |color: &mut Color| {
        let boosted = color.boost_contrast_to(base, ACCESSIBILITY_CONTRAST);
        if boosted != *color {
            *color = boosted;
            changed = true;
        }
    };

    for color in [
        &mut syntax.keyword,
        &mut syntax.operator,
        &mut syntax.function,
        &mut syntax.method,
        &mut syntax.type_,
        &mut syntax.variable,
        &mut syntax.parameter,
        &mut syntax.property,
        &mut syntax.string,
        &mut syntax.escape,
        &mut syntax.number,
        &mut syntax.boolean,
        &mut syntax.constant,
        &mut syntax.comment,
        &mut syntax.punctuation,
        &mut syntax.tag,
        &mut syntax.attribute,
        &mut syntax.macro_,
        &mut syntax.invalid,
    ] {
        lift(color);
    }
    for color in [&mut diff.add_fg, &mut diff.delete_fg, &mut diff.change_fg] {
        lift(color);
    }
    for color in [
        &mut ui.window_frame,
        &mut ui.border,
        &mut ui.border_focused,
        &mut ui.tab_active_fg,
        &mut ui.tab_inactive_fg,
        &mut ui.statusline_fg,
    ] {
        lift(color);
    }
    changed
}

fn default_typography() -> TypographyTokens {
    TypographyTokens {
        style: None,
        families: vec!["monospace".to_string()],
        size_pt: Some(11.0),
        line_height: Some(1.2),
        weight: None,
        ligatures: None,
    }
}

/// Lifts palette colors on the OKLCH lightness axis until every required pair
/// clears WCAG AAA. Returns true when anything actually changed.
fn repair_palette_for_aaa(palette: &mut PaletteTokens) -> bool {
    let mut changed = false;
    let base = palette.surfaces.base;

    let mut lift = |color: &mut Color, target: f64| {
        let boosted = color.boost_contrast_to(base, target);
        if boosted != *color {
            *color = boosted;
            changed = true;
        }
    };

    lift(&mut palette.text.primary, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.text.secondary, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.text.muted, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.accents.primary, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.accents.secondary, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.accents.tertiary, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.semantic.error, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.semantic.warning, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.semantic.success, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.semantic.info, ACCESSIBILITY_CONTRAST);
    lift(&mut palette.semantic.hint, ACCESSIBILITY_CONTRAST);

    for color in [
        &mut palette.ansi.red,
        &mut palette.ansi.green,
        &mut palette.ansi.yellow,
        &mut palette.ansi.blue,
        &mut palette.ansi.magenta,
        &mut palette.ansi.cyan,
    ] {
        lift(color, ACCESSIBILITY_CONTRAST);
    }
    // Bright variants are already lighter; lift them to the same floor so the
    // whole ANSI palette is uniformly legible.
    for color in [
        &mut palette.ansi.bright_red,
        &mut palette.ansi.bright_green,
        &mut palette.ansi.bright_yellow,
        &mut palette.ansi.bright_blue,
        &mut palette.ansi.bright_magenta,
        &mut palette.ansi.bright_cyan,
    ] {
        lift(color, ACCESSIBILITY_CONTRAST);
    }

    // Selection and cursor pairs need internal repair, not canvas repair.
    let selection_bg = palette.interaction.selection_bg;
    let selection_fg = repair_pair(palette.interaction.selection_fg, selection_bg, ACCESSIBILITY_CONTRAST);
    if selection_fg != palette.interaction.selection_fg {
        palette.interaction.selection_fg = selection_fg;
        changed = true;
    }
    let cursor = palette.interaction.cursor;
    let cursor_text = repair_pair(palette.interaction.cursor_text, cursor, ACCESSIBILITY_CONTRAST);
    if cursor_text != palette.interaction.cursor_text {
        palette.interaction.cursor_text = cursor_text;
        changed = true;
    }
    // If the foreground is already at the extreme end of the lightness axis
    // (pure black text on a saturated cursor), the *background* must move.
    if cursor_text.contrast_ratio(cursor) < ACCESSIBILITY_CONTRAST {
        let lifted = lift_away_from(cursor, cursor_text, ACCESSIBILITY_CONTRAST);
        if lifted != palette.interaction.cursor {
            palette.interaction.cursor = lifted;
            changed = true;
        }
    }

    changed
}

/// Boosts `fg` against `bg`, then boosts `bg` away from `fg` if needed.
fn repair_pair(fg: Color, bg: Color, target: f64) -> Color {
    let boosted = fg.boost_contrast_to(bg, target);
    if boosted.contrast_ratio(bg) >= target {
        boosted
    } else {
        fg
    }
}

/// Pushes `subject` away from `anchor` on the OKLCH lightness axis until the
/// pair clears `target`. Used when the foreground cannot move any further.
fn lift_away_from(subject: Color, anchor: Color, target: f64) -> Color {
    if subject.contrast_ratio(anchor) >= target {
        return subject;
    }
    let base = subject.to_oklch();
    let brighten = anchor.relative_luminance() < 0.5;
    let mut low = if brighten { base.l } else { 0.0 };
    let mut high = if brighten { 1.0 } else { base.l };
    let mut best = subject;
    for _ in 0..24 {
        let mid = (low + high) / 2.0;
        let candidate = Oklch { l: mid, ..base }.to_color();
        if candidate.contrast_ratio(anchor) >= target {
            best = candidate;
            if brighten {
                high = mid;
            } else {
                low = mid;
            }
        } else if brighten {
            low = mid;
        } else {
            high = mid;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::nerv_eva_pack;

    #[test]
    fn balanced_resolution_mounts_shader_and_wallpaper() {
        let theme = ResolvedTheme::from_pack(&nerv_eva_pack(), RenderingProfile::Balanced);
        assert!(theme.shader_enabled);
        assert_eq!(theme.glsl_path(), Some("shaders/live.glsl"));
        assert_eq!(theme.hlsl_path(), Some("shaders/live.hlsl"));
        assert_eq!(theme.wallpaper_path(), Some("assets/wallpaper.png"));
        assert!(!theme.crt_enabled);
        assert!(!theme.accessibility_adjusted);
    }

    #[test]
    fn static_profile_drops_shader_but_keeps_wallpaper() {
        let theme = ResolvedTheme::from_pack(&nerv_eva_pack(), RenderingProfile::Static);
        assert!(!theme.shader_enabled);
        assert_eq!(theme.glsl_path(), None);
        assert_eq!(theme.wallpaper_path(), Some("assets/wallpaper.png"));
        assert!(!theme.crt_enabled);
    }

    #[test]
    fn accessibility_disables_wallpaper_blur_transparency_and_shader() {
        let theme = ResolvedTheme::from_pack(&nerv_eva_pack(), RenderingProfile::Accessibility);
        assert!(!theme.shader_enabled);
        assert_eq!(theme.wallpaper_path(), None);
        assert_eq!(theme.wallpaper_opacity, None);
        assert_eq!(theme.window_opacity, 1.0);
        assert!(!theme.crt_enabled);
        let window = theme.atmosphere.window.as_ref().unwrap();
        assert!(!window.blur);
        assert!(!window.use_mica);
    }

    #[test]
    fn accessibility_reaches_wcag_aaa_everywhere() {
        let theme = ResolvedTheme::from_pack(&nerv_eva_pack(), RenderingProfile::Accessibility);
        let report = theme.contrast_report();
        let failures: Vec<String> = report
            .checks
            .iter()
            .filter(|check| !check.passes_minimum())
            .map(|check| format!("{}={:.2}", check.name, check.ratio))
            .collect();
        assert!(report.is_compliant(), "accessibility failures: {failures:?}");
        assert!(theme.accessibility_adjusted);
        for check in &report.checks {
            assert!(
                check.ratio >= ACCESSIBILITY_CONTRAST - 0.01,
                "{} only reached {:.2}:1",
                check.name,
                check.ratio
            );
        }
    }

    #[test]
    fn balanced_theme_also_meets_wcag_errors() {
        let theme = ResolvedTheme::from_pack(&nerv_eva_pack(), RenderingProfile::Balanced);
        assert!(theme.meets_wcag(), "{:?}", theme.contrast_report());
    }

    #[test]
    fn bg_opacity_override_reaches_shader_and_wallpaper() {
        let pack = nerv_eva_pack();
        let user = UserOverrides {
            bg_opacity: Some(0.55),
            ..Default::default()
        };
        let theme =
            ResolvedTheme::resolve(&pack, RenderingProfile::Balanced, &Composition::default(), &user, &[])
                .unwrap();
        assert_eq!(theme.wallpaper_opacity, Some(0.55));
        assert_eq!(theme.atmosphere.wallpaper.as_ref().unwrap().opacity, Some(0.55));
        assert!(theme.window_opacity >= 0.55);
    }

    #[test]
    fn crt_requires_a_mounted_shader() {
        let pack = nerv_eva_pack();
        let user = UserOverrides {
            crt: Some(true),
            ..Default::default()
        };
        let balanced =
            ResolvedTheme::resolve(&pack, RenderingProfile::Balanced, &Composition::default(), &user, &[])
                .unwrap();
        assert!(balanced.crt_enabled);

        let static_theme =
            ResolvedTheme::resolve(&pack, RenderingProfile::Static, &Composition::default(), &user, &[])
                .unwrap();
        assert!(!static_theme.crt_enabled, "static must never enable CRT");
    }

    #[test]
    fn font_overrides_always_win_over_the_theme() {
        let pack = nerv_eva_pack();
        let unlocked = UserOverrides {
            font_style: Some("monospace".into()),
            font_size_pt: Some(14.0),
            ..Default::default()
        };
        let theme =
            ResolvedTheme::resolve(&pack, RenderingProfile::Balanced, &Composition::default(), &unlocked, &[])
                .unwrap();
        assert_eq!(theme.typography.families, vec!["monospace".to_string()]);
        assert_eq!(theme.typography.size_pt, Some(14.0));

        // `font_locked` preserves the user's font across `kiseki use` switches,
        // so a locked override must still be applied.
        let locked = UserOverrides {
            font_locked: true,
            font_style: Some("monospace".into()),
            font_size_pt: Some(14.0),
            ..Default::default()
        };
        let theme =
            ResolvedTheme::resolve(&pack, RenderingProfile::Balanced, &Composition::default(), &locked, &[])
                .unwrap();
        assert_eq!(theme.typography.families, vec!["monospace".to_string()]);
        assert_eq!(theme.typography.size_pt, Some(14.0));

        // With no override the theme's typography is untouched either way.
        let theme = ResolvedTheme::resolve(
            &pack,
            RenderingProfile::Balanced,
            &Composition::default(),
            &UserOverrides {
                font_locked: true,
                ..Default::default()
            },
            &[],
        )
        .unwrap();
        assert_eq!(theme.typography.families, vec!["jetbrains-mono-nf", "maple-mono-nf-cn"]);
        assert_eq!(theme.typography.size_pt, Some(11.0));
    }

    #[test]
    fn composition_splices_atmosphere_from_another_theme() {
        let base = nerv_eva_pack();
        let mut other = nerv_eva_pack();
        other.manifest.id = ThemeId::new("shinkai-rain").unwrap();
        other.tokens.atmosphere.as_mut().unwrap().wallpaper.as_mut().unwrap().path =
            "assets/rain.png".into();

        let composition = Composition {
            atmosphere: Some(ThemeId::new("shinkai-rain").unwrap()),
            ..Default::default()
        };
        let theme = ResolvedTheme::resolve(
            &base,
            RenderingProfile::Balanced,
            &composition,
            &UserOverrides::default(),
            &[other],
        )
        .unwrap();
        assert_eq!(theme.wallpaper_path(), Some("assets/rain.png"));
        // Palette stays with the base theme.
        assert_eq!(theme.palette.surfaces.base, base.tokens.palette.surfaces.base);
        assert_eq!(theme.id.as_str(), "nerv-eva");
    }

    #[test]
    fn composition_reports_unknown_theme() {
        let base = nerv_eva_pack();
        let composition = Composition {
            palette: Some(ThemeId::new("does-not-exist").unwrap()),
            ..Default::default()
        };
        let error = ResolvedTheme::resolve(
            &base,
            RenderingProfile::Balanced,
            &composition,
            &UserOverrides::default(),
            &[],
        )
        .unwrap_err();
        assert!(error.contains("does-not-exist"), "{error}");
    }

    #[test]
    fn composition_palette_rederives_l2() {
        let base = nerv_eva_pack();
        let mut alt = nerv_eva_pack();
        alt.manifest.id = ThemeId::new("kyoto-sakura").unwrap();
        alt.tokens.syntax = None;
        alt.tokens.palette.accents.primary = Color::parse_hex("#FF9EC7").unwrap();

        let composition = Composition {
            palette: Some(ThemeId::new("kyoto-sakura").unwrap()),
            ..Default::default()
        };
        let theme = ResolvedTheme::resolve(
            &base,
            RenderingProfile::Balanced,
            &composition,
            &UserOverrides::default(),
            &[alt],
        )
        .unwrap();
        // Derived syntax.function tracks the composed palette accent, not nerv-eva's.
        assert_eq!(theme.syntax.function, Color::parse_hex("#FF9EC7").unwrap());
    }

    #[test]
    fn composition_helpers() {
        let empty = Composition::default();
        assert!(empty.is_empty());
        assert!(empty.referenced_themes().is_empty());

        let composition = Composition {
            palette: Some(ThemeId::new("a-b").unwrap()),
            atmosphere: Some(ThemeId::new("c-d").unwrap()),
            ..Default::default()
        };
        assert!(!composition.is_empty());
        assert_eq!(composition.referenced_themes().len(), 2);
    }

    #[test]
    fn shader_macros_read_declared_geometry() {
        let theme = ResolvedTheme::from_pack(&nerv_eva_pack(), RenderingProfile::Balanced);
        assert!((theme.safe_zone_end_x() - 0.56).abs() < f64::EPSILON);
        assert!((theme.narrow_fade_aspect() - 1.15).abs() < f64::EPSILON);
    }

    #[test]
    fn resolution_is_deterministic() {
        let pack = nerv_eva_pack();
        let first = ResolvedTheme::from_pack(&pack, RenderingProfile::Ultra);
        let second = ResolvedTheme::from_pack(&pack, RenderingProfile::Ultra);
        assert_eq!(first, second);
    }
}
