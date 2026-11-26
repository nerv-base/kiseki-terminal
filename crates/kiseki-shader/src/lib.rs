//! Parameterized shader pipeline and static validation (RFC-0004).
//!
//! Three responsibilities, all pure:
//! 1. build the `KISEKI_*` macro header from a resolved theme,
//! 2. wrap a theme's template into a complete host program and validate it
//!    with the embedded `naga` frontend + validator,
//! 3. run the security/complexity gates that `naga` cannot express
//!    (reversed `smoothstep`, unguarded division, TDR budgets).

pub mod budget;
pub mod glsl;
pub mod header;
pub mod hlsl;
pub mod lint;
pub mod report;

pub use budget::Budget;
pub use glsl::{compile_glsl, compile_glsl_for, lint_glsl_source, validate_glsl, GlslProgram};
pub use header::{build_header, HeaderParams, ShaderLanguage};
pub use hlsl::{compile_hlsl, compile_hlsl_for, lint_hlsl_source, validate_hlsl, HlslProgram};
pub use lint::{lint_source, SourceLint};
pub use report::{ShaderReport, ShaderStage};
