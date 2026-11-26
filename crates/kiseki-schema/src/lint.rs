//! WCAG 2.1 contrast lint (RFC-0002 §4).
//!
//! Errors block installation; warnings are advisory. `Accessibility` profile
//! rendering repairs the errors instead of reporting them (see `resolve`).

use kiseki_types::{Color, Diagnostic, DiagnosticReport, DiagnosticSeverity};

use crate::tokens::{PaletteTokens, SyntaxTokens};

/// Minimum ratios from the RFC-0002 table.
pub mod threshold {
    /// `text.primary` vs `surfaces.base` — WCAG AAA.
    pub const TEXT_PRIMARY: f64 = 7.0;
    /// `text.secondary` vs `surfaces.base` — WCAG AA.
    pub const TEXT_SECONDARY: f64 = 4.5;
    /// `text.muted` / `syntax.comment` vs `surfaces.base`.
    pub const TEXT_MUTED: f64 = 3.0;
    /// ANSI red/green/yellow/blue/magenta/cyan vs `surfaces.base`.
    pub const ANSI: f64 = 4.0;
    /// `selectionFg` vs `selectionBg`.
    pub const SELECTION: f64 = 4.5;
    /// `cursorText` vs `cursor`.
    pub const CURSOR: f64 = 4.5;
    /// Recommended ceiling above the muted floor.
    pub const TEXT_MUTED_RECOMMENDED: f64 = 3.8;
    /// Recommended ANSI ratio.
    pub const ANSI_RECOMMENDED: f64 = 4.5;
    /// Recommended selection ratio.
    pub const SELECTION_RECOMMENDED: f64 = 6.0;
}

/// One contrast measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastCheck {
    /// Stable identifier, e.g. `text.primary`.
    pub name: String,
    pub foreground: Color,
    pub background: Color,
    pub ratio: f64,
    /// Minimum ratio that must be met (error threshold).
    pub minimum: f64,
    /// Advisory ratio that is nice to meet.
    pub recommended: Option<f64>,
}

impl ContrastCheck {
    pub fn passes_minimum(&self) -> bool {
        self.ratio >= self.minimum
    }

    pub fn meets_recommended(&self) -> bool {
        self.recommended.is_none_or(|target| self.ratio >= target)
    }
}

/// The full contrast report for one palette.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContrastReport {
    pub checks: Vec<ContrastCheck>,
}

impl ContrastReport {
    /// True when every check clears its error threshold.
    pub fn is_compliant(&self) -> bool {
        self.checks.iter().all(ContrastCheck::passes_minimum)
    }

    /// Converts the report into schema diagnostics.
    pub fn to_diagnostics(&self) -> DiagnosticReport {
        let mut report = DiagnosticReport::new();
        for check in &self.checks {
            if !check.passes_minimum() {
                report.push(Diagnostic::new(
                    DiagnosticSeverity::Error,
                    "wcag.contrast.fail",
                    format!(
                        "{} has {:.2}:1 contrast against its background (requires {:.1}:1)",
                        check.name, check.ratio, check.minimum
                    ),
                ));
            } else if !check.meets_recommended() {
                let target = check.recommended.unwrap_or_default();
                report.push(Diagnostic::new(
                    DiagnosticSeverity::Warning,
                    "wcag.contrast.recommended",
                    format!(
                        "{} has {:.2}:1 contrast, below the recommended {:.1}:1",
                        check.name, check.ratio, target
                    ),
                ));
            }
        }
        report
    }

    /// Looks up a single check by name.
    pub fn get(&self, name: &str) -> Option<&ContrastCheck> {
        self.checks.iter().find(|check| check.name == name)
    }
}

/// Measures every required contrast pair for a palette.
pub fn lint_contrast(palette: &PaletteTokens, syntax: Option<&SyntaxTokens>) -> ContrastReport {
    let base = palette.surfaces.base;
    let mut checks = Vec::new();

    let mut push = |name: &str, foreground: Color, background: Color, minimum: f64, recommended: Option<f64>| {
        checks.push(ContrastCheck {
            name: name.to_string(),
            foreground,
            background,
            ratio: foreground.contrast_ratio(background),
            minimum,
            recommended,
        });
    };

    push(
        "text.primary",
        palette.text.primary,
        base,
        threshold::TEXT_PRIMARY,
        None,
    );
    push(
        "text.secondary",
        palette.text.secondary,
        base,
        threshold::TEXT_SECONDARY,
        None,
    );
    push(
        "text.muted",
        palette.text.muted,
        base,
        threshold::TEXT_MUTED,
        Some(threshold::TEXT_MUTED_RECOMMENDED),
    );
    if let Some(syntax) = syntax {
        push(
            "syntax.comment",
            syntax.comment,
            base,
            threshold::TEXT_MUTED,
            Some(threshold::TEXT_MUTED_RECOMMENDED),
        );
    }
    for (name, color) in palette.ansi.contrast_checked() {
        push(
            &format!("ansi.{name}"),
            color,
            base,
            threshold::ANSI,
            Some(threshold::ANSI_RECOMMENDED),
        );
    }
    push(
        "interaction.selectionFg",
        palette.interaction.selection_fg,
        palette.interaction.selection_bg,
        threshold::SELECTION,
        Some(threshold::SELECTION_RECOMMENDED),
    );
    push(
        "interaction.cursorText",
        palette.interaction.cursor_text,
        palette.interaction.cursor,
        threshold::CURSOR,
        None,
    );

    ContrastReport { checks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::nerv_eva_tokens;

    #[test]
    fn nerv_eva_palette_is_compliant() {
        let tokens = nerv_eva_tokens();
        let report = lint_contrast(&tokens.palette, None);
        let failures: Vec<String> = report
            .checks
            .iter()
            .filter(|check| !check.passes_minimum())
            .map(|check| format!("{} = {:.2}", check.name, check.ratio))
            .collect();
        assert!(report.is_compliant(), "failing: {failures:?}");
    }

    #[test]
    fn report_covers_every_required_pair() {
        let tokens = nerv_eva_tokens();
        let syntax = crate::derive::derive_syntax(&tokens.palette, None);
        let report = lint_contrast(&tokens.palette, Some(&syntax));
        for name in [
            "text.primary",
            "text.secondary",
            "text.muted",
            "syntax.comment",
            "ansi.red",
            "ansi.green",
            "ansi.yellow",
            "ansi.blue",
            "ansi.magenta",
            "ansi.cyan",
            "interaction.selectionFg",
            "interaction.cursorText",
        ] {
            assert!(report.get(name).is_some(), "missing check {name}");
        }
        assert_eq!(report.checks.len(), 12);
    }

    #[test]
    fn low_contrast_primary_text_is_fatal() {
        let mut tokens = nerv_eva_tokens();
        tokens.palette.text.primary = tokens.palette.surfaces.base.mix_oklch(kiseki_types::Color::WHITE, 0.2);
        let report = lint_contrast(&tokens.palette, None);
        assert!(!report.is_compliant());
        let diagnostics = report.to_diagnostics();
        assert!(diagnostics.has_fatal());
        assert!(diagnostics.iter().any(|d| d.message.contains("text.primary")));
    }

    #[test]
    fn recommended_band_produces_warning_only() {
        let mut tokens = nerv_eva_tokens();
        // #666060 sits between 3.0 and 3.8 against the canvas.
        tokens.palette.text.muted = kiseki_types::Color::parse_hex("#666060").unwrap();
        let report = lint_contrast(&tokens.palette, None);
        let muted = report.get("text.muted").unwrap();
        assert!(muted.passes_minimum());
        assert!(!muted.meets_recommended());
        let diagnostics = report.to_diagnostics();
        assert!(!diagnostics.has_fatal());
        assert!(diagnostics.iter().any(|d| d.code == "wcag.contrast.recommended"));
    }
}
