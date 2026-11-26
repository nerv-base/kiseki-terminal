//! Ghostty GLSL template compilation and `naga` validation (RFC-0004 §1, §3).
//!
//! Ghostty's ShaderToy-style post-processing shader receives the already
//! composited terminal texture as `iChannel0`. Ghostty itself supplies the
//! `#version`, the uniform/binding preamble and `main()`, so the emitted
//! artefact is only the Kiseki macro header, the shared helpers and the theme's
//! `mainImage` template. The Vulkan validation harness below is applied to a
//! throwaway copy that is handed to `naga` and is never shipped.

use kiseki_types::Diagnostic;
use naga::valid::{Capabilities, ValidationFlags, Validator};

use crate::budget::Budget;
use crate::header::{HeaderParams, ShaderLanguage, HELPERS_GLSL};
use crate::lint::{lint_with_budget, SourceLint};
use crate::report::{ShaderReport, ShaderStage};

/// The uniform block every Kiseki GLSL program shares with the host.
///
/// Ghostty exposes these as ShaderToy-style globals; declaring them as a
/// binding block is what `naga`'s Vulkan GLSL frontend accepts.
const FRAME_BLOCK: &str = r#"layout(set = 0, binding = 0) uniform KisekiFrame {
    vec3 iResolution;
    float iTime;
    vec4 iMouse;
    float iTimeDelta;
    int iFrame;
    float iFrameRate;
};
"#;

/// Channel bindings. `iChannel0` is a macro alias so theme code reads naturally.
const CHANNEL_BINDINGS: &str = r#"layout(set = 0, binding = 1) uniform texture2D kiseki_ch0_tex;
layout(set = 0, binding = 2) uniform sampler kiseki_ch0_smp;
#define iChannel0 sampler2D(kiseki_ch0_tex, kiseki_ch0_smp)
"#;

/// A compiled GLSL program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlslProgram {
    /// What Ghostty consumes: header + helpers + the theme's `mainImage`.
    pub source: String,
    /// Just the macro header, for diffing and debugging.
    pub header: String,
}

impl GlslProgram {
    /// The program text written to `generated/shaders/active.glsl`.
    pub fn as_str(&self) -> &str {
        &self.source
    }
}

/// Wraps a theme template into the artefact Ghostty loads.
///
/// `template` must define `mainImage(out vec4, in vec2)`. Ghostty provides its
/// own preamble and `main()`, so emitting either here would produce a shader
/// Ghostty cannot compile (duplicate `main`, conflicting `#version`).
pub fn compile_glsl(header: &str, template: &str) -> GlslProgram {
    let mut source = String::with_capacity(header.len() + template.len() + HELPERS_GLSL.len());
    source.push_str(header);
    source.push_str(HELPERS_GLSL);
    source.push_str("\n// --- Theme template ------------------------------------------------\n");
    source.push_str(template);

    GlslProgram {
        source,
        header: header.to_string(),
    }
}

/// Builds the complete Vulkan GLSL 450 program used **only** for `naga`.
fn validation_source(header: &str, template: &str) -> String {
    let mut source = String::with_capacity(header.len() + template.len() + 4096);
    source.push_str("#version 450\n");
    source.push_str("layout(location = 0) out vec4 kiseki_fragColor;\n");
    source.push_str(FRAME_BLOCK);
    source.push_str(CHANNEL_BINDINGS);
    source.push_str(&compile_glsl(header, template).source);
    source.push_str(
        r#"
// --- Kiseki validation entry point ---------------------------------------
void main() {
    vec4 kiseki_color;
    mainImage(kiseki_color, gl_FragCoord.xy);
    kiseki_fragColor = kiseki_color;
}
"#,
    );
    source
}

/// Convenience: build the header for a theme and compile it at a profile.
pub fn compile_glsl_for(theme: &kiseki_schema::ResolvedTheme, template: &str) -> GlslProgram {
    let header = crate::header::build_header(theme, HeaderParams::from_theme(theme), ShaderLanguage::Glsl);
    compile_glsl(&header, template)
}

/// Validates a theme GLSL template at a rendering profile.
///
/// Runs, in order: source lint (UB + budgets), `naga` parse, `naga` validate,
/// and entry-point presence checks.
pub fn validate_glsl(
    theme: &kiseki_schema::ResolvedTheme,
    template: &str,
) -> ShaderReport {
    let budget = Budget::for_profile(theme.profile);
    let program = compile_glsl_for(theme, template);
    let header = crate::header::build_header(theme, HeaderParams::from_theme(theme), ShaderLanguage::Glsl);

    let mut report = ShaderReport::new(ShaderStage::Glsl, theme.profile);
    // Lint the *preprocessed* program: a 16-iteration loop guarded by
    // `#if KISEKI_QUALITY_LEVEL >= 3` must not count against Eco's budget, and
    // unsupported `#if`/`#elif` syntax produces a blocking diagnostic instead
    // of silently skipping an active Ghostty branch.
    let (lint_source, pp_diagnostics) = crate::hlsl::expand_macros_checked(&program.source);
    report.diagnostics.extend(pp_diagnostics);
    report.lint = lint_with_budget(&lint_source, budget);
    report.bytes = program.source.len();

    // The theme template itself must declare the expected entry point; the
    // validation wrapper is not allowed to paper over a missing `mainImage`.
    if !template.contains("mainImage") {
        report.diagnostics.push(Diagnostic::error(
            "shader.glsl.entry-point",
            "GLSL template does not define `mainImage(out vec4, in vec2)`",
        ));
    }

    if report.lint.has_fatal() {
        // Parse errors after a known-fatal lint would only add noise.
        report.diagnostics.extend(report.lint.diagnostics.items.clone());
        return report;
    }

    report.diagnostics.extend(report.lint.diagnostics.items.clone());

    match parse_and_validate(&validation_source(&header, template)) {
        Ok(entry_points) => {
            if !entry_points.iter().any(|name| name == "main") {
                report.diagnostics.push(Diagnostic::error(
                    "shader.glsl.entry-point",
                    "compiled program has no `main` entry point",
                ));
            }
            report.parsed = true;
            report.validated = true;
        }
        Err(message) => {
            report.diagnostics.push(Diagnostic::error("shader.glsl.invalid", message));
        }
    }

    report
}

/// Parses and validates a complete GLSL 450 program with `naga`.
///
/// Returns the entry-point names on success, or a single-line message on
/// failure.
fn parse_and_validate(source: &str) -> Result<Vec<String>, String> {
    let mut frontend = naga::front::glsl::Frontend::default();
    let options = naga::front::glsl::Options {
        stage: naga::ShaderStage::Fragment,
        defines: Default::default(),
    };
    let module = frontend
        .parse(&options, source)
        .map_err(|errors| format!("GLSL parse failed: {errors:?}"))?;

    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::empty());
    validator
        .validate(&module)
        .map_err(|error| format!("GLSL validation failed: {error:?}"))?;

    Ok(module
        .entry_points
        .iter()
        .map(|entry| entry.name.clone())
        .collect())
}

/// Exposed for tests and tooling: lint a raw GLSL source without a theme.
pub fn lint_glsl_source(source: &str, budget: Budget) -> SourceLint {
    lint_with_budget(source, budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::ShaderStage;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_types::RenderingProfile;

    const NERV_EVA_GLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.glsl");

    fn pack() -> ThemePack {
        ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
        }
    }

    fn theme(profile: RenderingProfile) -> ResolvedTheme {
        ResolvedTheme::from_pack(&pack(), profile)
    }

    #[test]
    fn nerv_eva_glsl_validates_at_every_dynamic_profile() {
        for profile in [
            RenderingProfile::Eco,
            RenderingProfile::Balanced,
            RenderingProfile::Ultra,
        ] {
            let theme = theme(profile);
            let report = validate_glsl(&theme, NERV_EVA_GLSL);
            let errors: Vec<String> = report
                .diagnostics
                .iter()
                .filter(|d| d.is_fatal())
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect();
            assert!(errors.is_empty(), "{profile} -> {errors:#?}");
            assert!(report.parsed && report.validated, "{profile} not validated");
            assert_eq!(report.stage, ShaderStage::Glsl);
        }
    }

    #[test]
    fn compiled_program_is_a_ghostty_template() {
        let program = compile_glsl_for(&theme(RenderingProfile::Balanced), NERV_EVA_GLSL);
        // Ghostty wraps `mainImage` with its own preamble and `main()`.
        assert!(!program.source.contains("#version"), "{}...", &program.source[..80]);
        assert!(!program.source.contains("void main()"));
        assert!(!program.source.contains("kiseki_fragColor"));
        assert!(!program.source.contains("layout(set = 0, binding = 0)"));
        assert!(program.source.contains("mainImage"));
        assert!(program.source.contains("#define KISEKI_QUALITY_LEVEL 2"));
        assert!(program.source.contains("float kiseki_bg_mask(vec4 term, vec2 uv)"));
        assert!(program.source.contains("float kiseki_narrow_factor(vec2 res)"));
        assert!(program.header.contains("KISEKI_ACCENT_PRIMARY"));
        assert!(program.source.starts_with("// === Auto-generated"));
    }

    #[test]
    fn compilation_is_deterministic() {
        let theme = theme(RenderingProfile::Ultra);
        let first = compile_glsl_for(&theme, NERV_EVA_GLSL);
        let second = compile_glsl_for(&theme, NERV_EVA_GLSL);
        assert_eq!(first.source, second.source);
    }

    #[test]
    fn validation_wrapper_is_never_emitted() {
        let theme = theme(RenderingProfile::Balanced);
        let program = compile_glsl_for(&theme, NERV_EVA_GLSL);
        let wrapper = validation_source(
            &crate::header::build_header(
                &theme,
                HeaderParams::from_theme(&theme),
                ShaderLanguage::Glsl,
            ),
            NERV_EVA_GLSL,
        );
        assert!(wrapper.starts_with("#version 450\n"));
        assert!(wrapper.contains("void main()"));
        assert!(!program.source.contains("void main()"));
        assert!(!program.source.contains("#version"));
    }

    #[test]
    fn quality_levels_change_the_compiled_output() {
        let eco = compile_glsl_for(&theme(RenderingProfile::Eco), NERV_EVA_GLSL);
        let ultra = compile_glsl_for(&theme(RenderingProfile::Ultra), NERV_EVA_GLSL);
        assert_ne!(eco.source, ultra.source);
        assert!(eco.source.contains("#define KISEKI_QUALITY_LEVEL 1"));
        assert!(ultra.source.contains("#define KISEKI_QUALITY_LEVEL 3"));
    }

    #[test]
    fn reversed_smoothstep_template_is_rejected() {
        let template = r#"
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    float d = smoothstep(0.86, 0.81, fragCoord.x);
    fragColor = vec4(d);
}
"#;
        let report = validate_glsl(&theme(RenderingProfile::Balanced), template);
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.smoothstep.reversed"));
        assert!(!report.parsed, "must not even attempt to parse");
    }

    #[test]
    fn syntax_error_template_is_rejected_by_naga() {
        let template = r#"
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    fragColor = this_function_does_not_exist(fragCoord);
}
"#;
        let report = validate_glsl(&theme(RenderingProfile::Balanced), template);
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.glsl.invalid"));
    }

    #[test]
    fn missing_entry_point_is_rejected() {
        let report = validate_glsl(&theme(RenderingProfile::Balanced), "float unused() { return 1.0; }");
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.glsl.entry-point"));
    }

    #[test]
    fn ungarded_division_template_is_rejected() {
        let template = r#"
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec4 term = texture(iChannel0, fragCoord);
    fragColor = vec4(term.rgb / term.a, 1.0);
}
"#;
        let report = validate_glsl(&theme(RenderingProfile::Balanced), template);
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.division.unguarded"));
    }

    #[test]
    fn excessive_sample_budget_is_rejected_at_eco() {
        let template = r#"
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord;
    vec4 a = texture(iChannel0, uv);
    vec4 b = texture(iChannel0, uv + 0.1);
    vec4 c = texture(iChannel0, uv + 0.2);
    vec4 d = texture(iChannel0, uv + 0.3);
    vec4 e = texture(iChannel0, uv + 0.4);
    fragColor = a + b + c + d + e;
}
"#;
        let report = validate_glsl(&theme(RenderingProfile::Eco), template);
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.budget.texture-samples"));
    }

    #[test]
    fn long_loop_is_rejected_at_eco_but_allowed_at_ultra() {
        let template = r#"
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec3 acc = vec3(0.0);
    for (int i = 0; i < 16; i++) {
        acc += texture(iChannel0, fragCoord + float(i) * 0.01).rgb;
    }
    fragColor = vec4(acc, 1.0);
}
"#;
        let eco = validate_glsl(&theme(RenderingProfile::Eco), template);
        assert!(eco.has_fatal(), "eco must reject a 16-iteration loop");
        let ultra = validate_glsl(&theme(RenderingProfile::Ultra), template);
        assert!(!ultra.has_fatal(), "{:?}", ultra.diagnostics);
    }

    #[test]
    fn profile_guarded_loop_is_linted_after_preprocessing() {
        let template = r#"
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec3 acc = vec3(0.0);
#if KISEKI_QUALITY_LEVEL >= 3
    for (int i = 0; i < 16; i++) {
        acc += texture(iChannel0, fragCoord + float(i) * 0.01).rgb;
    }
#endif
    fragColor = vec4(acc, 1.0);
}
"#;
        // The loop is compiled out at Eco, so its 16 iterations never count
        // against the 8-iteration budget.
        let eco = validate_glsl(&theme(RenderingProfile::Eco), template);
        assert!(!eco.has_fatal(), "{:?}", eco.diagnostics);
        assert!(!eco
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.budget.loop-iterations"));
        // At Ultra the loop exists and is within the high budget.
        let ultra = validate_glsl(&theme(RenderingProfile::Ultra), template);
        assert!(!ultra.has_fatal(), "{:?}", ultra.diagnostics);
        assert_eq!(ultra.lint.max_loop_iterations, Some(16));
    }
}
