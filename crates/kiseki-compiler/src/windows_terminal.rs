//! Windows Terminal visual patch payload (RFC-0003 §5.1).
//!
//! Windows Terminal has no include mechanism, so Kiseki patches `settings.json`
//! in place. This module only *builds the payload*; the comment-preserving CST
//! edit lives in `kiseki-adapters`. Keeping the payload declarative is what lets
//! the adapter restrict itself to the managed key whitelist.

use std::path::Path;

use kiseki_schema::{CursorShape, FontWeight, ResolvedTheme};
use kiseki_types::{Appearance, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Every Windows Terminal scheme name Kiseki owns starts with this prefix.
pub const OWNED_PREFIX: &str = "Kiseki: ";

/// Keys under `profiles.defaults` that Kiseki is allowed to manage.
///
/// Windows Terminal spells nested settings as *flat dotted keys*
/// (`"experimental.pixelShaderPath"`), so the literal dotted key is what the
/// payload, the whitelist and the baseline all use.
pub const MANAGED_DEFAULTS_KEYS: &[&str] = &[
    "colorScheme",
    "font",
    "cursorShape",
    "opacity",
    "useAcrylic",
    "padding",
    "unfocusedAppearance",
    "backgroundImage",
    "backgroundImageOpacity",
    "backgroundImageAlignment",
    "backgroundImageStretchMode",
    "experimental.pixelShaderPath",
    "experimental.pixelShaderImagePath",
];

/// Object-valued managed keys, together with the *only* leaves Kiseki owns.
///
/// Every other property inside these objects belongs to the user and must
/// survive a patch (`font.features`, `font.axes`, ...) along with its comments.
pub const MANAGED_NESTED_KEYS: &[(&str, &[&str])] = &[
    ("font", &["face", "size", "weight"]),
    ("unfocusedAppearance", &["opacity"]),
];

/// The managed leaves of `key`, when it is a leaf-managed object.
pub fn managed_leaves(key: &str) -> Option<&'static [&'static str]> {
    MANAGED_NESTED_KEYS
        .iter()
        .find(|(parent, _)| *parent == key)
        .map(|(_, leaves)| *leaves)
}

/// Keys Kiseki must never touch (behaviour, not appearance).
pub const FORBIDDEN_KEYS: &[&str] = &[
    "scrollbarState",
    "bellStyle",
    "intenseTextStyle",
    "startingDirectory",
    "commandline",
    "keybindings",
    "actions",
    "closeOnExit",
    "suppressApplicationTitle",
];

/// The declarative patch payload, serialized to `generated/wt-visual.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WtVisualPayload {
    pub schema_version: u32,
    pub theme_id: String,
    /// Complete `schemes[]` entry owned by Kiseki.
    pub scheme: Value,
    /// Complete `themes[]` entry owned by Kiseki (Windows Terminal 1.20+).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_theme: Option<Value>,
    /// Values to upsert under `profiles.defaults`.
    pub defaults: Map<String, Value>,
    /// Value for the top-level `theme` key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_level_theme: Option<String>,
}

impl WtVisualPayload {
    pub fn scheme_name(&self) -> &str {
        self.scheme
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// Parses the generated payload back from JSON.
    pub fn from_json(raw: &str) -> Result<Self> {
        serde_json::from_str(raw).map_err(|error| kiseki_types::KisekiError::Json(error.to_string()))
    }

    /// Rejects payloads that would smuggle a forbidden key past the adapter.
    pub fn lint(&self) -> Vec<kiseki_types::Diagnostic> {
        let mut diagnostics = Vec::new();
        for key in self.defaults.keys() {
            if FORBIDDEN_KEYS.contains(&key.as_str()) {
                diagnostics.push(kiseki_types::Diagnostic::error(
                    "adapter.wt.forbidden-key",
                    format!("generated payload must not manage `{key}`"),
                ));
            } else if !MANAGED_DEFAULTS_KEYS.contains(&key.as_str()) {
                diagnostics.push(kiseki_types::Diagnostic::error(
                    "adapter.wt.unmanaged-key",
                    format!("generated payload manages non-whitelisted key `{key}`"),
                ));
            }
        }
        diagnostics
    }
}

/// Builds the payload for a resolved theme.
pub fn build(
    theme: &ResolvedTheme,
    font_families: &[String],
    wallpaper: Option<&Path>,
    shader: Option<&Path>,
) -> WtVisualPayload {
    let scheme_name = format!("{OWNED_PREFIX}{}", theme.name);
    let palette = &theme.palette;
    let ansi = &palette.ansi;

    let scheme = json!({
        "name": scheme_name,
        "background": palette.surfaces.base.to_hex(),
        "foreground": palette.text.primary.to_hex(),
        "cursorColor": palette.interaction.cursor.to_hex(),
        "selectionBackground": palette.interaction.selection_bg.to_hex(),
        "black": ansi.black.to_hex(),
        "red": ansi.red.to_hex(),
        "green": ansi.green.to_hex(),
        "yellow": ansi.yellow.to_hex(),
        "blue": ansi.blue.to_hex(),
        // Windows Terminal still spells magenta as `purple`.
        "purple": ansi.magenta.to_hex(),
        "cyan": ansi.cyan.to_hex(),
        "white": ansi.white.to_hex(),
        "brightBlack": ansi.bright_black.to_hex(),
        "brightRed": ansi.bright_red.to_hex(),
        "brightGreen": ansi.bright_green.to_hex(),
        "brightYellow": ansi.bright_yellow.to_hex(),
        "brightBlue": ansi.bright_blue.to_hex(),
        "brightPurple": ansi.bright_magenta.to_hex(),
        "brightCyan": ansi.bright_cyan.to_hex(),
        "brightWhite": ansi.bright_white.to_hex(),
    });

    let ui = &theme.ui;
    let ui_theme = json!({
        "name": scheme_name,
        "tab": {
            "background": ui.tab_active_bg.to_hex(),
            "unfocusedBackground": ui.tab_inactive_bg.to_hex(),
        },
        "tabRow": {
            "background": ui.tab_bar_bg.to_hex(),
            "unfocusedBackground": ui.tab_bar_unfocused_bg.to_hex(),
        },
        "window": {
            "applicationTheme": match theme.appearance {
                Appearance::Dark => "dark",
                Appearance::Light => "light",
            },
            "frame": ui.window_frame.to_hex(),
            "unfocusedFrame": ui.window_frame_unfocused.to_hex(),
        },
    });

    let mut defaults = Map::new();
    defaults.insert("colorScheme".to_string(), json!(scheme_name));

    let face = font_families
        .first()
        .cloned()
        .unwrap_or_else(|| "monospace".to_string());
    let mut font = Map::new();
    font.insert("face".to_string(), json!(face));
    if let Some(size) = theme.typography.size_pt {
        font.insert("size".to_string(), json!(size));
    }
    if let Some(weight) = theme.typography.weight {
        font.insert("weight".to_string(), json!(wt_weight(weight)));
    }
    defaults.insert("font".to_string(), Value::Object(font));

    let cursor = theme
        .overrides
        .as_ref()
        .and_then(|overrides| overrides.terminal.as_ref())
        .and_then(|terminal| terminal.cursor_shape)
        .unwrap_or(CursorShape::Block);
    defaults.insert("cursorShape".to_string(), json!(cursor.windows_terminal()));
    defaults.insert(
        "opacity".to_string(),
        json!(percent(theme.window_opacity)),
    );

    if let Some(window) = theme.atmosphere.window.as_ref() {
        defaults.insert("useAcrylic".to_string(), json!(window.blur));
        if let Some(padding) = window.padding {
            defaults.insert(
                "padding".to_string(),
                json!(format!("{}, {}, {}, {}", padding.x, padding.y, padding.x, padding.y)),
            );
        }
        if let Some(unfocused) = window.unfocused_opacity {
            defaults.insert(
                "unfocusedAppearance".to_string(),
                json!({ "opacity": percent(unfocused) }),
            );
        }
    }

    let active_wallpaper =
        wallpaper.filter(|_| theme.wallpaper_opacity.is_none_or(|opacity| opacity > 0.0));

    if let Some(shader) = shader {
        // Windows Terminal reads these as literal dotted keys directly under
        // `profiles.defaults`, not as a nested `experimental` object.
        // Live HLSL shaders composite `imageTexture` (t1) themselves, so omit
        // `backgroundImage*` to avoid rendering the wallpaper twice behind the
        // swap chain.
        defaults.insert(
            "experimental.pixelShaderPath".to_string(),
            json!(shader.to_string_lossy()),
        );
        if let Some(wallpaper) = active_wallpaper {
            defaults.insert(
                "experimental.pixelShaderImagePath".to_string(),
                json!(wallpaper.to_string_lossy()),
            );
        }
    } else if let Some(wallpaper) = active_wallpaper {
        defaults.insert(
            "backgroundImage".to_string(),
            json!(wallpaper.to_string_lossy()),
        );
        if let Some(opacity) = theme.wallpaper_opacity {
            defaults.insert("backgroundImageOpacity".to_string(), json!(opacity));
        }
        if let Some(wallpaper_tokens) = theme.atmosphere.wallpaper.as_ref() {
            if let Some(alignment) = wallpaper_tokens.alignment {
                defaults.insert(
                    "backgroundImageAlignment".to_string(),
                    json!(wt_alignment(alignment.as_str())),
                );
            }
            if let Some(fit) = wallpaper_tokens.fit {
                defaults.insert(
                    "backgroundImageStretchMode".to_string(),
                    json!(wt_stretch(fit.as_str())),
                );
            }
        }
    }

    WtVisualPayload {
        schema_version: 1,
        theme_id: theme.id.to_string(),
        scheme,
        ui_theme: Some(ui_theme),
        defaults,
        top_level_theme: Some(scheme_name),
    }
}

fn percent(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 100.0).round() as u8
}

fn wt_weight(weight: FontWeight) -> &'static str {
    match weight {
        FontWeight::Thin => "thin",
        FontWeight::Light => "light",
        FontWeight::Normal => "normal",
        FontWeight::Medium => "medium",
        FontWeight::Semibold => "semi-bold",
        FontWeight::Bold => "bold",
    }
}

fn wt_alignment(alignment: &str) -> &'static str {
    match alignment {
        "top-left" => "topLeft",
        "top-right" => "topRight",
        "bottom-left" => "bottomLeft",
        "bottom-right" => "bottomRight",
        _ => "center",
    }
}

fn wt_stretch(fit: &str) -> &'static str {
    match fit {
        "contain" => "uniform",
        "fill" => "fill",
        _ => "uniformToFill",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_types::RenderingProfile;
    use std::path::PathBuf;

    fn theme(profile: RenderingProfile) -> ResolvedTheme {
        let pack = ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
        };
        ResolvedTheme::from_pack(&pack, profile)
    }

    #[test]
    fn payload_uses_the_managed_whitelist_only() {
        let theme = theme(RenderingProfile::Balanced);
        let payload = build(
            &theme,
            &["JetBrainsMono Nerd Font".to_string()],
            Some(&PathBuf::from("/themes/nerv-eva/assets/wallpaper.png")),
            Some(&PathBuf::from("/cfg/generated/shaders/active.hlsl")),
        );
        assert!(payload.lint().is_empty(), "{:?}", payload.lint());
        for key in payload.defaults.keys() {
            assert!(MANAGED_DEFAULTS_KEYS.contains(&key.as_str()), "{key}");
        }
        for forbidden in FORBIDDEN_KEYS {
            assert!(!payload.defaults.contains_key(*forbidden), "{forbidden}");
        }
        assert_eq!(payload.scheme_name(), "Kiseki: NERV UNIT-02");
        assert_eq!(payload.top_level_theme.as_deref(), Some("Kiseki: NERV UNIT-02"));
    }

    #[test]
    fn scheme_maps_magenta_to_purple() {
        let payload = build(&theme(RenderingProfile::Balanced), &["monospace".into()], None, None);
        assert_eq!(payload.scheme["purple"], "#E05A6D");
        assert_eq!(payload.scheme["brightPurple"], "#FFA58F");
        assert!(payload.scheme.get("magenta").is_none());
        assert_eq!(payload.scheme["background"], "#0B090A");
    }

    #[test]
    fn defaults_carry_font_cursor_opacity_and_padding() {
        let payload = build(&theme(RenderingProfile::Balanced), &["Maple Mono NF CN".into()], None, None);
        assert_eq!(payload.defaults["font"]["face"], "Maple Mono NF CN");
        assert_eq!(payload.defaults["font"]["size"], 11.0);
        assert_eq!(payload.defaults["font"]["weight"], "normal");
        assert_eq!(payload.defaults["cursorShape"], "filledBox");
        assert_eq!(payload.defaults["opacity"], 95);
        assert_eq!(payload.defaults["useAcrylic"], true);
        assert_eq!(payload.defaults["padding"], "18, 14, 18, 14");
        assert_eq!(payload.defaults["unfocusedAppearance"]["opacity"], 88);
    }

    #[test]
    fn shader_and_wallpaper_paths_are_absolute_passthrough() {
        // Live shader mode binds wallpaper to `experimental.pixelShaderImagePath`
        // and omits static `backgroundImage*` to avoid double-rendering.
        let payload = build(
            &theme(RenderingProfile::Ultra),
            &["monospace".into()],
            Some(&PathBuf::from("/wall.png")),
            Some(&PathBuf::from("/active.hlsl")),
        );
        // Flat dotted keys, exactly as Windows Terminal reads them.
        assert_eq!(
            payload.defaults["experimental.pixelShaderPath"],
            "/active.hlsl"
        );
        assert_eq!(
            payload.defaults["experimental.pixelShaderImagePath"],
            "/wall.png"
        );
        assert!(!payload.defaults.contains_key("experimental"));
        assert!(!payload.defaults.contains_key("backgroundImage"));
        assert!(!payload.defaults.contains_key("backgroundImageOpacity"));
        assert!(payload.lint().is_empty(), "{:?}", payload.lint());

        // Static mode (no shader) sets `backgroundImage*`.
        let static_payload = build(
            &theme(RenderingProfile::Static),
            &["monospace".into()],
            Some(&PathBuf::from("/wall.png")),
            None,
        );
        assert_eq!(static_payload.defaults["backgroundImage"], "/wall.png");
        assert_eq!(
            static_payload.defaults["backgroundImageAlignment"],
            "bottomRight"
        );
        assert_eq!(
            static_payload.defaults["backgroundImageStretchMode"],
            "uniformToFill"
        );
        assert!(!static_payload
            .defaults
            .contains_key("experimental.pixelShaderPath"));
        assert!(static_payload.lint().is_empty(), "{:?}", static_payload.lint());
    }

    #[test]
    fn zero_wallpaper_opacity_omits_wallpaper_and_shader_image() {
        let mut t = theme(RenderingProfile::Balanced);
        t.wallpaper_opacity = Some(0.0);
        let live = build(
            &t,
            &["monospace".into()],
            Some(&PathBuf::from("/wall.png")),
            Some(&PathBuf::from("/active.hlsl")),
        );
        assert_eq!(live.defaults["experimental.pixelShaderPath"], "/active.hlsl");
        assert!(!live.defaults.contains_key("experimental.pixelShaderImagePath"));
        assert!(!live.defaults.contains_key("backgroundImage"));

        let static_off = build(
            &t,
            &["monospace".into()],
            Some(&PathBuf::from("/wall.png")),
            None,
        );
        assert!(!static_off.defaults.contains_key("backgroundImage"));
        assert!(!static_off.defaults.contains_key("backgroundImageOpacity"));
    }

    #[test]
    fn accessibility_payload_has_no_wallpaper_or_shader() {
        let payload = build(&theme(RenderingProfile::Accessibility), &["monospace".into()], None, None);
        assert!(!payload.defaults.contains_key("backgroundImage"));
        assert!(payload
            .defaults
            .keys()
            .all(|key| !key.starts_with("experimental")));
        assert_eq!(payload.defaults["opacity"], 100);
    }

    #[test]
    fn every_managed_nested_leaf_is_a_managed_key() {
        for (parent, leaves) in MANAGED_NESTED_KEYS {
            assert!(MANAGED_DEFAULTS_KEYS.contains(parent), "{parent}");
            assert!(!leaves.is_empty(), "{parent}");
            assert_eq!(managed_leaves(parent), Some(*leaves));
        }
        assert_eq!(managed_leaves("cursorShape"), None);
    }

    #[test]
    fn payload_roundtrips_through_json() {
        let payload = build(&theme(RenderingProfile::Balanced), &["monospace".into()], None, None);
        let encoded = serde_json::to_string(&payload).unwrap();
        let decoded = WtVisualPayload::from_json(&encoded).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn lint_rejects_forbidden_and_unmanaged_keys() {
        let mut payload = build(&theme(RenderingProfile::Balanced), &["monospace".into()], None, None);
        payload.defaults.insert("scrollbarState".into(), json!("hidden"));
        payload.defaults.insert("somethingElse".into(), json!(1));
        let codes: Vec<String> = payload.lint().into_iter().map(|d| d.code).collect();
        assert!(codes.contains(&"adapter.wt.forbidden-key".to_string()));
        assert!(codes.contains(&"adapter.wt.unmanaged-key".to_string()));
    }
}
