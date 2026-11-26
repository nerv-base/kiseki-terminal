//! Comment-marker blocks used for idempotent host-config anchoring.
//!
//! The engine never rewrites a user's whole `.zshrc` or PowerShell profile: it
//! owns one clearly delimited block, updates it in place, and can remove it
//! byte-for-byte during `uninstall` or v1 migration.

/// The v2 marker. v1 used `# >>> kiseki-terminal >>>`.
pub const MARKER_BEGIN: &str = "# >>> kiseki >>>";
/// Closing marker.
pub const MARKER_END: &str = "# <<< kiseki <<<";

/// Byte range of a marker block, markers included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerSpan {
    pub start: usize,
    pub end: usize,
}

/// Finds the marker block in `text`, returning byte offsets.
pub fn find_marker_block(text: &str, begin: &str, end: &str) -> Option<MarkerSpan> {
    let mut search_from = 0;
    while let Some(relative) = text[search_from..].find(begin) {
        let index = search_from + relative;
        let line_start = text[..index].rfind('\n').map(|offset| offset + 1).unwrap_or(0);
        // Only accept the needle when it is the first non-space token on its
        // line, so a comment inside a string literal cannot be mistaken for a
        // marker.
        if !text[line_start..index].trim().is_empty() {
            search_from = index + begin.len();
            continue;
        }
        let after_begin = index + begin.len();
        let end_relative = text[after_begin..].find(end)?;
        let end_marker_start = after_begin + end_relative;
        let end_of_line = text[end_marker_start..]
            .find('\n')
            .map(|offset| end_marker_start + offset + 1)
            .unwrap_or(text.len());
        return Some(MarkerSpan {
            start: line_start,
            end: end_of_line,
        });
    }
    None
}

/// Detects the newline style used by a document.
fn newline_of(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn render_block(begin: &str, end: &str, body: &str, newline: &str) -> String {
    let body = body.trim_matches(['\r', '\n']);
    if body.is_empty() {
        format!("{begin}{newline}{end}{newline}")
    } else {
        let mut out = String::new();
        out.push_str(begin);
        out.push_str(newline);
        for line in body.lines() {
            out.push_str(line);
            out.push_str(newline);
        }
        out.push_str(end);
        out.push_str(newline);
        out
    }
}

/// Inserts or replaces the marker block, leaving every other byte untouched.
///
/// Idempotent: feeding the output back in produces the same document.
pub fn upsert_marker_block(text: &str, begin: &str, end: &str, body: &str) -> String {
    let newline = newline_of(text);
    let block = render_block(begin, end, body, newline);
    if let Some(span) = find_marker_block(text, begin, end) {
        let mut out = String::with_capacity(text.len() + block.len());
        out.push_str(&text[..span.start]);
        out.push_str(&block);
        out.push_str(&text[span.end..]);
        return out;
    }

    let mut out = String::with_capacity(text.len() + block.len() + 2);
    out.push_str(text);
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push_str(newline);
        }
        if !out.ends_with(&format!("{newline}{newline}")) {
            out.push_str(newline);
        }
    }
    out.push_str(&block);
    out
}

/// Removes the marker block and the blank separator line we may have added.
pub fn remove_marker_block(text: &str, begin: &str, end: &str) -> String {
    let Some(span) = find_marker_block(text, begin, end) else {
        return text.to_string();
    };
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..span.start]);
    out.push_str(&text[span.end..]);
    // Collapse the trailing blank line left behind when the block was appended.
    let newline = newline_of(&out);
    while out.ends_with(&format!("{newline}{newline}")) {
        out.truncate(out.len() - newline.len());
    }
    out
}

/// True when the document already contains our marker block.
pub fn has_marker_block(text: &str, begin: &str, end: &str) -> bool {
    find_marker_block(text, begin, end).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_appends_with_a_separator_and_is_idempotent() {
        let original = "export PATH=/usr/bin\n";
        let once = upsert_marker_block(original, MARKER_BEGIN, MARKER_END, "source '/x/env.zsh'");
        assert!(once.starts_with(original));
        assert!(once.contains(MARKER_BEGIN));
        assert!(once.contains("source '/x/env.zsh'"));
        assert!(once.ends_with(MARKER_END) || once.ends_with(&format!("{MARKER_END}\n")));

        let twice = upsert_marker_block(&once, MARKER_BEGIN, MARKER_END, "source '/x/env.zsh'");
        assert_eq!(once, twice, "upsert must be idempotent");
    }

    #[test]
    fn upsert_replaces_only_the_block() {
        let original = format!(
            "# user line\nalias ll='ls -l'\n{begin}\nold body\n{end}\nexport FOO=1\n",
            begin = MARKER_BEGIN,
            end = MARKER_END
        );
        let updated = upsert_marker_block(&original, MARKER_BEGIN, MARKER_END, "new body");
        assert!(updated.contains("# user line"));
        assert!(updated.contains("alias ll='ls -l'"));
        assert!(updated.contains("export FOO=1"));
        assert!(updated.contains("new body"));
        assert!(!updated.contains("old body"));
        assert_eq!(updated.matches(MARKER_BEGIN).count(), 1);
    }

    #[test]
    fn remove_restores_the_document() {
        let original = "line one\nline two\n";
        let with_block = upsert_marker_block(original, MARKER_BEGIN, MARKER_END, "body");
        let removed = remove_marker_block(&with_block, MARKER_BEGIN, MARKER_END);
        assert_eq!(removed, original);
        assert!(!removed.contains(MARKER_BEGIN));
    }

    #[test]
    fn remove_is_a_noop_when_absent() {
        let text = "nothing here\n";
        assert_eq!(remove_marker_block(text, MARKER_BEGIN, MARKER_END), text);
    }

    #[test]
    fn markers_inside_strings_are_not_matched() {
        let text = "echo '# >>> kiseki >>>'\n# >>> kiseki >>>\nbody\n# <<< kiseki <<<\n";
        let span = find_marker_block(text, MARKER_BEGIN, MARKER_END).unwrap();
        assert!(span.start > 20, "must skip the string literal");
        let removed = remove_marker_block(text, MARKER_BEGIN, MARKER_END);
        assert!(removed.contains("echo '# >>> kiseki >>>'"));
    }

    #[test]
    fn crlf_documents_keep_crlf() {
        let text = "line\r\n";
        let updated = upsert_marker_block(text, MARKER_BEGIN, MARKER_END, "body");
        assert!(updated.contains("\r\n"));
        assert!(!updated.contains("\n\n"));
        let removed = remove_marker_block(&updated, MARKER_BEGIN, MARKER_END);
        assert_eq!(removed, text);
    }

    #[test]
    fn empty_document_starts_with_the_block() {
        let updated = upsert_marker_block("", MARKER_BEGIN, MARKER_END, "body");
        assert!(updated.starts_with(MARKER_BEGIN));
        assert!(has_marker_block(&updated, MARKER_BEGIN, MARKER_END));
    }
}
