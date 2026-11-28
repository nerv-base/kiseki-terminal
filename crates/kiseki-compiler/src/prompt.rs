//! Declarative prompt artefacts: Oh My Posh JSON and Starship TOML.
//!
//! ADR-012 is absolute here: the generated prompt config contains **zero**
//! `command` segments and zero `[custom.*]` blocks. A theme can only contribute
//! colours, static labels and glyphs, so switching themes can never execute
//! code on every shell prompt.

use kiseki_schema::{PromptOverrides, PromptPreset, ResolvedTheme};
use kiseki_types::{Color, PromptPolicy};
use serde_json::{json, Value};

use crate::escape::{sanitize_text, toml_string};

/// Resolved colors for the prompt segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptColors {
    pub badge_bg: Color,
    pub badge_fg: Color,
    pub path_bg: Color,
    pub path_fg: Color,
    pub git_bg: Color,
    pub git_fg: Color,
    pub error: Color,
    pub muted: Color,
}

/// Derives prompt colors from L1/L5.
pub fn prompt_colors(theme: &ResolvedTheme) -> PromptColors {
    let segment = theme
        .overrides
        .as_ref()
        .and_then(|overrides| overrides.prompt.as_ref())
        .and_then(|prompt| prompt.segment_colors.as_ref());
    PromptColors {
        badge_bg: segment
            .and_then(|segment| segment.badge_bg)
            .unwrap_or(theme.palette.accents.primary),
        badge_fg: segment
            .and_then(|segment| segment.badge_fg)
            .unwrap_or(theme.palette.text.inverse),
        path_bg: segment
            .and_then(|segment| segment.path_bg)
            .unwrap_or(theme.palette.surfaces.surface0),
        path_fg: segment
            .and_then(|segment| segment.path_fg)
            .unwrap_or(theme.palette.text.primary),
        git_bg: segment
            .and_then(|segment| segment.git_bg)
            .unwrap_or(theme.palette.surfaces.mantle),
        git_fg: segment
            .and_then(|segment| segment.git_fg)
            .unwrap_or(theme.palette.accents.tertiary),
        error: theme.palette.semantic.error,
        muted: theme.palette.text.muted,
    }
}

fn prompt_overrides(theme: &ResolvedTheme) -> Option<&PromptOverrides> {
    theme
        .overrides
        .as_ref()
        .and_then(|overrides| overrides.prompt.as_ref())
}

fn badge_text(theme: &ResolvedTheme) -> String {
    prompt_overrides(theme)
        .and_then(|prompt| prompt.badge_text.clone())
        .map(|value| sanitize_text(&value, 24))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| theme.id.as_str().chars().take(2).collect())
}

fn ok_label(theme: &ResolvedTheme) -> String {
    prompt_overrides(theme)
        .and_then(|prompt| prompt.ok_label.clone())
        .map(|value| sanitize_text(&value, 32))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "ok".to_string())
}

fn error_label(theme: &ResolvedTheme) -> String {
    prompt_overrides(theme)
        .and_then(|prompt| prompt.error_label.clone())
        .map(|value| sanitize_text(&value, 32))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "err".to_string())
}

fn prompt_symbol(theme: &ResolvedTheme) -> String {
    prompt_overrides(theme)
        .and_then(|prompt| prompt.prompt_symbol.clone())
        .map(|value| sanitize_text(&value, 4))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ">".to_string())
}

fn console_title(theme: &ResolvedTheme) -> String {
    // The fallback is the manifest `name`, which is pack-controlled: sanitise it
    // exactly like an authored prefix so Go template syntax (`{{ ... }}`) in the
    // name can never be evaluated by Oh My Posh.
    let prefix = prompt_overrides(theme)
        .and_then(|prompt| prompt.console_title_prefix.clone())
        .or_else(|| Some(theme.name.clone()))
        .map(|value| sanitize_text(&value, 24))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Kiseki".to_string());
    format!("{prefix} // {{{{ .Folder }}}}")
}

/// Builds a complete, safe Oh My Posh v3 configuration.
pub fn omp_config(theme: &ResolvedTheme) -> Value {
    let colors = prompt_colors(theme);
    let p = &theme.palette;
    let preset = prompt_overrides(theme)
        .and_then(|prompt| prompt.preset)
        .unwrap_or(PromptPreset::TacticalBar);
    let badge = badge_text(theme);
    let ok = ok_label(theme);
    let error = error_label(theme);
    let symbol = prompt_symbol(theme);

    let accent = p.accents.primary.to_hex();
    let accent2 = p.accents.secondary.to_hex();
    let tertiary = p.accents.tertiary.to_hex();
    let canvas = p.surfaces.base.to_hex();
    let surface0 = p.surfaces.surface0.to_hex();
    let border = p.surfaces.surface2.to_hex();
    let success = p.semantic.success.to_hex();
    let warning = p.semantic.warning.to_hex();
    let info = p.semantic.info.to_hex();
    let err_hex = colors.error.to_hex();
    let badge_bg = colors.badge_bg.to_hex();
    let badge_fg = colors.badge_fg.to_hex();
    let path_bg = colors.path_bg.to_hex();
    let path_fg = colors.path_fg.to_hex();
    let git_bg = colors.git_bg.to_hex();
    let git_fg = colors.git_fg.to_hex();
    let muted = colors.muted.to_hex();
    let secondary_text = p.text.secondary.to_hex();

    let lang_segment = |kind: &str, label: &str, fg_hex: &str, bg_hex: &str, tmpl: String| {
        json!({
            "type": kind,
            "style": "plain",
            "foreground": fg_hex,
            "background": bg_hex,
            "template": tmpl.replace("{label}", label),
            "properties": {
                "fetch_version": true,
                "display_mode": "files"
            }
        })
    };

    let (left_segments, right_segments, status_template) = match preset {
        PromptPreset::TacticalBar => {
            let left = vec![
                json!({
                    "type": "text",
                    "style": "plain",
                    "foreground": badge_fg,
                    "background": badge_bg,
                    "template": format!("<{badge_bg},transparent>▛</> <b>{badge}</b> <{badge_bg},transparent>▟</>")
                }),
                json!({
                    "type": "path",
                    "style": "plain",
                    "foreground": path_fg,
                    "background": path_bg,
                    "template": format!("<{canvas},transparent> </><{accent}>┃</> <b>{{{{ .Path }}}}</b> "),
                    "properties": {
                        "style": "agnoster_short",
                        "folder_separator_icon": "/",
                        "home_icon": "~",
                        "max_depth": 2
                    }
                }),
                json!({
                    "type": "git",
                    "style": "plain",
                    "foreground": git_fg,
                    "background": git_bg,
                    "template": format!(
                        "<{canvas},transparent> </><{accent2}>┃</> {{{{ .HEAD }}}}{{{{ if .BranchStatus }}}} <{info}>{{{{ .BranchStatus }}}}</>{{{{ end }}}}{{{{ if .Working.Changed }}}} <{warning}>*{{{{ .Working.String }}}}</>{{{{ end }}}}{{{{ if .Staging.Changed }}}} <{success}>+{{{{ .Staging.String }}}}</>{{{{ end }}}} "
                    ),
                    "properties": {
                        "branch_icon": "",
                        "fetch_status": true,
                        "fetch_upstream_icon": false
                    }
                }),
                lang_segment("rust", "rs", &accent, &surface0, format!("<{canvas},transparent> </><{accent}>┃</> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}} ")),
                lang_segment("node", "node", &success, &surface0, format!("<{canvas},transparent> </><{success}>┃</> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}} ")),
                lang_segment("python", "py", &success, &surface0, format!("<{canvas},transparent> </><{success}>┃</> {{label}} {{{{ if .Venv }}}}{{{{ .Venv }}}} {{{{ end }}}}{{{{ .Major }}}}.{{{{ .Minor }}}} ")),
                lang_segment("go", "go", &info, &surface0, format!("<{canvas},transparent> </><{info}>┃</> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}} ")),
            ];
            let right = vec![
                json!({
                    "type": "executiontime",
                    "style": "plain",
                    "foreground": warning,
                    "background": surface0,
                    "template": format!("<{warning}>┃ {{{{ .FormattedMs }}}} </>"),
                    "properties": { "threshold": 1500, "style": "roundrock" }
                }),
                json!({
                    "type": "time",
                    "style": "plain",
                    "foreground": secondary_text,
                    "background": surface0,
                    "template": format!("<{canvas},transparent> </><{accent}>┃</> {{{{ .CurrentDate | date .Format }}}} <{badge_bg},transparent>▜</>"),
                    "properties": { "time_format": "15:04" }
                }),
            ];
            let status = format!(
                "<{border}>┗━</>{{{{ if gt .Code 0 }}}}<{err_hex}>{error} // ERR:{{{{ .Code }}}} {symbol}</>{{{{ else }}}}<{accent2}>{ok}</> <{accent}>{symbol}</>{{{{ end }}}}"
            );
            (left, right, status)
        }
        PromptPreset::ShrineRibbon => {
            let left = vec![
                json!({
                    "type": "path",
                    "style": "plain",
                    "foreground": path_fg,
                    "background": path_bg,
                    "template": format!("<{accent},transparent>「</><{accent}> {badge}</> <b>{{{{ .Path }}}}</b> <{accent},transparent>」</>"),
                    "properties": {
                        "style": "agnoster_short",
                        "folder_separator_icon": "/",
                        "home_icon": "~",
                        "max_depth": 2
                    }
                }),
                json!({
                    "type": "git",
                    "style": "plain",
                    "foreground": git_fg,
                    "background": git_bg,
                    "template": format!(
                        "<{canvas},transparent> </> <{accent2}>·</> {{{{ .HEAD }}}}{{{{ if .BranchStatus }}}} <{success}>{{{{ .BranchStatus }}}}</>{{{{ end }}}}{{{{ if .Working.Changed }}}} <{warning}>*{{{{ .Working.String }}}}</>{{{{ end }}}}{{{{ if .Staging.Changed }}}} <{success}>+{{{{ .Staging.String }}}}</>{{{{ end }}}} "
                    ),
                    "properties": {
                        "branch_icon": "",
                        "fetch_status": true,
                        "fetch_upstream_icon": false
                    }
                }),
                lang_segment("rust", "rs", &warning, &surface0, format!("<{canvas},transparent> </> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}} ")),
                lang_segment("node", "node", &success, &surface0, format!("<{canvas},transparent> </> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}} ")),
                lang_segment("python", "py", &success, &surface0, format!("<{canvas},transparent> </> {{label}} {{{{ if .Venv }}}}{{{{ .Venv }}}} {{{{ end }}}}{{{{ .Major }}}}.{{{{ .Minor }}}} ")),
                lang_segment("go", "go", &info, &surface0, format!("<{canvas},transparent> </> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}} ")),
            ];
            let right = vec![
                json!({
                    "type": "executiontime",
                    "style": "plain",
                    "foreground": warning,
                    "background": surface0,
                    "template": " {{ .FormattedMs }} ",
                    "properties": { "threshold": 1500, "style": "roundrock" }
                }),
                json!({
                    "type": "time",
                    "style": "plain",
                    "foreground": secondary_text,
                    "background": surface0,
                    "template": format!("<{canvas},transparent> </><{accent2},transparent>「</> {{{{ .CurrentDate | date .Format }}}} <{accent2},transparent>」</>"),
                    "properties": { "time_format": "15:04" }
                }),
            ];
            let status = format!(
                "  {{{{ if gt .Code 0 }}}}<{err_hex}>「{error} {{{{ .Code }}}}」 {symbol}</>{{{{ else }}}}<{accent2}>{ok}</> <{accent}>{symbol}</>{{{{ end }}}}"
            );
            (left, right, status)
        }
        PromptPreset::CinematicLine => {
            let left = vec![
                json!({
                    "type": "path",
                    "style": "plain",
                    "foreground": path_fg,
                    "background": "transparent",
                    "template": format!("<{border}>╭─</> <{info}>{badge}</> <b>{{{{ .Path }}}}</b>"),
                    "properties": {
                        "style": "agnoster_short",
                        "folder_separator_icon": "/",
                        "home_icon": "~",
                        "max_depth": 2
                    }
                }),
                json!({
                    "type": "git",
                    "style": "plain",
                    "foreground": git_fg,
                    "background": "transparent",
                    "template": format!(
                        " <{border}>·</> {{{{ .HEAD }}}}{{{{ if .BranchStatus }}}} <{accent}>{{{{ .BranchStatus }}}}</>{{{{ end }}}}{{{{ if .Working.Changed }}}} <{warning}>*{{{{ .Working.String }}}}</>{{{{ end }}}}{{{{ if .Staging.Changed }}}} <{success}>+{{{{ .Staging.String }}}}</>{{{{ end }}}}"
                    ),
                    "properties": {
                        "branch_icon": "",
                        "fetch_status": true,
                        "fetch_upstream_icon": false
                    }
                }),
                lang_segment("rust", "rs", &warning, "transparent", format!(" <{border}>·</> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}}")),
                lang_segment("node", "node", &success, "transparent", format!(" <{border}>·</> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}}")),
                lang_segment("python", "py", &success, "transparent", format!(" <{border}>·</> {{label}} {{{{ if .Venv }}}}{{{{ .Venv }}}} {{{{ end }}}}{{{{ .Major }}}}.{{{{ .Minor }}}}")),
                lang_segment("go", "go", &accent, "transparent", format!(" <{border}>·</> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}}")),
            ];
            let right = vec![
                json!({
                    "type": "executiontime",
                    "style": "plain",
                    "foreground": warning,
                    "background": surface0,
                    "template": " {{ .FormattedMs }} ",
                    "properties": { "threshold": 1500, "style": "roundrock" }
                }),
                json!({
                    "type": "time",
                    "style": "plain",
                    "foreground": secondary_text,
                    "background": surface0,
                    "template": format!("<{canvas},transparent> </> <{accent2}>◦</> {{{{ .CurrentDate | date .Format }}}} <{border},transparent>─╮</>"),
                    "properties": { "time_format": "15:04" }
                }),
            ];
            let status = format!(
                "<{border}>╰─</> {{{{ if gt .Code 0 }}}}<{err_hex}>× {{{{ .Code }}}} {symbol}</>{{{{ else }}}}<{accent}>{symbol}</>{{{{ end }}}}"
            );
            (left, right, status)
        }
        PromptPreset::PixelFrame => {
            let left = vec![
                json!({
                    "type": "text",
                    "style": "plain",
                    "foreground": badge_bg,
                    "background": "transparent",
                    "template": format!("<{border}>┌─</>░▒▓")
                }),
                json!({
                    "type": "path",
                    "style": "plain",
                    "foreground": path_fg,
                    "background": path_bg,
                    "template": format!(" {badge} <b>{{{{ .Path }}}}</b> "),
                    "properties": {
                        "style": "agnoster_short",
                        "folder_separator_icon": "/",
                        "home_icon": "~",
                        "max_depth": 2
                    }
                }),
                json!({
                    "type": "text",
                    "style": "plain",
                    "foreground": badge_bg,
                    "background": "transparent",
                    "template": "▓▒░"
                }),
                json!({
                    "type": "git",
                    "style": "plain",
                    "foreground": git_fg,
                    "background": git_bg,
                    "template": format!(
                        "<{canvas},transparent> </><{accent2}>▌</><{git_fg}> {{{{ .HEAD }}}}</>{{{{ if .BranchStatus }}}} <{accent}>{{{{ .BranchStatus }}}}</>{{{{ end }}}}{{{{ if .Working.Changed }}}} <{warning}>*{{{{ .Working.String }}}}</>{{{{ end }}}}{{{{ if .Staging.Changed }}}} <{success}>+{{{{ .Staging.String }}}}</>{{{{ end }}}} "
                    ),
                    "properties": {
                        "branch_icon": "",
                        "fetch_status": true,
                        "fetch_upstream_icon": false
                    }
                }),
                lang_segment("rust", "rs", &warning, &surface0, format!("<{canvas},transparent> </><{accent2}>▌</> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}} ")),
                lang_segment("node", "node", &success, &surface0, format!("<{canvas},transparent> </><{success}>▌</> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}} ")),
                lang_segment("python", "py", &success, &surface0, format!("<{canvas},transparent> </><{success}>▌</> {{label}} {{{{ if .Venv }}}}{{{{ .Venv }}}} {{{{ end }}}}{{{{ .Major }}}}.{{{{ .Minor }}}} ")),
                lang_segment("go", "go", &accent, &surface0, format!("<{canvas},transparent> </><{accent}>▌</> {{label}} {{{{ .Major }}}}.{{{{ .Minor }}}} ")),
            ];
            let right = vec![
                json!({
                    "type": "executiontime",
                    "style": "plain",
                    "foreground": warning,
                    "background": surface0,
                    "template": format!("<{warning}>▌ {{{{ .FormattedMs }}}} </>"),
                    "properties": { "threshold": 1500, "style": "roundrock" }
                }),
                json!({
                    "type": "time",
                    "style": "plain",
                    "foreground": secondary_text,
                    "background": surface0,
                    "template": format!("<{canvas},transparent> </><{tertiary}>▌</> <{tertiary}>✦</> {{{{ .CurrentDate | date .Format }}}} <{border},transparent>─┐</>"),
                    "properties": { "time_format": "15:04" }
                }),
            ];
            let status = format!(
                "<{border}>└─</> {{{{ if gt .Code 0 }}}}<{err_hex}>{error} {{{{ .Code }}}} {symbol}</>{{{{ else }}}}<{accent2}>{ok}</> <{accent}>{symbol}</>{{{{ end }}}}"
            );
            (left, right, status)
        }
        PromptPreset::Minimal => {
            let left = vec![
                json!({
                    "type": "path",
                    "style": "plain",
                    "foreground": accent,
                    "background": "transparent",
                    "template": "<b>{{ .Path }}</b>",
                    "properties": {
                        "style": "agnoster_short",
                        "folder_separator_icon": "/",
                        "home_icon": "~",
                        "max_depth": 2
                    }
                }),
                json!({
                    "type": "git",
                    "style": "plain",
                    "foreground": git_fg,
                    "background": "transparent",
                    "template": format!(
                        " {{{{ .HEAD }}}}{{{{ if .Working.Changed }}}} <{warning}>*</>{{{{ end }}}}{{{{ if .Staging.Changed }}}} <{success}>+</>{{{{ end }}}}"
                    ),
                    "properties": {
                        "branch_icon": "",
                        "fetch_status": true,
                        "fetch_upstream_icon": false
                    }
                }),
            ];
            let right = vec![json!({
                "type": "time",
                "style": "plain",
                "foreground": muted,
                "background": "transparent",
                "template": "{{ .CurrentDate | date .Format }}",
                "properties": { "time_format": "15:04" }
            })];
            let status = format!(
                "{{{{ if gt .Code 0 }}}}<{err_hex}>{{{{ .Code }}}} {symbol}</>{{{{ else }}}}<{accent}>{symbol}</>{{{{ end }}}}"
            );
            (left, right, status)
        }
    };

    json!({
        "$schema": "https://raw.githubusercontent.com/JanDeDobbeleer/oh-my-posh/main/themes/schema.json",
        "version": 3,
        "final_space": true,
        "console_title_template": console_title(theme),
        "transient_prompt": {
            "foreground": muted,
            "background": "transparent",
            "template": format!("{symbol} ")
        },
        "blocks": [
            {
                "type": "prompt",
                "alignment": "left",
                "newline": true,
                "segments": left_segments
            },
            {
                "type": "prompt",
                "alignment": "right",
                "overflow": "hide",
                "segments": right_segments
            },
            {
                "type": "prompt",
                "alignment": "left",
                "newline": true,
                "segments": [
                    {
                        "type": "status",
                        "style": "plain",
                        "foreground": accent,
                        "background": "transparent",
                        "template": status_template,
                        "properties": {
                            "always_enabled": true
                        }
                    }
                ]
            }
        ],
        "secondary_prompt": {
            "foreground": muted,
            "background": "transparent",
            "template": format!("{symbol} ")
        }
    })
}

/// Palette-only fragment for users who keep their own Oh My Posh layout.
pub fn omp_palette_fragment(theme: &ResolvedTheme) -> Value {
    let colors = prompt_colors(theme);
    json!({
        "palette": {
            "list": {
                "kiseki_primary": theme.palette.accents.primary.to_hex(),
                "kiseki_secondary": theme.palette.accents.secondary.to_hex(),
                "kiseki_tertiary": theme.palette.accents.tertiary.to_hex(),
                "kiseki_surface": theme.palette.surfaces.surface0.to_hex(),
                "kiseki_canvas": theme.palette.surfaces.base.to_hex(),
                "kiseki_text": theme.palette.text.primary.to_hex(),
                "kiseki_muted": colors.muted.to_hex(),
                "kiseki_badge_bg": colors.badge_bg.to_hex(),
                "kiseki_badge_fg": colors.badge_fg.to_hex(),
                "kiseki_error": colors.error.to_hex()
            }
        }
    })
}

/// Renders the Starship palette file.
///
/// `managed` adds style bindings; `false` emits only the palette table so a
/// user's own `starship.toml` can reference it without losing their layout.
pub fn starship_config(theme: &ResolvedTheme, managed: bool) -> String {
    let palette = &theme.palette;
    let colors = prompt_colors(theme);
    let mut out = String::with_capacity(1024);
    out.push_str("# === Auto-generated by Kiseki Terminal (DO NOT EDIT) ===\n");
    out.push_str(&format!("# theme: {} | profile: {}\n\n", theme.id, theme.profile));
    out.push_str("palette = \"kiseki\"\n\n");
    out.push_str("[palettes.kiseki]\n");
    for (name, color) in [
        ("primary", palette.accents.primary),
        ("secondary", palette.accents.secondary),
        ("tertiary", palette.accents.tertiary),
        ("canvas", palette.surfaces.base),
        ("surface", palette.surfaces.surface0),
        ("mantle", palette.surfaces.mantle),
        ("crust", palette.surfaces.crust),
        ("text", palette.text.primary),
        ("muted", palette.text.muted),
        ("success", palette.semantic.success),
        ("warning", palette.semantic.warning),
        ("error", palette.semantic.error),
        ("info", palette.semantic.info),
        ("badge_bg", colors.badge_bg),
        ("badge_fg", colors.badge_fg),
        ("path_bg", colors.path_bg),
        ("path_fg", colors.path_fg),
        ("git_bg", colors.git_bg),
        ("git_fg", colors.git_fg),
    ] {
        out.push_str(&format!("{name} = {}\n", toml_string(&color.to_hex())));
    }

    if managed {
        let symbol = prompt_symbol(theme);
        out.push('\n');
        out.push_str("[character]\n");
        out.push_str(&format!(
            "success_symbol = {}\n",
            toml_string(&format!("[{symbol}](bold primary)"))
        ));
        out.push_str(&format!(
            "error_symbol = {}\n",
            toml_string(&format!("[{symbol}](bold error)"))
        ));
        out.push_str("vimcmd_symbol = \"[<](bold muted)\"\n");
        out.push_str("\n[directory]\n");
        out.push_str("style = \"bold primary\"\n");
        out.push_str("truncation_length = 3\n");
        out.push_str("\n[git_branch]\n");
        out.push_str("style = \"bold secondary\"\n");
        out.push_str("symbol = \"\"\n");
        out.push_str("\n[status]\n");
        out.push_str("disabled = false\n");
        out.push_str("style = \"bold muted\"\n");
        out.push_str(&format!(
            "format = {}\n",
            toml_string(&format!("[{symbol}](bold) "))
        ));
        out.push_str("\n[time]\n");
        out.push_str("disabled = false\n");
        out.push_str("time_format = \"%H:%M\"\n");
        out.push_str("style = \"muted\"\n");
    }

    out
}

/// True when the generated OMP config is free of executable segments.
pub fn omp_is_declarative(value: &Value) -> bool {
    fn walk(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                if map.contains_key("command") {
                    return false;
                }
                if map
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind == "command")
                {
                    return false;
                }
                map.values().all(walk)
            }
            Value::Array(items) => items.iter().all(walk),
            _ => true,
        }
    }
    walk(value)
}

/// Prompt artefacts for a policy. Returns `(path, content)` pairs.
pub fn artifacts(theme: &ResolvedTheme, policy: PromptPolicy) -> Vec<(&'static str, String)> {
    match policy {
        PromptPolicy::Disabled => Vec::new(),
        PromptPolicy::PaletteOnly => vec![
            (
                crate::artifact::names::STARSHIP_PALETTE,
                starship_config(theme, false),
            ),
            (
                crate::artifact::names::PROMPT_OMP_PALETTE,
                serde_json::to_string_pretty(&omp_palette_fragment(theme))
                    .expect("prompt palette fragment is serializable"),
            ),
        ],
        PromptPolicy::ManagedTemplate => vec![
            (
                crate::artifact::names::PROMPT_OMP,
                serde_json::to_string_pretty(&omp_config(theme)).expect("OMP config is serializable"),
            ),
            (
                crate::artifact::names::STARSHIP_PALETTE,
                starship_config(theme, true),
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_types::RenderingProfile;

    fn theme() -> ResolvedTheme {
        let pack = ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
        };
        ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced)
    }

    #[test]
    fn omp_config_is_declarative_and_contains_no_command_segment() {
        let config = omp_config(&theme());
        assert!(omp_is_declarative(&config), "{config}");
        let text = config.to_string();
        assert!(!text.contains("\"command\""));
        assert!(!text.contains("\"type\":\"command\""));
        assert_eq!(config["version"], 3);
        assert_eq!(config["blocks"][0]["type"], "prompt");
        let blocks = config["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1]["alignment"], "right");
        assert_eq!(blocks[1]["overflow"], "hide");
        let segments = blocks[0]["segments"].as_array().unwrap();
        assert_eq!(segments.len(), 7);
        for block in blocks {
            for segment in block["segments"].as_array().unwrap() {
                assert_ne!(segment["type"], "command");
                // Every OMP segment style must be a valid rendering style.
                assert_eq!(segment["style"], "plain");
            }
        }
    }

    #[test]
    fn omp_uses_theme_labels_and_colors() {
        let config = omp_config(&theme());
        let text = config.to_string();
        assert!(text.contains("02"), "{text}");
        assert!(text.contains("[弐号機]"));
        assert!(text.contains("[精神汚染]"));
        assert!(text.contains("#D8262E"), "{text}");
        assert!(text.contains("UNIT-02 // {{ .Folder }}"));
    }

    #[test]
    fn manifest_name_cannot_inject_a_go_template() {
        for name in [
            "{{ .Env.SECRET }}",
            "{{ exec \"rm -rf ~\" }}",
            "A; rm -rf ~",
        ] {
            let mut theme = theme();
            theme.name = name.to_string();
            // Exercise the *fallback* path: no authored console-title prefix.
            if let Some(prompt) = theme
                .overrides
                .as_mut()
                .and_then(|overrides| overrides.prompt.as_mut())
            {
                prompt.console_title_prefix = None;
            }
            let config = omp_config(&theme);
            let title = config["console_title_template"].as_str().unwrap();
            let prefix = title
                .strip_suffix(" // {{ .Folder }}")
                .unwrap_or_else(|| panic!("unexpected title shape {title:?}"));
            // The manifest-supplied part is inert literal text.
            assert!(!prefix.contains('{'), "{title}");
            assert!(!prefix.contains('}'), "{title}");
            assert!(!prefix.contains('$'), "{title}");
            assert!(!prefix.contains(';'), "{title}");
            assert!(!prefix.trim().is_empty(), "{title}");
            assert!(omp_is_declarative(&config));
        }
        // A name that sanitises to nothing falls back to a stable title.
        let mut theme = theme();
        theme.name = "{{}}".to_string();
        if let Some(prompt) = theme
            .overrides
            .as_mut()
            .and_then(|overrides| overrides.prompt.as_mut())
        {
            prompt.console_title_prefix = None;
        }
        assert_eq!(
            omp_config(&theme)["console_title_template"],
            "Kiseki // {{ .Folder }}"
        );
    }

    #[test]
    fn hostile_labels_cannot_break_the_template() {
        let mut theme = theme();
        let overrides = theme.overrides.as_mut().unwrap();
        let prompt = overrides.prompt.get_or_insert_with(Default::default);
        prompt.badge_text = Some("$({ evil })".to_string());
        prompt.ok_label = Some("{{ .Env.SECRET }}".to_string());
        prompt.console_title_prefix = Some("A; rm -rf ~".to_string());
        let config = omp_config(&theme);
        let text = config.to_string();
        // Braces and `$` are stripped, so the payload is inert literal text
        // rather than a Go template expression.
        // The only template expressions left are the ones Kiseki itself
        // authored (`{{ .Path }}`, `{{ .Folder }}`, ...).
        assert!(!text.contains("{{ .Env"));
        assert!(!text.contains("{{ .Env.SECRET }}"));
        assert!(!text.contains("$("));
        // The hostile console-title prefix survives only as inert literal
        // text: `;` and template braces were stripped.
        assert!(!text.contains("A; rm"));
        assert!(text.contains(".Env.SECRET"));
        assert!(omp_is_declarative(&config));
    }

    #[test]
    fn starship_managed_and_palette_only_differ() {
        let managed = starship_config(&theme(), true);
        let palette_only = starship_config(&theme(), false);
        assert!(managed.contains("[palettes.kiseki]"));
        assert!(managed.contains("[character]"));
        assert!(managed.contains("[directory]"));
        assert!(managed.contains("[git_branch]"));
        assert!(!managed.contains("[custom."));
        assert!(palette_only.contains("[palettes.kiseki]"));
        assert!(!palette_only.contains("[custom."));
        assert!(!palette_only.contains("[character]"));
        assert!(palette_only.contains("primary = \"#F06A22\""));
    }

    #[test]
    fn starship_toml_is_parseable() {
        for managed in [true, false] {
            let content = starship_config(&theme(), managed);
            let parsed: toml::Value = toml::from_str(&content).expect("valid TOML");
            assert!(parsed.get("palettes").is_some());
            assert!(parsed.get("custom").is_none());
        }
    }

    #[test]
    fn palette_only_fragment_has_no_blocks() {
        let fragment = omp_palette_fragment(&theme());
        assert!(fragment.get("blocks").is_none());
        assert!(fragment["palette"]["list"]["kiseki_primary"].is_string());
        assert!(omp_is_declarative(&fragment));
    }

    #[test]
    fn every_prompt_preset_is_declarative_and_distinct() {
        for (preset, marker) in [
            (PromptPreset::TacticalBar, "▛"),
            (PromptPreset::ShrineRibbon, "「"),
            (PromptPreset::CinematicLine, "╭─"),
            (PromptPreset::PixelFrame, "░▒▓"),
            (PromptPreset::Minimal, "{{ .Path }}"),
        ] {
            let mut t = theme();
            t.overrides
                .as_mut()
                .unwrap()
                .prompt
                .get_or_insert_with(Default::default)
                .preset = Some(preset);
            let config = omp_config(&t);
            assert!(omp_is_declarative(&config), "{preset:?} must be declarative");
            let text = config.to_string();
            assert!(
                text.contains(marker),
                "{preset:?} expected marker {marker:?} in {text}"
            );
            for block in config["blocks"].as_array().unwrap() {
                for segment in block["segments"].as_array().unwrap() {
                    assert_eq!(segment["style"], "plain");
                }
            }
        }
    }

    #[test]
    fn prompt_preset_snapshots() {
        macro_rules! fixture {
            ($theme:literal, $preset:literal) => {
                (
                    $preset,
                    include_str!(concat!("../../../themes/", $theme, "/manifest.json")),
                    include_str!(concat!("../../../themes/", $theme, "/tokens.json")),
                    include_str!(concat!("../tests/snapshots/", $preset, ".omp.json")),
                )
            };
        }
        for (preset, manifest, tokens, snapshot) in [
            fixture!("nerv-eva", "tactical-bar"),
            fixture!("kyoto-sakura", "shrine-ribbon"),
            fixture!("shinkai-rain", "cinematic-line"),
            fixture!("starlight-8bit", "pixel-frame"),
        ] {
            let pack = ThemePack {
                manifest: serde_json::from_str(manifest).unwrap(),
                tokens: serde_json::from_str(tokens).unwrap(),
            };
            let theme = ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced);
            let (_, output) = artifacts(&theme, PromptPolicy::ManagedTemplate)
                .into_iter()
                .find(|(path, _)| *path == crate::artifact::names::PROMPT_OMP)
                .unwrap();
            // Snapshots are compared verbatim, but a Windows checkout with
            // `core.autocrlf` can hand us CRLF even for files pinned to LF in
            // `.gitattributes`. Normalise before comparing.
            let snapshot = snapshot.replace("\r\n", "\n");
            assert_eq!(output, snapshot.trim_end(), "{preset}");
        }
    }

    #[test]
    fn policy_controls_which_artifacts_exist() {
        let theme = theme();
        assert!(artifacts(&theme, PromptPolicy::Disabled).is_empty());
        let palette_only = artifacts(&theme, PromptPolicy::PaletteOnly);
        assert_eq!(palette_only.len(), 2);
        assert!(palette_only
            .iter()
            .any(|(path, _)| *path == "starship-palette.toml"));
        assert!(palette_only
            .iter()
            .any(|(path, _)| *path == "prompt.omp.palette.json"));

        let managed = artifacts(&theme, PromptPolicy::ManagedTemplate);
        assert_eq!(managed.len(), 2);
        assert!(managed.iter().any(|(path, _)| *path == "prompt.omp.json"));
    }
}
