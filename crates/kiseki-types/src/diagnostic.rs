//! Diagnostics surfaced by validation, lint and `doctor`.

use serde::{Deserialize, Serialize};

/// How badly a diagnostic affects the operation that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    /// Informational only; the operation is fully healthy.
    Info,
    /// Renders correctly but violates a recommendation.
    Warning,
    /// Blocks the operation: the transaction must fail before any write.
    Error,
}

impl DiagnosticSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// True for severities that must abort a transaction.
    pub const fn is_fatal(self) -> bool {
        matches!(self, Self::Error)
    }
}

impl std::fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single finding, always tied to a stable machine-readable code.
///
/// `code` is what `doctor --json` consumers and tests key off, so it must stay
/// stable across releases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    /// Stable identifier, e.g. `schema.color.invalid`.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// Optional remediation hint shown by `doctor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Diagnostic {
    pub fn new(severity: DiagnosticSeverity, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            hint: None,
        }
    }

    /// Convenience constructor for blocking findings.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Error, code, message)
    }

    /// Convenience constructor for advisory findings.
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Warning, code, message)
    }

    /// Convenience constructor for informational findings.
    pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Info, code, message)
    }

    /// Attaches a remediation hint.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub const fn is_fatal(&self) -> bool {
        self.severity.is_fatal()
    }
}

/// Aggregates diagnostics and answers "may we write?".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub items: Vec<Diagnostic>,
}

impl DiagnosticReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn extend(&mut self, diagnostics: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(diagnostics);
    }

    /// True when at least one finding blocks the operation.
    pub fn has_fatal(&self) -> bool {
        self.items.iter().any(Diagnostic::is_fatal)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Counts findings by severity.
    pub fn count(&self, severity: DiagnosticSeverity) -> usize {
        self.items.iter().filter(|d| d.severity == severity).count()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.items.iter()
    }
}

impl IntoIterator for DiagnosticReport {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl FromIterator<Diagnostic> for DiagnosticReport {
    fn from_iter<T: IntoIterator<Item = Diagnostic>>(iter: T) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_severity_is_fatal() {
        assert!(DiagnosticSeverity::Error.is_fatal());
        assert!(!DiagnosticSeverity::Warning.is_fatal());
        assert!(!DiagnosticSeverity::Info.is_fatal());
    }

    #[test]
    fn report_tracks_fatal_and_counts() {
        let mut report = DiagnosticReport::new();
        report.push(Diagnostic::info("a.b", "fine"));
        report.push(Diagnostic::warning("c.d", "meh").with_hint("do better"));
        assert!(!report.has_fatal());
        assert_eq!(report.count(DiagnosticSeverity::Info), 1);
        assert_eq!(report.count(DiagnosticSeverity::Warning), 1);
        assert_eq!(report.count(DiagnosticSeverity::Error), 0);

        report.push(Diagnostic::error("e.f", "boom"));
        assert!(report.has_fatal());
        assert_eq!(report.len(), 3);
    }

    #[test]
    fn serde_skips_absent_hint() {
        let diag = Diagnostic::warning("x.y", "msg");
        let json = serde_json::to_string(&diag).unwrap();
        assert!(!json.contains("hint"), "{json}");
        let back: Diagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(back, diag);
    }
}
