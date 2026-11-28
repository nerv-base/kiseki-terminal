//! Theme pack scaffolding (`kiseki theme init`).
//!
//! Generates a ready-to-validate starter theme directory with WCAG-compliant
//! dark or light tokens, provenance/license files, an SGR-safe logo, and
//! optional `naga`-clean GLSL/HLSL shader templates.

use std::path::{Path, PathBuf};

use kiseki_types::{Appearance, KisekiError, Result, ThemeId};
use serde_json::json;

/// Options for scaffolding a new theme pack.
#[derive(Debug, Clone)]
pub struct ScaffoldOptions {
    pub id: ThemeId,
    pub name: String,
    pub author: String,
    pub appearance: Appearance,
    pub with_shader: bool,
    pub force: bool,
}

/// Summary returned after creating a scaffolded theme pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldResult {
    pub id: String,
    pub root: PathBuf,
    pub files: Vec<String>,
}

/// Converts a kebab-case theme ID (`tokyo-midnight`) to Title Case (`Tokyo Midnight`).
pub fn title_case_id(id: &str) -> String {
    id.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Creates a new starter theme pack at `target_dir`.
pub fn scaffold_theme_pack(
    target_dir: &Path,
    options: &ScaffoldOptions,
) -> Result<ScaffoldResult> {
    if target_dir.exists() {
        if !target_dir.is_dir() {
            return Err(KisekiError::unsafe_path(
                target_dir.to_string_lossy(),
                "target path exists and is not a directory",
            ));
        }
        let non_empty = std::fs::read_dir(target_dir)
            .map_err(|error| KisekiError::io(target_dir, error))?
            .next()
            .is_some();
        if non_empty && !options.force {
            return Err(KisekiError::other(format!(
                "directory {} is not empty (pass --force to overwrite)",
                target_dir.display()
            )));
        }
    }

    let mut planned: Vec<(&str, String)> = vec![
        ("manifest.json", render_manifest(options)),
        ("tokens.json", render_tokens(options)),
        ("LICENSE", render_license(&options.author)),
        ("CREDITS.md", render_credits(options)),
        ("assets/logo.txt", render_logo(&options.name)),
    ];
    if options.with_shader {
        planned.push(("shaders/live.glsl", STARTER_GLSL.to_string()));
        planned.push(("shaders/live.hlsl", STARTER_HLSL.to_string()));
    }

    let mut written = Vec::with_capacity(planned.len());
    for (relative, contents) in planned {
        let dest = target_dir.join(relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|error| KisekiError::io(parent, error))?;
        }
        std::fs::write(&dest, contents.as_bytes()).map_err(|error| KisekiError::io(&dest, error))?;
        written.push(relative.to_string());
    }

    Ok(ScaffoldResult {
        id: options.id.as_str().to_string(),
        root: target_dir.to_path_buf(),
        files: written,
    })
}

fn render_manifest(options: &ScaffoldOptions) -> String {
    let mut layers = vec!["palette", "syntax", "ui", "typography", "overrides"];
    let mut tags = vec![options.appearance.as_str(), "community"];
    let shader_origin = if options.with_shader {
        layers.insert(3, "atmosphere");
        tags.push("live-shader");
        "original"
    } else {
        "none"
    };
    let manifest = json!({
        "schemaVersion": 1,
        "id": options.id.as_str(),
        "name": options.name,
        "subtitle": format!("{} — Community theme for Kiseki Terminal v2", options.name),
        "version": "0.1.0",
        "appearance": options.appearance.as_str(),
        "pairedTheme": null,
        "tags": tags,
        "authors": [{ "name": options.author }],
        "license": "MIT",
        "creditsFile": "CREDITS.md",
        "layers": layers,
        "minEngineVersion": "2.0.0",
        "provenance": {
            "wallpaperOrigin": "none",
            "shaderOrigin": shader_origin
        }
    });
    serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string()) + "\n"
}

fn render_tokens(options: &ScaffoldOptions) -> String {
    let (palette, syntax, ui, seg_colors) = match options.appearance {
        Appearance::Dark => (
            json!({
                "surfaces": {
                    "crust": "#0A0C12",
                    "mantle": "#10131C",
                    "base": "#131722",
                    "surface0": "#1D2333",
                    "surface1": "#272F45",
                    "surface2": "#3B4664",
                    "overlay0": "#6C7999",
                    "overlay1": "#93A0BF"
                },
                "text": {
                    "primary": "#E9EEF8",
                    "secondary": "#C9D3E6",
                    "muted": "#8B98B5",
                    "inverse": "#131722"
                },
                "accents": {
                    "primary": "#74C7EC",
                    "secondary": "#F5A9B8",
                    "tertiary": "#B4BEFE"
                },
                "semantic": {
                    "error": "#F38BA8",
                    "warning": "#F9E2AF",
                    "success": "#A6E3A1",
                    "info": "#89DCEB",
                    "hint": "#CBA6F7"
                },
                "interaction": {
                    "cursor": "#74C7EC",
                    "cursorText": "#131722",
                    "selectionBg": "#2D3B5E",
                    "selectionFg": "#F2F6FF",
                    "matchBg": "#4D3D24",
                    "matchFg": "#F9E2AF"
                },
                "ansi": {
                    "black": "#1D2333",
                    "red": "#F38BA8",
                    "green": "#A6E3A1",
                    "yellow": "#F9E2AF",
                    "blue": "#89B4FA",
                    "magenta": "#CBA6F7",
                    "cyan": "#89DCEB",
                    "white": "#C9D3E6",
                    "brightBlack": "#7A87A6",
                    "brightRed": "#F7A6BD",
                    "brightGreen": "#BCECB8",
                    "brightYellow": "#FCECC6",
                    "brightBlue": "#A8C9FC",
                    "brightMagenta": "#D9BEF9",
                    "brightCyan": "#A8E7F2",
                    "brightWhite": "#F2F6FF"
                }
            }),
            json!({
                "keyword": "#CBA6F7",
                "operator": "#89DCEB",
                "function": "#89B4FA",
                "method": "#74C7EC",
                "type": "#F9E2AF",
                "variable": "#E9EEF8",
                "parameter": "#F5A9B8",
                "property": "#74C7EC",
                "string": "#A6E3A1",
                "escape": "#F5C2E7",
                "number": "#FAB387",
                "boolean": "#FAB387",
                "constant": "#F5A9B8",
                "comment": "#8B98B5",
                "punctuation": "#93A0BF",
                "tag": "#F38BA8",
                "attribute": "#F9E2AF",
                "macro": "#74C7EC",
                "invalid": "#F38BA8"
            }),
            json!({
                "windowFrame": "#272F45",
                "windowFrameUnfocused": "#1A2030",
                "tabBarBg": "#0A0C12",
                "tabBarUnfocusedBg": "#07090E",
                "tabActiveBg": "#1D2333",
                "tabActiveFg": "#E9EEF8",
                "tabInactiveBg": "#10131C",
                "tabInactiveFg": "#8B98B5",
                "statuslineBg": "#1D2333",
                "statuslineFg": "#E9EEF8",
                "border": "#3B4664",
                "borderFocused": "#74C7EC",
                "panelBg": "#10131C"
            }),
            json!({
                "badgeBg": "#74C7EC",
                "badgeFg": "#131722",
                "pathBg": "#1D2333",
                "pathFg": "#E9EEF8",
                "gitBg": "#272F45",
                "gitFg": "#F5A9B8"
            }),
        ),
        Appearance::Light => (
            json!({
                "surfaces": {
                    "crust": "#DCE2EC",
                    "mantle": "#E6EBF3",
                    "base": "#F5F7FB",
                    "surface0": "#E9EEF6",
                    "surface1": "#DCE3EF",
                    "surface2": "#C4CFE0",
                    "overlay0": "#7B879E",
                    "overlay1": "#5B667D"
                },
                "text": {
                    "primary": "#1A2030",
                    "secondary": "#323D54",
                    "muted": "#535F77",
                    "inverse": "#F5F7FB"
                },
                "accents": {
                    "primary": "#1D5FD1",
                    "secondary": "#B52B65",
                    "tertiary": "#0F7A7A"
                },
                "semantic": {
                    "error": "#C12842",
                    "warning": "#945B00",
                    "success": "#1E7B46",
                    "info": "#1565A8",
                    "hint": "#6B38B8"
                },
                "interaction": {
                    "cursor": "#1D5FD1",
                    "cursorText": "#F5F7FB",
                    "selectionBg": "#BFD3F8",
                    "selectionFg": "#121826",
                    "matchBg": "#F3DB99",
                    "matchFg": "#1A2030"
                },
                "ansi": {
                    "black": "#242C3D",
                    "red": "#B8253E",
                    "green": "#1D7543",
                    "yellow": "#8E5700",
                    "blue": "#1D5FD1",
                    "magenta": "#8B32A8",
                    "cyan": "#0F747D",
                    "white": "#DCE2EC",
                    "brightBlack": "#535F77",
                    "brightRed": "#C92A46",
                    "brightGreen": "#20824B",
                    "brightYellow": "#9C6000",
                    "brightBlue": "#226BE6",
                    "brightMagenta": "#9A3BB8",
                    "brightCyan": "#11828C",
                    "brightWhite": "#F5F7FB"
                }
            }),
            json!({
                "keyword": "#7E2CA3",
                "operator": "#0F747D",
                "function": "#1D5FD1",
                "method": "#1565A8",
                "type": "#8E5700",
                "variable": "#1A2030",
                "parameter": "#B52B65",
                "property": "#0F747D",
                "string": "#1D7543",
                "escape": "#B52B65",
                "number": "#A64B00",
                "boolean": "#A64B00",
                "constant": "#94356F",
                "comment": "#535F77",
                "punctuation": "#4B566E",
                "tag": "#B8253E",
                "attribute": "#8E5700",
                "macro": "#1D5FD1",
                "invalid": "#C12842"
            }),
            json!({
                "windowFrame": "#C4CFE0",
                "windowFrameUnfocused": "#DCE3EF",
                "tabBarBg": "#DCE2EC",
                "tabBarUnfocusedBg": "#E6EBF3",
                "tabActiveBg": "#F5F7FB",
                "tabActiveFg": "#1A2030",
                "tabInactiveBg": "#E6EBF3",
                "tabInactiveFg": "#535F77",
                "statuslineBg": "#E6EBF3",
                "statuslineFg": "#1A2030",
                "border": "#C4CFE0",
                "borderFocused": "#1D5FD1",
                "panelBg": "#E9EEF6"
            }),
            json!({
                "badgeBg": "#1D5FD1",
                "badgeFg": "#F5F7FB",
                "pathBg": "#E6EBF3",
                "pathFg": "#1A2030",
                "gitBg": "#DCE3EF",
                "gitFg": "#B52B65"
            }),
        ),
    };

    let mut root = json!({
        "schemaVersion": 1,
        "palette": palette,
        "syntax": syntax,
        "ui": ui,
        "typography": {
            "style": "modern",
            "families": ["jetbrains-mono-nf", "maple-mono-nf-cn"],
            "sizePt": 11.0,
            "lineHeight": 1.24,
            "weight": "normal",
            "ligatures": true
        },
        "overrides": {
            "terminal": {
                "cursorShape": "block",
                "cursorBlink": false
            },
            "prompt": {
                "preset": "cinematic-line",
                "badgeText": options.id.as_str().chars().take(6).collect::<String>().to_uppercase(),
                "okLabel": format!("[{}]", options.id.as_str()),
                "errorLabel": "[ERR]",
                "promptSymbol": "❯",
                "consoleTitlePrefix": options.name,
                "segmentColors": seg_colors
            },
            "fzf": {
                "border": "rounded",
                "prompt": "❯ ",
                "pointer": "▸",
                "marker": "✓"
            },
            "fastfetch": {
                "logoPath": "assets/logo.txt",
                "headerTitle": options.name,
                "fieldLabels": {
                    "title": "HOST",
                    "os": "OS",
                    "shell": "SHELL",
                    "terminal": "TERM",
                    "cpu": "CPU",
                    "memory": "MEM",
                    "uptime": "UP"
                }
            }
        }
    });

    if options.with_shader {
        root["atmosphere"] = json!({
            "window": {
                "opacity": 0.96,
                "unfocusedOpacity": 0.90,
                "blur": true,
                "blurRadius": 16,
                "useMica": false,
                "padding": { "x": 14, "y": 12 }
            },
            "shader": {
                "glsl": "shaders/live.glsl",
                "hlsl": "shaders/live.hlsl",
                "defaultProfile": "balanced",
                "params": {
                    "pulseSpeed": 0.8,
                    "glowStrength": 0.08,
                    "narrowFadeAspect": 1.15
                }
            }
        });
    }

    serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string()) + "\n"
}

fn render_license(author: &str) -> String {
    format!(
        "MIT License\n\nCopyright (c) 2026 {author}\n\n\
Permission is hereby granted, free of charge, to any person obtaining a copy\n\
of this software and associated documentation files (the \"Software\"), to deal\n\
in the Software without restriction, including without limitation the rights\n\
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell\n\
copies of the Software, and to permit persons to whom the Software is\n\
furnished to do so, subject to the following conditions:\n\n\
The above copyright notice and this permission notice shall be included in all\n\
copies or substantial portions of the Software.\n\n\
THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\n\
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,\n\
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.\n"
    )
}

fn render_credits(options: &ScaffoldOptions) -> String {
    format!(
        "# Credits & Asset Provenance — {}\n\n\
- **Theme ID**: `{}`\n\
- **Author**: {}\n\
- **License**: MIT\n\
- **Wallpaper**: None (replace with original / CC0 / CC-BY / licensed artwork and document source + license here)\n\
- **Shaders**: {}\n\
- **Fonts**: Referenced by Font Catalog ID (`jetbrains-mono-nf`, `maple-mono-nf-cn`); never bundle third-party font binaries inside a theme pack.\n",
        options.name,
        options.id,
        options.author,
        if options.with_shader {
            "Original starter GLSL/HLSL templates"
        } else {
            "None"
        }
    )
}

fn render_logo(name: &str) -> String {
    let label: String = name
        .chars()
        .filter(|ch| !ch.is_control())
        .take(20)
        .collect();
    format!(
        "  ╔════════════════════════╗\n\
  ║  ★  KISEKI TERMINAL    ║\n\
  ║  {label:<20}  ║\n\
  ╚════════════════════════╝\n"
    )
}

const STARTER_GLSL: &str = r#"// Starter Kiseki v2 GLSL shader template (Ghostty postprocess).
// kiseki-shader automatically injects KISEKI_BASE_BG, KISEKI_ACCENT_PRIMARY,
// KISEKI_PARAM_*, kiseki_bg_mask() and kiseki_narrow_factor().

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 res = max(iResolution.xy, vec2(1.0));
    vec2 uv = fragCoord / res;

    vec4 term = texture(iChannel0, uv);
    vec3 col = term.rgb / max(term.a, 0.001);

    float bgMask = kiseki_bg_mask(term, uv);
    float narrow = kiseki_narrow_factor(res);

#if KISEKI_QUALITY_LEVEL >= 1
    float wave = 0.5 + 0.5 * sin(iTime * KISEKI_PARAM_PULSE_SPEED + uv.x * 3.14159);
    float corner = smoothstep(0.45, 1.0, uv.x) * smoothstep(0.35, 1.0, uv.y);
    col += KISEKI_ACCENT_PRIMARY * (corner * wave * KISEKI_PARAM_GLOW_STRENGTH) * bgMask * narrow;
#endif

    fragColor = vec4(clamp(col, 0.0, 1.0) * term.a, term.a);
}
"#;

const STARTER_HLSL: &str = r#"// Starter Kiseki v2 HLSL shader template (Windows Terminal layered bg).
// t0 `shaderTexture` — text layer (premultiplied alpha, empty = 0)
// t1 `imageTexture`  — wallpaper layer (sampled independently)
Texture2D shaderTexture : register(t0);
Texture2D imageTexture : register(t1);
SamplerState samplerState : register(s0);

cbuffer PixelShaderSettings : register(b0) {
    float Time;
    float Scale;
    float2 Resolution;
    float4 Background;
};

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float4 term = shaderTexture.Sample(samplerState, tex);
    float4 wall = imageTexture.Sample(samplerState, tex);
    float3 animBg = lerp(KISEKI_BASE_BG, wall.rgb, wall.a * KISEKI_BG_OPACITY);

#if KISEKI_QUALITY_LEVEL >= 1
    float narrow = kiseki_narrow_factor(Resolution);
    float wave = 0.5 + 0.5 * sin(Time * KISEKI_PARAM_PULSE_SPEED + tex.x * 3.14159);
    float corner = smoothstep(0.45, 1.0, tex.x) * smoothstep(0.35, 1.0, tex.y);
    animBg += KISEKI_ACCENT_PRIMARY * (corner * wave * KISEKI_PARAM_GLOW_STRENGTH) * narrow;
#endif

    // Porter-Duff Source-Over so text pixels stay crisp.
    float4 bgPremul = float4(saturate(animBg) * KISEKI_WINDOW_ALPHA, KISEKI_WINDOW_ALPHA);
    float4 outColor;
    outColor.rgb = term.rgb + bgPremul.rgb * (1.0 - term.a);
    outColor.a = term.a + bgPremul.a * (1.0 - term.a);
    return saturate(outColor);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate_theme_dir;

    #[test]
    fn scaffolded_packs_pass_full_validation_in_all_modes() {
        for (id, appearance, with_shader) in [
            ("starter-dark", Appearance::Dark, false),
            ("starter-light", Appearance::Light, false),
            ("starter-dark-live", Appearance::Dark, true),
            ("starter-light-live", Appearance::Light, true),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join(id);
            let options = ScaffoldOptions {
                id: ThemeId::new(id).unwrap(),
                name: title_case_id(id),
                author: "Community Author".to_string(),
                appearance,
                with_shader,
                force: false,
            };
            let result = scaffold_theme_pack(&target, &options).unwrap();
            assert_eq!(result.id, id);

            let validation = validate_theme_dir(&target).unwrap();
            let fatals: Vec<String> = validation
                .diagnostics
                .iter()
                .filter(|d| d.is_fatal())
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect();
            assert!(
                validation.is_valid(),
                "scaffold ({id}) failed validation: {fatals:#?}"
            );
        }
    }

    #[test]
    fn refuses_non_empty_directory_without_force() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("existing.txt"), b"keep").unwrap();
        let options = ScaffoldOptions {
            id: ThemeId::new("sample-pack").unwrap(),
            name: "Sample Pack".to_string(),
            author: "Tester".to_string(),
            appearance: Appearance::Dark,
            with_shader: false,
            force: false,
        };
        assert!(scaffold_theme_pack(dir.path(), &options).is_err());
    }
}
