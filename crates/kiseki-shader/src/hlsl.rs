//! HLSL template preprocessing and static validation (RFC-0004 §1, §3).
//!
//! `naga` has no HLSL frontend, so Windows Terminal shaders are validated by
//! (a) a real C-preprocessor pass that expands the `KISEKI_*` header exactly as
//! the driver would, and (b) structural + lexical gates on the expanded source:
//! the required Windows Terminal contract symbols, reversed `smoothstep`,
//! unguarded division, loop nesting and the TDR budgets.

use kiseki_types::Diagnostic;

use crate::budget::Budget;
use crate::header::{HeaderParams, ShaderLanguage, HELPERS_HLSL};
use crate::lint::{lint_with_budget, SourceLint};
use crate::report::{ShaderReport, ShaderStage};

/// Symbols Windows Terminal's pixel-shader contract requires.
///
/// Missing any of these means WT will silently fall back to no shader, which is
/// exactly the failure mode `doctor` must catch.
pub const REQUIRED_SYMBOLS: &[(&str, &str)] = &[
    ("cbuffer PixelShaderSettings", "the PixelShaderSettings constant buffer"),
    ("shaderTexture", "the t0 text layer texture"),
    ("imageTexture", "the t1 wallpaper layer texture"),
    ("samplerState", "the sampler state"),
    ("SV_TARGET", "the pixel-shader output semantic"),
    ("float4 main(", "the `float4 main(...)` entry point"),
];

/// A compiled HLSL program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlslProgram {
    /// Header + helpers + theme template, with macros expanded.
    pub source: String,
    /// The macro header, unexpanded, for diffing.
    pub header: String,
}

impl HlslProgram {
    pub fn as_str(&self) -> &str {
        &self.source
    }
}

fn assemble_hlsl(header: &str, template: &str) -> String {
    let mut raw = String::with_capacity(header.len() + template.len() + 1024);
    raw.push_str(header);
    raw.push_str(HELPERS_HLSL);
    raw.push_str("\n// --- Theme template ------------------------------------------------\n");
    raw.push_str(template);
    raw
}

fn raw_hlsl(theme: &kiseki_schema::ResolvedTheme, template: &str) -> String {
    let header = crate::header::build_header(theme, HeaderParams::from_theme(theme), ShaderLanguage::Hlsl);
    assemble_hlsl(&header, template)
}

/// Preprocesses a theme HLSL template into a complete, macro-expanded program.
///
/// The header is prepended so theme code can use `KISEKI_*` macros, then the
/// whole document is run through a minimal C preprocessor.
pub fn compile_hlsl(header: &str, template: &str) -> HlslProgram {
    let raw = assemble_hlsl(header, template);
    HlslProgram {
        source: expand_macros(&raw),
        header: header.to_string(),
    }
}

/// Convenience: build the header for a theme and compile it.
pub fn compile_hlsl_for(theme: &kiseki_schema::ResolvedTheme, template: &str) -> HlslProgram {
    let header = crate::header::build_header(theme, HeaderParams::from_theme(theme), ShaderLanguage::Hlsl);
    compile_hlsl(&header, template)
}

/// GLSL vector types that HLSL never accepts.
const GLSL_VECTOR_TYPES: &[&str] = &["vec2", "vec3", "vec4"];

/// Keywords that look like a function call but are not definitions.
const NON_FUNCTION_KEYWORDS: &[&str] = &[
    "if", "for", "while", "switch", "do", "else", "return", "sizeof", "register", "cbuffer", "struct",
];

/// Rejects GLSL vector spellings, duplicate definitions and unexpanded macros
/// in a fully preprocessed HLSL program.
fn lint_hlsl_structure(source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let cleaned = strip_comments_and_strings(source);

    for (index, line) in cleaned.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        for token in line.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')) {
            if GLSL_VECTOR_TYPES.contains(&token) {
                diagnostics.push(
                    Diagnostic::error(
                        "shader.hlsl.glsl-type",
                        format!(
                            "line {}: `{token}` is a GLSL vector type; HLSL uses `{token}3`-style `float` vectors",
                            index + 1
                        ),
                    )
                    .with_hint("replace vec2/vec3/vec4 with float2/float3/float4"),
                );
            }
        }
    }

    for (name, first_line) in duplicate_functions(&cleaned) {
        diagnostics.push(
            Diagnostic::error(
                "shader.hlsl.duplicate-function",
                format!(
                    "`{name}` is defined more than once (first at line {first_line}); the helper is already injected by Kiseki"
                ),
            )
            .with_hint("delete the template copy; `HELPERS_HLSL` already provides the shared helper"),
        );
    }

    diagnostics
}

/// Returns every function signature defined more than once at file scope, with
/// the line number of its first definition.
///
/// HLSL supports overloading, so two definitions only collide when both the
/// name *and* the normalized parameter types match (`float foo(float)` and
/// `float foo(float2)` are valid overloads).
fn duplicate_functions(source: &str) -> Vec<(String, usize)> {
    use std::collections::BTreeMap;

    let mut seen: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    let bytes = source.as_bytes();
    let mut depth: i32 = 0;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'(' if depth == 0 => {
                if let Some((name, signature)) = definition_signature(source, index) {
                    let line = line_number(source, index);
                    seen.entry((name, signature))
                        .and_modify(|entry| entry.1 += 1)
                        .or_insert((line, 1));
                }
            }
            _ => {}
        }
        index += 1;
    }
    seen.into_iter()
        .filter(|(_, (_, count))| *count > 1)
        .map(|((name, _), (line, _))| (name, line))
        .collect()
}

/// Normalizes a parameter list so parameter names and whitespace do not hide a
/// real redefinition (`float2 res` and `float2 x` are the same signature).
fn parameter_signature(params: &str) -> String {
    let mut types = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in params.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                types.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    types.push(current);
    if types.len() == 1 && types[0].trim().is_empty() {
        return String::new();
    }
    types
        .iter()
        .map(|param| {
            // Semantics annotate the return value, not the overload identity.
            let param = param.split(':').next().unwrap_or_default().trim();
            let tokens: Vec<&str> = param.split_whitespace().collect();
            let ty = if tokens.len() > 1 {
                &tokens[..tokens.len() - 1]
            } else {
                &tokens[..]
            };
            // In HLSL an omitted direction modifier defaults to `in`, so
            // `in float x` and `float y` have the exact same overload signature.
            let normalized: Vec<&str> = ty
                .iter()
                .copied()
                .filter(|token| *token != "in")
                .collect();
            normalized.join(" ")
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// True when the `(` at `open` belongs to a function definition.
fn definition_signature(source: &str, open: usize) -> Option<(String, String)> {
    let bytes = source.as_bytes();
    let mut cursor = open;
    while cursor > 0 && bytes[cursor - 1].is_ascii_whitespace() {
        cursor -= 1;
    }
    let end = cursor;
    while cursor > 0 && (bytes[cursor - 1].is_ascii_alphanumeric() || bytes[cursor - 1] == b'_') {
        cursor -= 1;
    }
    if cursor == end {
        return None;
    }
    let name = source.get(cursor..end)?;
    if NON_FUNCTION_KEYWORDS.contains(&name) {
        return None;
    }
    // A definition is preceded by a return type (`float4 main(`, `float f(`).
    let mut before = cursor;
    while before > 0 && bytes[before - 1].is_ascii_whitespace() {
        before -= 1;
    }
    if before == 0 {
        return None;
    }
    let previous = bytes[before - 1];
    if !(previous.is_ascii_alphanumeric() || previous == b'_' || previous == b'*' || previous == b'>') {
        return None;
    }
    // The closing paren must be followed by `{` or a `: SEMANTIC` suffix.
    let mut closing = open + 1;
    let mut parens = 1usize;
    while closing < bytes.len() && parens > 0 {
        match bytes[closing] {
            b'(' => parens += 1,
            b')' => parens -= 1,
            _ => {}
        }
        closing += 1;
    }
    if parens != 0 {
        return None;
    }
    let rest = source.get(closing..)?;
    let rest = rest.trim_start();
    let is_definition = rest.starts_with('{') || rest.starts_with(':');
    if !is_definition {
        return None;
    }
    let params = source.get(open + 1..closing.saturating_sub(1))?;
    Some((name.to_string(), parameter_signature(params)))
}

fn line_number(source: &str, index: usize) -> usize {
    source.as_bytes()[..index.min(source.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

/// Validates a theme HLSL template at a rendering profile.
pub fn validate_hlsl(theme: &kiseki_schema::ResolvedTheme, template: &str) -> ShaderReport {
    let budget = Budget::for_profile(theme.profile);
    let program = compile_hlsl_for(theme, template);

    let mut report = ShaderReport::new(ShaderStage::Hlsl, theme.profile);
    report.bytes = program.source.len();

    // Structural contract checks run on the *template*, not the wrapper: the
    // wrapper cannot invent a missing `imageTexture` binding for WT.
    for (symbol, description) in REQUIRED_SYMBOLS {
        if !template.contains(symbol) {
            report.diagnostics.push(
                Diagnostic::error(
                    "shader.hlsl.missing-symbol",
                    format!("HLSL template is missing {description} (`{symbol}`)"),
                )
                .with_hint("copy the binding block from the theme template reference"),
            );
        }
    }

    if !template.contains("KISEKI_") {
        report.diagnostics.push(Diagnostic::warning(
            "shader.hlsl.hardcoded",
            "HLSL template references no KISEKI_* macro; colors and opacity may be hard-coded",
        ));
    }

    let (expanded, pp_diagnostics) = expand_macros_checked(&raw_hlsl(theme, template));
    report.diagnostics.extend(pp_diagnostics);

    let lint = lint_with_budget(&expanded, budget);
    report.lint = lint.clone();
    report.diagnostics.extend(lint.diagnostics.items.clone());

    // A preprocessor pass that leaves unknown identifiers behind is a strong
    // signal of a typo'd macro, which HLSL compilers report far less clearly.
    for diagnostic in lint_unresolved_macros(&expanded) {
        report.diagnostics.push(diagnostic);
    }

    for diagnostic in lint_hlsl_structure(&expanded) {
        report.diagnostics.push(diagnostic);
    }

    for diagnostic in lint_structure(&expanded) {
        report.diagnostics.push(diagnostic);
    }

    // No HLSL frontend exists in naga; a clean lint pass is the strongest
    // statement we can make, so record it explicitly.
    report.parsed = !report.has_fatal();
    report.validated = report.parsed;

    report
}

/// Expands object-like `#define NAME value` macros and evaluates `#if`/`#elif`.
///
/// Deliberately minimal: Kiseki templates only use object-like macros and
/// integer `#if`/`#elif` conditions, so unsupported expressions are flagged
/// explicitly by [`expand_macros_checked`] instead of silently dropping active
/// branches.
pub fn expand_macros(source: &str) -> String {
    expand_macros_checked(source).0
}

/// Like [`expand_macros`], also returning blocking diagnostics for any
/// conditional directive or expression that could not be evaluated safely.
pub(crate) fn expand_macros_checked(source: &str) -> (String, Vec<Diagnostic>) {
    use std::collections::BTreeMap;

    let mut macros: BTreeMap<String, String> = BTreeMap::new();
    let mut out = String::with_capacity(source.len());
    let mut diagnostics = Vec::new();
    // Each entry is `(parent_active, branch_active, any_branch_taken)`.
    let mut conditionals: Vec<(bool, bool, bool)> = Vec::new();
    let current_active = |stack: &Vec<(bool, bool, bool)>| {
        stack
            .last()
            .is_none_or(|(parent, branch, _)| *parent && *branch)
    };

    for (line_idx, line) in source.lines().enumerate() {
        let line_no = line_idx + 1;
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("#define ") {
            if current_active(&conditionals) {
                let rest = rest.trim();
                let mut parts = rest.splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or_default();
                let value = parts.next().unwrap_or("1").trim().to_string();
                if !name.contains('(') {
                    macros.insert(name.to_string(), value);
                }
            }
            out.push('\n');
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("#if ") {
            let parent = current_active(&conditionals);
            let branch = if parent {
                match eval_condition(rest.trim(), &macros) {
                    Ok(value) => value,
                    Err(reason) => {
                        diagnostics.push(Diagnostic::error(
                            "shader.preprocessor.unsupported",
                            format!("line {line_no}: unsupported `#if` expression `{}`: {reason}", rest.trim()),
                        ));
                        // Fail-safe for linting: keep checking the body rather than silently skipping it.
                        true
                    }
                }
            } else {
                false
            };
            conditionals.push((parent, branch, branch));
            out.push('\n');
            continue;
        }
        if let Some(name) = trimmed.strip_prefix("#ifdef ") {
            let parent = current_active(&conditionals);
            let branch = parent && macros.contains_key(name.trim());
            conditionals.push((parent, branch, branch));
            out.push('\n');
            continue;
        }
        if let Some(name) = trimmed.strip_prefix("#ifndef ") {
            let parent = current_active(&conditionals);
            let branch = parent && !macros.contains_key(name.trim());
            conditionals.push((parent, branch, branch));
            out.push('\n');
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#elif ") {
            if let Some((parent, _branch, taken)) = conditionals.pop() {
                let next_branch = if parent && !taken {
                    match eval_condition(rest.trim(), &macros) {
                        Ok(value) => value,
                        Err(reason) => {
                            diagnostics.push(Diagnostic::error(
                                "shader.preprocessor.unsupported",
                                format!("line {line_no}: unsupported `#elif` expression `{}`: {reason}", rest.trim()),
                            ));
                            true
                        }
                    }
                } else {
                    false
                };
                conditionals.push((parent, next_branch, taken || next_branch));
            }
            out.push('\n');
            continue;
        }
        if trimmed.starts_with("#elifdef ") || trimmed.starts_with("#elifndef ") {
            diagnostics.push(Diagnostic::error(
                "shader.preprocessor.unsupported",
                format!("line {line_no}: `{trimmed}` is not supported; use `#if` / `#elif` / `#else`"),
            ));
            if let Some((parent, _, taken)) = conditionals.pop() {
                conditionals.push((parent, parent && !taken, true));
            }
            out.push('\n');
            continue;
        }
        if trimmed.starts_with("#else") {
            if let Some((parent, _branch, taken)) = conditionals.pop() {
                conditionals.push((parent, parent && !taken, true));
            }
            out.push('\n');
            continue;
        }
        if trimmed.starts_with("#endif") {
            conditionals.pop();
            out.push('\n');
            continue;
        }

        if !current_active(&conditionals) {
            out.push('\n');
            continue;
        }

        out.push_str(&expand_identifiers(line, &macros));
        out.push('\n');
    }

    (out, diagnostics)
}

fn expand_identifiers(line: &str, macros: &std::collections::BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = index;
            while index < chars.len() && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let ident: String = chars[start..index].iter().collect();
            match macros.get(&ident) {
                Some(value) => out.push_str(value),
                None => out.push_str(&ident),
            }
        } else {
            out.push(ch);
            index += 1;
        }
    }
    out
}

fn eval_condition(
    condition: &str,
    macros: &std::collections::BTreeMap<String, String>,
) -> Result<bool, String> {
    let condition = strip_outer_parens(condition.trim());
    if condition.is_empty() {
        return Err("empty condition".to_string());
    }
    if let Some((lhs, rhs)) = split_top_level(condition, "||") {
        return Ok(eval_condition(lhs, macros)? || eval_condition(rhs, macros)?);
    }
    if let Some((lhs, rhs)) = split_top_level(condition, "&&") {
        return Ok(eval_condition(lhs, macros)? && eval_condition(rhs, macros)?);
    }
    if let Some(rest) = condition.strip_prefix('!') {
        if !rest.starts_with('=') {
            return Ok(!eval_condition(rest, macros)?);
        }
    }
    for (op, cmp) in [
        (">=", (|a: i64, b: i64| a >= b) as fn(i64, i64) -> bool),
        ("<=", |a, b| a <= b),
        ("==", |a, b| a == b),
        ("!=", |a, b| a != b),
        (">", |a, b| a > b),
        ("<", |a, b| a < b),
    ] {
        if let Some((lhs, rhs)) = split_top_level(condition, op) {
            let left = resolve_number(lhs.trim(), macros)?;
            let right = resolve_number(rhs.trim(), macros)?;
            return Ok(cmp(left, right));
        }
    }
    if let Some(rest) = condition.strip_prefix("defined") {
        let rest = rest.trim();
        let name = rest
            .strip_prefix('(')
            .and_then(|inner| inner.strip_suffix(')'))
            .unwrap_or(rest)
            .trim();
        if is_identifier(name) {
            return Ok(macros.contains_key(name));
        }
        return Err(format!("malformed defined({name})"));
    }
    Ok(resolve_number(condition, macros)? != 0)
}

fn strip_outer_parens(mut s: &str) -> &str {
    loop {
        let trimmed = s.trim();
        if !(trimmed.starts_with('(') && trimmed.ends_with(')')) {
            return trimmed;
        }
        let mut depth = 0usize;
        let mut wraps_all = true;
        for (idx, ch) in trimmed.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && idx + 1 < trimmed.len() {
                        wraps_all = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if wraps_all {
            s = &trimmed[1..trimmed.len() - 1];
        } else {
            return trimmed;
        }
    }
}

fn split_top_level<'a>(expr: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let bytes = expr.as_bytes();
    let op_bytes = op.as_bytes();
    let mut depth = 0usize;
    let mut i = 0usize;
    while i + op_bytes.len() <= bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && &bytes[i..i + op_bytes.len()] == op_bytes => {
                // Avoid splitting `>=` / `<=` / `!=` / `==` on a 1-byte `>` / `<`.
                if op.len() == 1 {
                    let prev = i.checked_sub(1).map(|p| bytes[p]);
                    let next = bytes.get(i + 1).copied();
                    if matches!(next, Some(b'=') | Some(b'>') | Some(b'<'))
                        || matches!(prev, Some(b'>') | Some(b'<'))
                    {
                        i += 1;
                        continue;
                    }
                }
                return Some((&expr[..i], &expr[i + op_bytes.len()..]));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn is_identifier(token: &str) -> bool {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn resolve_number(
    token: &str,
    macros: &std::collections::BTreeMap<String, String>,
) -> Result<i64, String> {
    let token = strip_outer_parens(token.trim());
    if let Ok(value) = token.parse::<i64>() {
        return Ok(value);
    }
    if is_identifier(token) {
        return match macros.get(token) {
            Some(value) => value
                .trim()
                .parse::<i64>()
                .map_err(|_| format!("macro `{token}` expands to non-integer `{value}`")),
            // C/GLSL/HLSL preprocessor evaluates undefined identifiers in `#if` to 0.
            None => Ok(0),
        };
    }
    Err(format!("unsupported token `{token}`"))
}

/// Flags `KISEKI_*` identifiers that survived preprocessing, which always means
/// the macro name is misspelled.
fn lint_unresolved_macros(source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let source = strip_comments_and_strings(source);
    for (index, line) in source.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let mut rest = line;
        while let Some(position) = rest.find("KISEKI_") {
            let tail = &rest[position + "KISEKI_".len()..];
            let name: String = tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            diagnostics.push(Diagnostic::error(
                "shader.hlsl.unresolved-macro",
                format!(
                    "line {}: `KISEKI_{name}` is not a defined macro; check the name against the generated header",
                    index + 1
                ),
            ));
            rest = &tail[name.len()..];
        }
    }
    diagnostics
}

/// Structural checks that a lexical linter cannot express.
fn lint_structure(source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if brace_balance(source) != 0 {
        diagnostics.push(Diagnostic::error(
            "shader.hlsl.unbalanced",
            "HLSL source has unbalanced braces",
        ));
    }
    if paren_balance(source) != 0 {
        diagnostics.push(Diagnostic::error(
            "shader.hlsl.unbalanced",
            "HLSL source has unbalanced parentheses",
        ));
    }
    if !source.contains("return") {
        diagnostics.push(Diagnostic::error(
            "shader.hlsl.no-return",
            "HLSL pixel shader must return a color",
        ));
    }
    // Writing to a sampler or texture is never valid and signals a confused
    // template.
    for (needle, description) in [
        ("shaderTexture =", "assignment to shaderTexture"),
        ("imageTexture =", "assignment to imageTexture"),
    ] {
        if source.contains(needle) {
            diagnostics.push(Diagnostic::error(
                "shader.hlsl.invalid-write",
                format!("HLSL source contains an {description}"),
            ));
        }
    }

    diagnostics
}

fn brace_balance(source: &str) -> i64 {
    let mut depth: i64 = 0;
    for ch in strip_comments_and_strings(source).chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn paren_balance(source: &str) -> i64 {
    let mut depth: i64 = 0;
    for ch in strip_comments_and_strings(source).chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Removes `//`, `/* */` comments and string literals so brace counting is not
/// fooled by braces inside them.
fn strip_comments_and_strings(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match (bytes[index], bytes.get(index + 1)) {
            (b'/', Some(b'/')) => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            (b'/', Some(b'*')) => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            (b'"', _) => {
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
                index = (index + 1).min(bytes.len());
            }
            (byte, _) => {
                out.push(byte as char);
                index += 1;
            }
        }
    }
    out
}

/// Exposed for tests: lint a raw HLSL source without a theme.
pub fn lint_hlsl_source(source: &str, budget: Budget) -> SourceLint {
    lint_with_budget(source, budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_types::RenderingProfile;

    const NERV_EVA_HLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.hlsl");

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
    fn nerv_eva_hlsl_validates_at_every_dynamic_profile() {
        for profile in [
            RenderingProfile::Eco,
            RenderingProfile::Balanced,
            RenderingProfile::Ultra,
        ] {
            let report = validate_hlsl(&theme(profile), NERV_EVA_HLSL);
            let errors: Vec<String> = report
                .diagnostics
                .iter()
                .filter(|d| d.is_fatal())
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect();
            assert!(errors.is_empty(), "{profile} -> {errors:#?}");
            assert!(report.is_ok());
        }
    }

    #[test]
    fn preprocessing_expands_every_macro() {
        let program = compile_hlsl_for(&theme(RenderingProfile::Balanced), NERV_EVA_HLSL);
        assert!(!program.source.contains("KISEKI_BASE_BG"));
        assert!(!program.source.contains("KISEKI_PARAM_SCAN_PERIOD_SEC"));
        assert!(program.source.contains("7.5000"), "scan period must be inlined");
        assert!(program.source.contains("float3(0.043137, 0.035294, 0.039216)") || program.source.contains("0.043137"));
    }

    #[test]
    fn conditional_blocks_respect_the_quality_level() {
        let eco = compile_hlsl_for(&theme(RenderingProfile::Eco), NERV_EVA_HLSL);
        let ultra = compile_hlsl_for(&theme(RenderingProfile::Ultra), NERV_EVA_HLSL);
        // The glitch block is `#if KISEKI_QUALITY_LEVEL >= 2`.
        assert!(!eco.source.contains("glitchCycle"), "eco must compile the glitch block out");
        assert!(ultra.source.contains("glitchCycle"));
    }

    #[test]
    fn macro_expansion_handles_conditionals() {
        let source = "#define LEVEL 1\n#if LEVEL >= 2\nhigh\n#else\nlow\n#endif\n";
        let expanded = expand_macros(source);
        assert!(expanded.contains("low"));
        assert!(!expanded.contains("\nhigh\n"));
    }

    #[test]
    fn reversed_smoothstep_is_rejected() {
        let template = NERV_EVA_HLSL.replace("smoothstep(0.81, 0.86, d)", "smoothstep(0.86, 0.81, d)");
        let report = validate_hlsl(&theme(RenderingProfile::Balanced), &template);
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.smoothstep.reversed"));
    }

    #[test]
    fn missing_required_symbols_are_reported() {
        let template = "float4 main(float4 p : SV_POSITION) : SV_TARGET { return p; }";
        let report = validate_hlsl(&theme(RenderingProfile::Balanced), template);
        assert!(report.has_fatal());
        let codes: Vec<&str> = report
            .diagnostics
            .iter()
            .filter(|d| d.is_fatal())
            .map(|d| d.code.as_str())
            .collect();
        assert!(codes.contains(&"shader.hlsl.missing-symbol"), "{codes:?}");
    }

    #[test]
    fn typo_d_macro_is_reported_as_unresolved() {
        let template = NERV_EVA_HLSL.replace("KISEKI_PARAM_HEX_CELL_PX", "KISEKI_PARAM_HEX_CELL_PXX");
        let report = validate_hlsl(&theme(RenderingProfile::Balanced), &template);
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.hlsl.unresolved-macro"));
    }

    #[test]
    fn unbalanced_braces_are_reported() {
        let template = format!("{NERV_EVA_HLSL}\n}}");
        let report = validate_hlsl(&theme(RenderingProfile::Balanced), &template);
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.hlsl.unbalanced"));
    }

    #[test]
    fn comment_braces_do_not_break_balance() {
        let source = "float4 main() : SV_TARGET { // }\n /* { */ return 0; }";
        assert_eq!(brace_balance(source), 0);
        assert_eq!(paren_balance(source), 0);
    }

    #[test]
    fn excessive_loop_budget_is_rejected_at_eco() {
        let template = NERV_EVA_HLSL.replace(
            "float narrow = kiseki_narrow_factor(texSize);",
            "float narrow = kiseki_narrow_factor(texSize);\n    for (int i = 0; i < 16; i++) { narrow += 0.001; }",
        );
        let report = validate_hlsl(&theme(RenderingProfile::Eco), &template);
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.budget.loop-iterations"));
    }

    #[test]
    fn compilation_is_deterministic() {
        let theme = theme(RenderingProfile::Ultra);
        assert_eq!(
            compile_hlsl_for(&theme, NERV_EVA_HLSL).source,
            compile_hlsl_for(&theme, NERV_EVA_HLSL).source
        );
    }

    #[test]
    fn bundled_hlsl_injects_the_helper_exactly_once() {
        let program = compile_hlsl_for(&theme(RenderingProfile::Balanced), NERV_EVA_HLSL);
        assert_eq!(
            program
                .source
                .matches("float kiseki_narrow_factor(float2 res)")
                .count(),
            1,
            "the helper must be injected once and never duplicated by the template"
        );
        assert!(program.source.contains("float3(0.043137, 0.035294, 0.039216)"));
        assert!(!program.source.contains("vec3("));
        assert!(lint_hlsl_structure(&program.source).is_empty());
    }

    #[test]
    fn a_duplicate_helper_definition_is_rejected() {
        let template = format!(
            "{NERV_EVA_HLSL}\nfloat kiseki_narrow_factor(float2 res) {{\n    return 1.0;\n}}\n"
        );
        let report = validate_hlsl(&theme(RenderingProfile::Balanced), &template);
        assert!(report.has_fatal());
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.hlsl.duplicate-function"));
    }

    #[test]
    fn valid_hlsl_overloads_are_not_duplicate_definitions() {
        let template = format!(
            "{NERV_EVA_HLSL}\nfloat kiseki_helper(float x) {{\n    return x;\n}}\nfloat kiseki_helper(float2 x) {{\n    return x.x;\n}}\n"
        );
        let report = validate_hlsl(&theme(RenderingProfile::Balanced), &template);
        assert!(!report.has_fatal(), "{:?}", report.diagnostics);
        assert!(!report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.hlsl.duplicate-function"));
    }

    #[test]
    fn a_helper_redefinition_with_another_parameter_name_is_rejected() {
        let template = format!(
            "{NERV_EVA_HLSL}\nfloat kiseki_narrow_factor(float2 other) {{\n    return 1.0;\n}}\n"
        );
        let report = validate_hlsl(&theme(RenderingProfile::Balanced), &template);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.hlsl.duplicate-function"));
    }

    #[test]
    fn glsl_vector_types_are_rejected_in_hlsl() {
        let template = NERV_EVA_HLSL.replace(
            "float3 animBg = lerp(KISEKI_BASE_BG, wall, bgOpacity);",
            "vec3 animBg = lerp(KISEKI_BASE_BG, wall, bgOpacity);",
        );
        let report = validate_hlsl(&theme(RenderingProfile::Balanced), &template);
        assert!(report.has_fatal());
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|d| d.code == "shader.hlsl.glsl-type")
            .expect("vec3 must be reported");
        assert!(diagnostic.message.contains("vec3"));
    }

    #[test]
    fn explicit_in_modifier_counts_as_duplicate_hlsl_definition() {
        let template = format!(
            "{NERV_EVA_HLSL}\nfloat kiseki_narrow_factor(in float2 other) {{\n    return 1.0;\n}}\n"
        );
        let report = validate_hlsl(&theme(RenderingProfile::Balanced), &template);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "shader.hlsl.duplicate-function"));
    }

    #[test]
    fn preprocessor_evaluates_elif_and_flags_unsupported_expressions() {
        let src = "#define KISEKI_QUALITY_LEVEL 2\n#if KISEKI_QUALITY_LEVEL >= 3\nint a = 3;\n#elif KISEKI_QUALITY_LEVEL == 2\nint a = 2;\n#else\nint a = 1;\n#endif\n";
        let (expanded, diagnostics) = expand_macros_checked(src);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(expanded.contains("int a = 2;"));
        assert!(!expanded.contains("int a = 3;"));
        assert!(!expanded.contains("int a = 1;"));

        let bad = "#define KISEKI_QUALITY_LEVEL 2\n#if KISEKI_QUALITY_LEVEL + 1 > 2\nint a = 1;\n#endif\n";
        let (_, bad_diags) = expand_macros_checked(bad);
        assert!(bad_diags
            .iter()
            .any(|d| d.code == "shader.preprocessor.unsupported"));
    }
}
