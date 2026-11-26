//! Validation report types shared by the GLSL and HLSL pipelines.

use kiseki_types::{Diagnostic, DiagnosticSeverity, RenderingProfile};

use crate::lint::SourceLint;

/// Which shader language a report describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    Glsl,
    Hlsl,
}

impl ShaderStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Glsl => "glsl",
            Self::Hlsl => "hlsl",
        }
    }

    /// The generated artefact name for this stage.
    pub const fn artifact_name(self) -> &'static str {
        match self {
            Self::Glsl => "active.glsl",
            Self::Hlsl => "active.hlsl",
        }
    }
}

impl std::fmt::Display for ShaderStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything validation learned about one shader at one profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderReport {
    pub stage: ShaderStage,
    pub profile: RenderingProfile,
    /// Size of the final compiled program in bytes.
    pub bytes: usize,
    /// True when the language frontend parsed the program.
    pub parsed: bool,
    /// True when the IR validator accepted the parsed module.
    pub validated: bool,
    /// Source-level findings (UB, budgets, structural requirements).
    pub lint: SourceLint,
    /// Combined findings, including parse/validate failures.
    pub diagnostics: Vec<Diagnostic>,
}

impl ShaderReport {
    pub fn new(stage: ShaderStage, profile: RenderingProfile) -> Self {
        Self {
            stage,
            profile,
            bytes: 0,
            parsed: false,
            validated: false,
            lint: SourceLint::default(),
            diagnostics: Vec::new(),
        }
    }

    /// True when any finding blocks installation.
    pub fn has_fatal(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_fatal)
    }

    /// True when the program parsed, validated and produced no errors.
    pub fn is_ok(&self) -> bool {
        self.parsed && self.validated && !self.has_fatal()
    }

    /// Findings at a specific severity.
    pub fn with_severity(&self, severity: DiagnosticSeverity) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(move |d| d.severity == severity)
    }

    /// One-line human summary used by `kiseki theme validate`.
    pub fn summary(&self) -> String {
        if self.is_ok() {
            format!(
                "{} {}: ok ({} bytes, {} samples, {} smoothstep)",
                self.stage, self.profile, self.bytes, self.lint.texture_samples, self.lint.smoothstep_calls
            )
        } else {
            let errors = self.with_severity(DiagnosticSeverity::Error).count();
            format!("{} {}: {} error(s)", self.stage, self.profile, errors)
        }
    }
}
