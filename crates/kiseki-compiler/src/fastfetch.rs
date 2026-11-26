//! Fastfetch artefact generation (RFC-0002 §3.5, RFC-0003 §4).
//!
//! Two hard requirements:
//! 1. the `logo.source` is a resolved absolute path (v1 hard-coded
//!    `~/.config/kiseki-terminal`, which broke every non-default install);
//! 2. no `command` module may ever appear, because fastfetch executes them.

use std::path::Path;

use kiseki_schema::{FastfetchFieldLabels, ResolvedTheme};
use kiseki_types::Color;
use serde_json::{json, Map, Value};

use crate::escape::sanitize_text;

fn fg(color: Color) -> String {
    let (r, g, b) = color.rgb();
    format!("\x1b[38;2;{r};{g};{b}m")
}

fn fg_bold(color: Color) -> String {
    let (r, g, b) = color.rgb();
    format!("\x1b[1;38;2;{r};{g};{b}m")
}

/// Renders the JSONC file, header comment included.
pub fn render(theme: &ResolvedTheme, logo: Option<&Path>) -> String {
    let p = &theme.palette;
    let overrides = theme
        .overrides
        .as_ref()
        .and_then(|overrides| overrides.fastfetch.as_ref());
    let labels = overrides.and_then(|fastfetch| fastfetch.field_labels.as_ref());
    let header_title = overrides
        .and_then(|fastfetch| fastfetch.header_title.clone())
        .map(|title| sanitize_text(&title, 48))
        .filter(|title| !title.is_empty());

    let bar = fg(p.accents.secondary);
    let border = fg(p.surfaces.surface2);
    let primary = fg(p.accents.primary);
    let bold_text = fg_bold(p.text.primary);
    let body_text = fg(p.text.secondary);
    let reset = "\x1b[0m";

    let mut modules = Vec::new();
    if let Some(title) = header_title.as_ref() {
        // Fastfetch has no top-level `title` key; a header is a declarative
        // `custom` module prepended to the list.
        modules.push(json!({
            "type": "custom",
            "format": format!("{bar}▛─ {bold_text}{title} {border}─────────{reset}")
        }));
    }

    let rows: [(&str, Option<&str>, &str, Color, String); 7] = [
        (
            "title",
            labels.and_then(|l| l.title.as_deref()),
            "USER",
            p.accents.primary,
            format!("{bold_text}{{user-name}}{bar}@{primary}{{host-name}}{reset}"),
        ),
        (
            "os",
            labels.and_then(|l| l.os.as_deref()),
            "OS",
            p.semantic.warning,
            format!("{body_text}{{pretty-name}}{reset}"),
        ),
        (
            "shell",
            labels.and_then(|l| l.shell.as_deref()),
            "SHELL",
            p.semantic.success,
            format!("{body_text}{{pretty-name}} {{version}}{reset}"),
        ),
        (
            "terminal",
            labels.and_then(|l| l.terminal.as_deref()),
            "TERM",
            p.accents.primary,
            format!("{body_text}{{pretty-name}}{reset}"),
        ),
        (
            "cpu",
            labels.and_then(|l| l.cpu.as_deref()),
            "CPU",
            p.semantic.hint,
            format!("{body_text}{{name}} ({{cores-logical}}T){reset}"),
        ),
        (
            "memory",
            labels.and_then(|l| l.memory.as_deref()),
            "MEM",
            p.semantic.error,
            format!("{body_text}{{used}} / {{total}} ({{percentage}}){reset}"),
        ),
        (
            "uptime",
            labels.and_then(|l| l.uptime.as_deref()),
            "UPTIME",
            p.semantic.success,
            format!("{body_text}{{days}}d {{hours}}h {{minutes}}m{reset}"),
        ),
    ];

    for (kind, custom_label, fallback_label, key_color, format_str) in rows {
        let label = custom_label
            .map(|raw| sanitize_text(raw, 24))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback_label.to_string());
        let padded = format!("{label:<6}");
        modules.push(json!({
            "type": kind,
            "key": format!("{bar}┃ {}{padded}{reset}", fg(key_color)),
            "format": format_str,
        }));
    }

    // Bottom palette swatch bar built directly from the theme's L1 palette.
    let swatch_colors = [
        p.ansi.red,
        p.accents.secondary,
        p.accents.primary,
        p.ansi.cyan,
        p.semantic.warning,
        p.semantic.success,
        p.text.primary,
    ];
    let mut swatch_bar = format!("{bar}▙─ ");
    for color in swatch_colors {
        swatch_bar.push_str(&fg(color));
        swatch_bar.push_str("██");
    }
    swatch_bar.push_str(&format!(" {border}─────────────────────{reset}"));
    modules.push(json!({
        "type": "custom",
        "format": swatch_bar,
    }));

    let mut root = Map::new();
    root.insert(
        "$schema".to_string(),
        json!("https://github.com/fastfetch-cli/fastfetch/raw/dev/doc/json_schema.json"),
    );
    if let Some(logo) = logo {
        let mut logo_object = Map::new();
        logo_object.insert(
            "source".to_string(),
            json!(logo.to_string_lossy().to_string()),
        );
        // `file-raw` preserves TrueColor SGR escapes authored in `logo.txt`.
        logo_object.insert("type".to_string(), json!("file-raw"));
        logo_object.insert(
            "padding".to_string(),
            json!({ "top": 0, "left": 1, "right": 3 }),
        );
        root.insert("logo".to_string(), Value::Object(logo_object));
    }
    root.insert("display".to_string(), json!({ "separator": "  " }));
    root.insert("modules".to_string(), Value::Array(modules));

    let mut out = String::from(
        "// === Auto-generated by Kiseki Terminal (DO NOT EDIT) ===\n\
         // Fastfetch executes no commands from this file: every module is static.\n",
    );
    out.push_str(
        &serde_json::to_string_pretty(&Value::Object(root))
            .expect("fastfetch config is serializable"),
    );
    out.push('\n');
    out
}

/// True when a rendered config contains no executable module.
pub fn is_declarative(value: &Value) -> bool {
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

/// Strips the JSONC header so callers can parse the body.
pub fn parse_body(raw: &str) -> Option<Value> {
    let body: String = raw
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(&body).ok()
}

/// Labels a theme may rename, for diagnostics.
pub fn renamed_labels(labels: &FastfetchFieldLabels) -> usize {
    [
        &labels.title,
        &labels.os,
        &labels.shell,
        &labels.terminal,
        &labels.cpu,
        &labels.memory,
        &labels.uptime,
    ]
    .into_iter()
    .flatten()
    .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_types::RenderingProfile;
    use std::path::PathBuf;

    fn theme() -> ResolvedTheme {
        let pack = ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
        };
        ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced)
    }

    #[test]
    fn logo_path_is_absolute_and_has_no_command_modules() {
        let logo = PathBuf::from("/data/themes/official/nerv-eva/assets/logo.txt");
        let raw = render(&theme(), Some(&logo));
        assert!(raw.starts_with("// === Auto-generated"));
        let parsed = parse_body(&raw).unwrap();
        assert!(is_declarative(&parsed));
        assert_eq!(parsed["logo"]["source"], logo.to_string_lossy().as_ref());
        assert_eq!(parsed["logo"]["type"], "file-raw");
        // 7 static modules + header + bottom palette swatch bar = 9.
        assert_eq!(parsed.get("modules").unwrap().as_array().unwrap().len(), 9);
        assert!(!parsed.to_string().contains("command"));
        assert!(parsed.get("title").is_none(), "fastfetch has no top-level title key");
        assert_eq!(parsed["modules"][0]["type"], "custom");
        assert!(parsed["modules"][0]["format"]
            .as_str()
            .unwrap()
            .contains("EVANGELION UNIT-02 // 弐号機"));
    }

    #[test]
    fn field_labels_rename_modules() {
        let raw = render(&theme(), None);
        let parsed = parse_body(&raw).unwrap();
        let modules = parsed["modules"].as_array().unwrap();
        let labels: Vec<&str> = modules
            .iter()
            .filter_map(|module| module.get("key").and_then(Value::as_str))
            .collect();
        assert!(labels.iter().any(|l| l.contains("PILOT")));
        assert!(labels.iter().any(|l| l.contains("MAGI")));
        assert!(labels.iter().any(|l| l.contains("LCL")));
        assert!(labels.iter().any(|l| l.contains("POWER")));
        // The header is prepended as a declarative custom module, not a key.
        let header = &modules[0];
        assert_eq!(header["type"], "custom");
        assert!(header["format"]
            .as_str()
            .unwrap()
            .contains("EVANGELION UNIT-02 // 弐号機"));
        assert!(header.get("command").is_none());
    }

    #[test]
    fn no_logo_omits_the_logo_object() {
        let parsed = parse_body(&render(&theme(), None)).unwrap();
        assert!(parsed.get("logo").is_none());
        assert!(is_declarative(&parsed));
    }

    #[test]
    fn hostile_labels_are_sanitized() {
        let mut theme = theme();
        let overrides = theme.overrides.as_mut().unwrap();
        let fastfetch = overrides.fastfetch.get_or_insert_with(Default::default);
        fastfetch.header_title = Some("A\n$(evil)".to_string());
        fastfetch.field_labels = Some(FastfetchFieldLabels {
            cpu: Some("CPU; rm -rf /".to_string()),
            ..Default::default()
        });
        let parsed = parse_body(&render(&theme, None)).unwrap();
        let text = parsed.to_string();
        // Labels are inert text: no control characters, no command
        // substitution and no hostile semicolons survive (only SGR escapes
        // generated by Kiseki itself contain `;`).
        assert!(!text.contains("$("));
        assert!(!text.contains("CPU;"));
        assert!(!text.contains("\\n"));
        assert!(is_declarative(&parsed));
    }

    #[test]
    fn renamed_label_count_is_reported() {
        let theme = theme();
        let labels = theme
            .overrides
            .as_ref()
            .and_then(|overrides| overrides.fastfetch.as_ref())
            .and_then(|fastfetch| fastfetch.field_labels.as_ref())
            .unwrap();
        assert_eq!(renamed_labels(labels), 7);
    }
}
