//! Target-format escaping.
//!
//! Schema validation already constrains theme input, but the compiler is the
//! last line of defence before a value reaches a line-oriented config file or a
//! shell script. Every helper here is deliberately conservative: it either
//! quotes correctly for the target language or strips the characters that make
//! escaping ambiguous.

use kiseki_types::{KisekiError, Result};

/// Removes characters that could break out of a config value or a template.
///
/// Used for human-facing labels (prompt badges, fastfetch titles) where the
/// correct behaviour is "show something safe", not "fail the switch".
pub fn sanitize_text(raw: &str, max_len: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_len));
    for ch in raw.chars() {
        if out.chars().count() >= max_len {
            break;
        }
        if ch.is_control() {
            continue;
        }
        if matches!(
            ch,
            '`' | '$' | ';' | '|' | '&' | '<' | '>' | '\\' | '"' | '\'' | '{' | '}'
        ) {
            continue;
        }
        out.push(ch);
    }
    out.trim().to_string()
}

/// Quotes a value for a POSIX shell single-quoted string.
///
/// Inside single quotes the shell expands nothing, so `$`, backticks and `;`
/// are inert. The only character needing work is the single quote itself.
pub fn shell_single_quote(raw: &str) -> String {
    let cleaned = strip_control(raw);
    format!("'{}'", cleaned.replace('\'', r"'\''"))
}

/// Every character PowerShell treats as a single-quote string delimiter.
///
/// PowerShell accepts the ASCII apostrophe and the four Unicode "curly"
/// single quotes as literal-string delimiters, so all five must be doubled.
const POWERSHELL_QUOTE_DELIMITERS: &[char] = &['\'', '\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'];

/// Quotes a value for a PowerShell single-quoted string.
pub fn powershell_single_quote(raw: &str) -> String {
    let cleaned = strip_control(raw);
    let mut out = String::with_capacity(cleaned.len() + 2);
    out.push('\'');
    for ch in cleaned.chars() {
        if POWERSHELL_QUOTE_DELIMITERS.contains(&ch) {
            out.push(ch);
            out.push(ch);
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Renders a value for a line-oriented `key = value` config (Ghostty).
///
/// Newlines, NULs and `#` are rejected because they can terminate the value or
/// start a comment; spaces are allowed and common in font family names.
pub fn conf_value(raw: &str) -> Result<String> {
    if raw.chars().any(|ch| ch.is_control() || matches!(ch, '#' | '"')) {
        return Err(KisekiError::compiler(format!(
            "value {raw:?} cannot be represented safely in a line-oriented config"
        )));
    }
    Ok(raw.to_string())
}

/// Escapes a TOML basic string, including the surrounding quotes.
pub fn toml_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04X}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Escapes a TOML bare key, quoting when necessary.
pub fn toml_key(raw: &str) -> String {
    let bare = !raw.is_empty()
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));
    if bare {
        raw.to_string()
    } else {
        toml_string(raw)
    }
}

/// Serializes a string as a JSON string literal (no surrounding array/object).
pub fn json_string(raw: &str) -> String {
    serde_json::to_string(raw).unwrap_or_else(|_| "\"\"".to_string())
}

fn strip_control(raw: &str) -> String {
    raw.chars().filter(|ch| !ch.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_single_quote_neutralises_injection() {
        assert_eq!(shell_single_quote("plain"), "'plain'");
        assert_eq!(
            shell_single_quote("$(rm -rf ~); `id` | & > <"),
            "'$(rm -rf ~); `id` | & > <'"
        );
        assert_eq!(shell_single_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_single_quote("line\nbreak"), "'linebreak'");
    }

    #[test]
    fn powershell_single_quote_doubles_quotes() {
        assert_eq!(powershell_single_quote("it's"), "'it''s'");
        assert_eq!(
            powershell_single_quote("$(evil); `id`"),
            "'$(evil); `id`'"
        );
        assert_eq!(powershell_single_quote("a\r\nb"), "'ab'");
    }

    #[test]
    fn powershell_doubles_every_unicode_quote_delimiter() {
        // PowerShell treats all five characters as literal-string delimiters.
        for quote in ['\'', '\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'] {
            let payload = format!("Pilot{quote}s Console");
            let quoted = powershell_single_quote(&payload);
            assert!(quoted.starts_with('\'') && quoted.ends_with('\''), "{quoted}");
            let inner = &quoted[1..quoted.len() - 1];
            assert_eq!(inner.matches(quote).count(), 2, "{quoted}");
        }
        assert_eq!(
            powershell_single_quote("Pilot\u{2019}s Console"),
            "'Pilot\u{2019}\u{2019}s Console'"
        );
    }

    #[test]
    fn powershell_neutralises_unicode_apostrophe_injection() {
        for payload in [
            "x\u{2019}; Invoke-Expression 'evil",
            "it\u{2018}s `$($(evil))`",
            "a\u{201A}b\u{201B}c",
        ] {
            let quoted = powershell_single_quote(payload);
            assert!(quoted.starts_with('\''), "{quoted}");
            assert!(quoted.ends_with('\''), "{quoted}");
            let inner = &quoted[1..quoted.len() - 1];
            // Every delimiter inside the payload must appear doubled so the
            // literal string can never be closed early.
            for quote in POWERSHELL_QUOTE_DELIMITERS {
                let mut chars = inner.chars().peekable();
                while let Some(ch) = chars.next() {
                    if ch == *quote {
                        assert_eq!(chars.next(), Some(*quote), "{quoted} has an odd run of {quote:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn conf_value_rejects_breaks_but_allows_spaces() {
        assert_eq!(conf_value("JetBrainsMono Nerd Font").unwrap(), "JetBrainsMono Nerd Font");
        assert!(conf_value("a\nb").is_err());
        assert!(conf_value("a#b").is_err());
        assert!(conf_value("a\"b").is_err());
        // Windows absolute paths contain backslashes; they are literal in a
        // line-oriented config and must not fail compilation.
        assert_eq!(conf_value("C:\\Users\\zz\\wall.png").unwrap(), "C:\\Users\\zz\\wall.png");
    }

    #[test]
    fn toml_string_escapes_every_quote_and_control() {
        assert_eq!(toml_string("plain"), "\"plain\"");
        assert_eq!(toml_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(toml_string("a\nb"), "\"a\\nb\"");
        assert_eq!(toml_key("primary"), "primary");
        assert_eq!(toml_key("a b"), "\"a b\"");
    }

    #[test]
    fn sanitize_text_strips_template_and_shell_metacharacters() {
        assert_eq!(sanitize_text("UNIT-02", 48), "UNIT-02");
        assert_eq!(sanitize_text("{{ .Env.SECRET }}", 48), ".Env.SECRET");
        assert_eq!(sanitize_text("a$(b);c|d&e<f>g`h`", 48), "a(b)cdefgh");
        assert_eq!(sanitize_text("  padded  ", 48), "padded");
        assert_eq!(sanitize_text("abcdef", 3), "abc");
        assert_eq!(sanitize_text("a\tb", 48), "ab");
    }

    #[test]
    fn json_string_roundtrips() {
        let raw = "a\"b\\c\n\u{7}";
        let encoded = json_string(raw);
        let decoded: String = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, raw);
    }
}
