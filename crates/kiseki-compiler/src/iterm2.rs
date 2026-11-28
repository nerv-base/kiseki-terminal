//! iTerm2 Dynamic Profile generation with Parent Profile inheritance (RFC-0003 §5.2).
//!
//! Only pure visual properties are emitted (`Background Color`, `Foreground Color`,
//! `Ansi 0..15 Color`, `Normal Font`, `Transparency`, `Blur`, `Background Image Location`).
//! Non-visual behaviour keys (`Silence Bell`, `Unlimited Scrollback`, `Option Key Sends`,
//! `Command`, `Working Directory`) are strictly forbidden so the user inherits
//! 100% of their original profile behaviour via `Dynamic Profile Parent Name`.

use std::path::Path;

use kiseki_schema::{CursorShape, ResolvedTheme};
use kiseki_types::{Color, Diagnostic, KisekiError, Result};
use serde_json::{json, Map, Value};

/// Stable profile name written to `DynamicProfiles/kiseki-terminal.json`.
pub const DYNAMIC_PROFILE_NAME: &str = "Kiseki Active";
/// Stable GUID used so iTerm2 updates the profile in place on every switch.
pub const DYNAMIC_PROFILE_GUID: &str = "kiseki-v2-active";

/// Renders `generated/iterm2-dynamic-profile.json`.
pub fn render(
    theme: &ResolvedTheme,
    font_families: &[String],
    wallpaper: Option<&Path>,
    macos_font_scale: bool,
    parent_name: Option<&str>,
) -> Result<String> {
    let palette = &theme.palette;
    let mut profile = Map::new();
    profile.insert("Name".to_string(), Value::String(DYNAMIC_PROFILE_NAME.to_string()));
    profile.insert("Guid".to_string(), Value::String(DYNAMIC_PROFILE_GUID.to_string()));
    profile.insert("Tags".to_string(), json!(["Kiseki", theme.id.as_str()]));

    if let Some(parent) = parent_name
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != DYNAMIC_PROFILE_NAME)
    {
        profile.insert(
            "Dynamic Profile Parent Name".to_string(),
            Value::String(parent.to_string()),
        );
    }

    profile.insert("Background Color".to_string(), iterm_color(palette.surfaces.base));
    profile.insert("Foreground Color".to_string(), iterm_color(palette.text.primary));
    profile.insert("Bold Color".to_string(), iterm_color(palette.text.primary));
    profile.insert("Cursor Color".to_string(), iterm_color(palette.interaction.cursor));
    profile.insert(
        "Cursor Text Color".to_string(),
        iterm_color(palette.interaction.cursor_text),
    );
    profile.insert(
        "Selection Color".to_string(),
        iterm_color(palette.interaction.selection_bg),
    );
    profile.insert(
        "Selected Text Color".to_string(),
        iterm_color(palette.interaction.selection_fg),
    );
    profile.insert("Link Color".to_string(), iterm_color(palette.semantic.info));

    for (index, color) in palette.ansi.as_indexed().iter().enumerate() {
        profile.insert(format!("Ansi {index} Color"), iterm_color(*color));
    }

    // Typography
    if let Some(primary) = font_families.first() {
        let size = theme.typography.size_pt.unwrap_or(11.0);
        let effective = if macos_font_scale {
            (size * 1.25 * 2.0).round() / 2.0
        } else {
            size
        };
        let size_fmt = if (effective.fract()).abs() < 0.01 {
            format!("{effective:.0}")
        } else {
            format!("{effective:.1}")
        };
        profile.insert(
            "Normal Font".to_string(),
            Value::String(format!("{primary} {size_fmt}")),
        );
    }
    if let Some(ligatures) = theme.typography.ligatures {
        profile.insert("ASCII Ligatures".to_string(), Value::Bool(ligatures));
    }

    // Transparency & Blur
    let transparency = (1.0 - theme.window_opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    profile.insert("Transparency".to_string(), json!(round4(transparency)));
    let blur = theme
        .atmosphere
        .window
        .as_ref()
        .is_some_and(|window| window.blur);
    profile.insert("Blur".to_string(), Value::Bool(blur));
    if blur {
        profile.insert("Blur Radius".to_string(), json!(16.0));
    }

    // Cursor shape (0 = Underline, 1 = Vertical Bar, 2 = Box)
    let cursor = theme
        .overrides
        .as_ref()
        .and_then(|overrides| overrides.terminal.as_ref())
        .and_then(|terminal| terminal.cursor_shape)
        .unwrap_or(CursorShape::Block);
    let cursor_type = match cursor {
        CursorShape::Underline => 0,
        CursorShape::Bar => 1,
        CursorShape::Block | CursorShape::BlockHollow => 2,
    };
    profile.insert("Cursor Type".to_string(), json!(cursor_type));

    // Wallpaper
    if let Some(wallpaper) = wallpaper.filter(|_| theme.wallpaper_opacity.is_none_or(|o| o > 0.0)) {
        let opacity = theme.wallpaper_opacity.unwrap_or(0.5).clamp(0.0, 1.0);
        profile.insert(
            "Background Image Location".to_string(),
            Value::String(wallpaper.to_string_lossy().into_owned()),
        );
        // iTerm2 `Blend` = 0 shows full image, 1 hides it completely.
        profile.insert("Blend".to_string(), json!(round4((1.0 - opacity).clamp(0.0, 1.0))));
        // 2 = Aspect Fill (cover)
        profile.insert("Background Image Mode".to_string(), json!(2));
    } else {
        profile.insert("Background Image Location".to_string(), Value::String(String::new()));
    }

    let document = json!({
        "Profiles": [Value::Object(profile)]
    });
    serde_json::to_string_pretty(&document).map_err(|error| KisekiError::Json(error.to_string()))
}

fn iterm_color(color: Color) -> Value {
    json!({
        "Color Space": "sRGB",
        "Red Component": round6(f64::from(color.r()) / 255.0),
        "Green Component": round6(f64::from(color.g()) / 255.0),
        "Blue Component": round6(f64::from(color.b()) / 255.0),
        "Alpha Component": 1.0,
    })
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

/// Injects `"Dynamic Profile Parent Name"` into an already-compiled dynamic profile JSON.
pub fn with_parent_name(raw_json: &str, parent_name: &str) -> Result<String> {
    let parent = parent_name.trim();
    if parent.is_empty() || parent == DYNAMIC_PROFILE_NAME {
        return Ok(raw_json.to_string());
    }
    let mut doc: Value =
        serde_json::from_str(raw_json).map_err(|error| KisekiError::Json(error.to_string()))?;
    if let Some(profile) = doc
        .get_mut("Profiles")
        .and_then(Value::as_array_mut)
        .and_then(|arr| arr.first_mut())
        .and_then(Value::as_object_mut)
    {
        profile.insert(
            "Dynamic Profile Parent Name".to_string(),
            Value::String(parent.to_string()),
        );
    }
    serde_json::to_string_pretty(&doc).map_err(|error| KisekiError::Json(error.to_string()))
}

/// Static validation: JSON parses and contains zero non-visual keys.
pub fn lint(content: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let parsed: Value = match serde_json::from_str(content) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                "adapter.iterm2.invalid-json",
                format!("generated iTerm2 dynamic profile is not valid JSON: {error}"),
            ));
            return diagnostics;
        }
    };
    const FORBIDDEN: &[&str] = &[
        "Silence Bell",
        "Unlimited Scrollback",
        "Option Key Sends",
        "Right Option Key Sends",
        "Command",
        "Custom Command",
        "Working Directory",
        "Triggers",
    ];
    if let Some(profiles) = parsed.get("Profiles").and_then(Value::as_array) {
        for profile in profiles {
            if let Some(obj) = profile.as_object() {
                for key in FORBIDDEN {
                    if obj.contains_key(*key) {
                        diagnostics.push(Diagnostic::error(
                            "adapter.iterm2.non-visual-key",
                            format!("iTerm2 dynamic profile must not set `{key}`"),
                        ));
                    }
                }
            }
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_schema::ThemePack;
    use kiseki_types::RenderingProfile;
    use std::path::PathBuf;

    fn theme(profile: RenderingProfile) -> ResolvedTheme {
        let pack = ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json"))
                .unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json"))
                .unwrap(),
        };
        ResolvedTheme::from_pack(&pack, profile)
    }

    #[test]
    fn renders_parent_inheriting_visual_only_profile() {
        let raw = render(
            &theme(RenderingProfile::Balanced),
            &["JetBrainsMono Nerd Font".to_string()],
            Some(&PathBuf::from("/themes/nerv-eva/assets/wallpaper.png")),
            true,
            Some("Default"),
        )
        .unwrap();
        let parsed: Value = serde_json::from_str(&raw).unwrap();
        let profile = &parsed["Profiles"][0];
        assert_eq!(profile["Name"], DYNAMIC_PROFILE_NAME);
        assert_eq!(profile["Guid"], DYNAMIC_PROFILE_GUID);
        assert_eq!(profile["Dynamic Profile Parent Name"], "Default");
        assert!(profile["Ansi 0 Color"].is_object());
        assert!(profile["Ansi 15 Color"].is_object());
        assert!(profile.get("Silence Bell").is_none());
        assert!(profile.get("Unlimited Scrollback").is_none());
        assert!(lint(&raw).is_empty());
    }

    #[test]
    fn lint_catches_non_visual_behaviour_keys() {
        let bad = r#"{"Profiles":[{"Name":"Kiseki Active","Silence Bell":true,"Option Key Sends":2}]}"#;
        let diagnostics = lint(bad);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(Diagnostic::is_fatal));
    }
}
