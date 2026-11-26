//! Zero-code-execution security lint (RFC-0004 §4).
//!
//! Every check here is a pure function over already-parsed data, so a hostile
//! theme pack can be rejected without ever touching the host filesystem.

use std::path::{Component, Path};

use kiseki_types::{Diagnostic, DiagnosticSeverity};

/// File extensions a theme pack may ship (RFC-0004 §4.2).
pub const ALLOWED_ASSET_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
/// File extensions a theme pack may *never* ship.
pub const FORBIDDEN_EXTENSIONS: &[&str] = &[
    "lua", "sh", "zsh", "bash", "fish", "ps1", "psm1", "bat", "cmd", "vim", "py", "rb", "js", "ts",
    "exe", "dll", "dylib", "so", "wasm", "bin", "ttf", "otf", "woff", "woff2",
];

/// Characters that would let a label break out of its config value.
const INJECTION_CHARS: &[char] = &[
    '\n', '\r', '\t', '\u{0}', '`', '$', ';', '|', '&', '<', '>', '\\', '"', '\'',
];

/// Validates a pack-relative path: no absolute roots, no `..`, no backslashes,
/// no control characters, and only `[A-Za-z0-9._/-]`.
pub fn validate_relative_path(raw: &str) -> Result<(), String> {
    if raw.is_empty() {
        return Err("path must not be empty".to_string());
    }
    if raw.len() > 160 {
        return Err(format!("path exceeds 160 bytes ({} bytes)", raw.len()));
    }
    if raw.starts_with('/') {
        return Err("path must be relative, not absolute".to_string());
    }
    if raw.contains('\\') {
        return Err("path must use '/' separators, not '\\'".to_string());
    }
    if raw.split('/').any(|segment| segment == "..") {
        return Err("path must not contain '..' segments".to_string());
    }
    if raw.split('/').any(|segment| segment == ".") {
        return Err("path must not contain '.' segments".to_string());
    }
    if raw.contains("//") {
        return Err("path must not contain empty segments".to_string());
    }
    for ch in raw.chars() {
        let allowed = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/');
        if !allowed {
            return Err(format!("path contains disallowed character {ch:?}"));
        }
    }
    // Defence in depth: the structural check above is authoritative, but a
    // normalized path must also stay inside the pack root.
    let path = Path::new(raw);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return Err("path escapes the theme pack root".to_string());
    }
    Ok(())
}

/// Validates a path whose extension must be in `allowed`.
pub fn validate_extension(raw: &str, allowed: &[&str]) -> Result<(), String> {
    let extension = raw
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .ok_or_else(|| format!("{raw:?} has no file extension"))?;
    if FORBIDDEN_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!(
            "extension '.{extension}' is executable/script content and is never allowed in a theme pack"
        ));
    }
    if !allowed.contains(&extension.as_str()) {
        return Err(format!(
            "extension '.{extension}' is not allowed here (expected one of {})",
            allowed.join(", ")
        ));
    }
    Ok(())
}

/// Rejects label/glyph strings that could inject configuration.
///
/// Applies to every user-visible string that ends up in a shell, TOML, JSON or
/// key-value config file.
pub fn validate_safe_label(raw: &str, field: &str, max_len: usize) -> Result<(), String> {
    let len = raw.chars().count();
    if len == 0 {
        return Err(format!("{field} must not be empty"));
    }
    if len > max_len {
        return Err(format!("{field} exceeds {max_len} characters ({len})"));
    }
    if let Some(bad) = raw.chars().find(|ch| INJECTION_CHARS.contains(ch) || ch.is_control()) {
        return Err(format!("{field} contains forbidden character {bad:?}"));
    }
    Ok(())
}

/// Glyph fields additionally reject quotes so they stay safe inside prompts.
pub fn validate_safe_glyph(raw: &str, field: &str) -> Result<(), String> {
    validate_safe_label(raw, field, 12)
}

/// Rejects `.lua`/`.sh`/... anywhere in a pack file list.
pub fn lint_pack_file_listing(files: &[String]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for file in files {
        let lower = file.to_ascii_lowercase();
        let extension = lower.rsplit_once('.').map(|(_, ext)| ext.to_string());
        if let Some(extension) = extension {
            if FORBIDDEN_EXTENSIONS.contains(&extension.as_str()) {
                diagnostics.push(Diagnostic::new(
                    DiagnosticSeverity::Error,
                    "security.forbidden-extension",
                    format!("theme pack must not ship '{file}': '.{extension}' files can execute code"),
                ));
                continue;
            }
        }
        if let Err(reason) = validate_relative_path(file) {
            diagnostics.push(Diagnostic::new(
                DiagnosticSeverity::Error,
                "security.unsafe-path",
                format!("theme pack entry '{file}' is unsafe: {reason}"),
            ));
        }
    }
    diagnostics
}

/// Result of auditing a `logo.txt` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoAudit {
    /// Number of SGR (`ESC [ ... m`) sequences found.
    pub sgr_count: usize,
}

/// Validates `assets/logo.txt`: UTF-8, bounded size, and *only* SGR color
/// escapes. OSC (title/clipboard), DCS and any non-`m` CSI sequence are
/// rejected because they can drive the terminal, not just color it.
///
/// Printable text, `\n`, `\r` and `\t` are allowed. Every other control
/// character — including BEL, BS, NUL, DEL and the C1 range
/// (`U+0080..=U+009F`) — is rejected, and an escape sequence must be exactly
/// `ESC [ <digits and semicolons> m`.
pub fn audit_logo_text(content: &str, max_bytes: usize) -> Result<LogoAudit, String> {
    if content.len() > max_bytes {
        return Err(format!(
            "logo.txt is {} bytes, exceeding the {max_bytes} byte limit",
            content.len()
        ));
    }
    let bytes = content.as_bytes();
    let mut index = 0;
    let mut sgr_count = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte != 0x1B {
            if (0x20..0x7F).contains(&byte) || matches!(byte, b'\n' | b'\r' | b'\t') {
                index += 1;
                continue;
            }
            // Decode one full character: continuation bytes of a multi-byte
            // sequence land here, and C0/C1 controls are rejected explicitly.
            let ch = content[index..]
                .chars()
                .next()
                .ok_or_else(|| "logo.txt is not valid UTF-8".to_string())?;
            if ch.is_control() || (0x80..=0x9F).contains(&(ch as u32)) {
                return Err(format!(
                    "logo.txt contains disallowed control character U+{:04X}",
                    ch as u32
                ));
            }
            index += ch.len_utf8();
            continue;
        }
        // Any escape sequence must be a CSI SGR: ESC '[' params 'm'.
        if index + 1 >= bytes.len() {
            return Err("logo.txt ends with a truncated escape sequence".to_string());
        }
        match bytes[index + 1] {
            b'[' => {}
            b']' => {
                return Err(
                    "logo.txt contains an OSC sequence (ESC ]): terminal title, clipboard and hyperlink control are forbidden"
                        .to_string(),
                )
            }
            b'P' | b'X' | b'^' | b'_' => {
                return Err(
                    "logo.txt contains a DCS/SOS/PM/APC sequence: only SGR color escapes are allowed"
                        .to_string(),
                )
            }
            b'(' | b')' | b'*' | b'+' => {
                return Err("logo.txt contains a character-set designation escape".to_string())
            }
            other => {
                return Err(format!(
                    "logo.txt contains a non-SGR escape sequence (ESC {})",
                    char::from(other)
                ))
            }
        }
        // Walk the CSI parameter bytes up to the final byte. Only decimal
        // digits and `;` are valid SGR parameters, so private markers (`?`,
        // `>`, `=`), intermediates and every other final byte are rejected.
        let mut cursor = index + 2;
        while cursor < bytes.len() && (bytes[cursor].is_ascii_digit() || bytes[cursor] == b';') {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return Err("logo.txt ends with an unterminated CSI sequence".to_string());
        }
        let final_byte = bytes[cursor];
        if final_byte != b'm' {
            return Err(format!(
                "logo.txt contains CSI {}: only SGR (m) sequences are allowed",
                char::from(final_byte)
            ));
        }
        sgr_count += 1;
        index = cursor + 1;
    }
    Ok(LogoAudit { sgr_count })
}

/// Audits a text file destined for a generated config for injected control
/// characters (belt-and-braces next to per-format escaping).
pub fn lint_control_characters(content: &str, allow_ansi: bool) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (line_index, line) in content.lines().enumerate() {
        let offending = line.chars().find(|ch| {
            if allow_ansi && *ch == '\u{1b}' {
                return false;
            }
            ch.is_control() && *ch != '\t'
        });
        if let Some(ch) = offending {
            diagnostics.push(Diagnostic::new(
                DiagnosticSeverity::Error,
                "security.control-character",
                format!("line {} contains control character U+{:04X}", line_index + 1, ch as u32),
            ));
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_safe_relative_paths() {
        for path in [
            "assets/wallpaper.png",
            "shaders/live.glsl",
            "CREDITS.md",
            "tokens.json",
            "a/b/c/d.txt",
            "file-name_with.underscore.webp",
        ] {
            assert!(validate_relative_path(path).is_ok(), "{path} should pass");
        }
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        for path in [
            "../etc/passwd",
            "assets/../../etc/passwd",
            "/etc/passwd",
            "assets\\wallpaper.png",
            "assets//wallpaper.png",
            "./assets/wallpaper.png",
            "assets/wall paper.png",
            "assets/$(whoami).png",
            "assets/wallpaper.png\n",
            "",
        ] {
            assert!(validate_relative_path(path).is_err(), "{path:?} should be rejected");
        }
    }

    #[test]
    fn rejects_overlong_paths() {
        let long = format!("assets/{}.png", "a".repeat(200));
        assert!(validate_relative_path(&long).is_err());
    }

    #[test]
    fn extension_whitelist_blocks_scripts() {
        assert!(validate_extension("assets/wallpaper.png", ALLOWED_ASSET_EXTENSIONS).is_ok());
        for script in [
            "overrides/wezterm.lua",
            "hooks/init.sh",
            "hooks/init.zsh",
            "hooks/profile.ps1",
            "scripts/run.py",
            "plugin/evil.vim",
            "bin/tool.exe",
            "lib/libfoo.so",
            "fonts/bad.ttf",
        ] {
            let error = validate_extension(script, ALLOWED_ASSET_EXTENSIONS).unwrap_err();
            assert!(
                error.contains("never allowed") || error.contains("not allowed"),
                "{script}: {error}"
            );
        }
        assert!(validate_extension("assets/noext", ALLOWED_ASSET_EXTENSIONS).is_err());
    }

    #[test]
    fn labels_reject_injection_payloads() {
        assert!(validate_safe_label("UNIT-02", "badgeText", 48).is_ok());
        assert!(validate_safe_label("弐号機", "badgeText", 48).is_ok());
        for payload in [
            "02\ncommand = evil",
            "02\rcommand",
            "02\tcommand",
            "02;rm -rf /",
            "02|sh",
            "02&whoami",
            "02`id`",
            "02$(id)",
            "02>out",
            "02<in",
            "02\\n",
            "02\u{0}",
            "02\u{7}",
        ] {
            assert!(
                validate_safe_label(payload, "badgeText", 48).is_err(),
                "{payload:?} must be rejected"
            );
        }
    }

    #[test]
    fn labels_respect_length_limits() {
        assert!(validate_safe_label(&"a".repeat(48), "badgeText", 48).is_ok());
        assert!(validate_safe_label(&"a".repeat(49), "badgeText", 48).is_err());
        assert!(validate_safe_glyph(&"a".repeat(13), "prompt").is_err());
        assert!(validate_safe_glyph("", "prompt").is_err());
    }

    #[test]
    fn pack_listing_flags_scripts_and_traversal() {
        let files = vec![
            "manifest.json".to_string(),
            "overrides/wezterm.lua".to_string(),
            "../evil.json".to_string(),
            "assets/logo.txt".to_string(),
        ];
        let diagnostics = lint_pack_file_listing(&files);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(Diagnostic::is_fatal));
        assert_eq!(diagnostics[0].code, "security.forbidden-extension");
        assert_eq!(diagnostics[1].code, "security.unsafe-path");
    }

    #[test]
    fn logo_allows_sgr_only() {
        let ok = "\u{1b}[0m\u{1b}[38;2;216;38;46m█\u{1b}[0m\n";
        let audit = audit_logo_text(ok, 32 * 1024).unwrap();
        assert_eq!(audit.sgr_count, 3);
    }

    #[test]
    fn logo_rejects_osc_dcs_and_non_sgr_csi() {
        let cases = [
            ("\u{1b}]0;pwned\u{7}", "OSC"),
            ("\u{1b}]52;c;base64\u{7}", "OSC"),
            ("\u{1b}Pq#0;2;0;0;0\u{1b}\\", "DCS"),
            ("\u{1b}[2J", "CSI"),
            ("\u{1b}[H", "CSI"),
            ("\u{1b}[?25l", "CSI"),
            ("\u{1b}(B", "character-set"),
            ("\u{1b}", "truncated"),
            ("\u{1b}[38;2;1", "unterminated"),
        ];
        for (payload, needle) in cases {
            let error = audit_logo_text(payload, 32 * 1024).unwrap_err();
            assert!(
                error.to_lowercase().contains(&needle.to_lowercase()),
                "{payload:?} -> {error}"
            );
        }
    }

    #[test]
    fn logo_enforces_size_cap() {
        let big = "a".repeat(100);
        assert!(audit_logo_text(&big, 50).is_err());
    }

    #[test]
    fn logo_rejects_every_disallowed_control_character() {
        for (payload, needle) in [
            ("\u{7}", "U+0007"),
            ("\u{8}", "U+0008"),
            ("\u{0}", "U+0000"),
            ("\u{7F}", "U+007F"),
            ("\u{80}", "U+0080"),
            ("\u{9B}", "U+009B"),
            ("\u{9F}", "U+009F"),
            ("\u{1b}[0m\u{7}", "U+0007"),
        ] {
            let error = audit_logo_text(payload, 32 * 1024).unwrap_err();
            assert!(error.contains(needle), "{payload:?} -> {error}");
        }
    }

    #[test]
    fn logo_allows_printable_text_and_layout_characters() {
        assert_eq!(audit_logo_text("▐  UNIT-02  ▐\n\t\r\n", 1024).unwrap().sgr_count, 0);
        // Non-ASCII printable text is fine.
        assert!(audit_logo_text("弐号機\n", 1024).is_ok());
    }

    #[test]
    fn logo_rejects_private_csi_parameters() {
        for payload in ["\u{1b}[?0m", "\u{1b}[>0m", "\u{1b}[=0m", "\u{1b}[1 m"] {
            assert!(audit_logo_text(payload, 1024).is_err(), "{payload:?}");
        }
    }

    #[test]
    fn control_character_lint_reports_line_numbers() {
        let content = "ok\nbad\u{7}line\n";
        let diagnostics = lint_control_characters(content, false);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("line 2"));
    }
}
