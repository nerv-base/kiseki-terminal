//! Static source analysis that complements `naga` (RFC-0004 §3, gates 2 & 3).
//!
//! `naga` proves a shader is well-formed; it cannot know that a *reversed*
//! `smoothstep` is undefined behaviour on real drivers, or that a loop will
//! hang a GPU. Those checks live here and run on the raw source before parsing.

use std::collections::{BTreeMap, BTreeSet};

use kiseki_types::{Diagnostic, DiagnosticReport};

use crate::budget::Budget;

/// Everything the linter learned about one source file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceLint {
    pub diagnostics: DiagnosticReport,
    /// Static texture sample operations found.
    pub texture_samples: usize,
    /// Deepest loop nesting observed.
    pub max_loop_depth: usize,
    /// Largest constant loop bound observed (`None` when no loops).
    pub max_loop_iterations: Option<u32>,
    /// Number of `smoothstep` calls checked.
    pub smoothstep_calls: usize,
}

impl SourceLint {
    pub fn has_fatal(&self) -> bool {
        self.diagnostics.has_fatal()
    }
}

/// A numeric constant expression evaluator, sufficient for `smoothstep` edges.
///
/// Supports decimal/hex literals, `+ - * /`, parentheses, unary minus, macro
/// substitution and GLSL/HLSL float suffixes.
#[derive(Debug, Clone, Default)]
struct ConstEval {
    macros: BTreeMap<String, f64>,
}

impl ConstEval {
    fn new(macros: BTreeMap<String, f64>) -> Self {
        Self { macros }
    }

    /// Evaluates `expr`, returning `None` when it is not a constant expression.
    fn eval(&self, expr: &str) -> Option<f64> {
        let tokens = tokenize(expr)?;
        let mut parser = Parser {
            tokens: &tokens,
            pos: 0,
            macros: &self.macros,
        };
        let value = parser.parse_expr()?;
        if parser.pos == tokens.len() {
            Some(value)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Open,
    Close,
}

fn tokenize(expr: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = expr.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            c if c.is_whitespace() => index += 1,
            '+' => {
                tokens.push(Token::Plus);
                index += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                index += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                index += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                index += 1;
            }
            '(' => {
                tokens.push(Token::Open);
                index += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                index += 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = index;
                while index < chars.len()
                    && (chars[index].is_ascii_digit() || chars[index] == '.' || chars[index] == 'e' || chars[index] == 'E')
                {
                    index += 1;
                }
                let literal: String = chars[start..index].iter().collect();
                // Skip a trailing float suffix (0.5f, 0.5F, 0.5lf).
                while index < chars.len() && matches!(chars[index], 'f' | 'F' | 'l' | 'L' | 'u' | 'U') {
                    index += 1;
                }
                tokens.push(Token::Number(literal.parse().ok()?));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = index;
                while index < chars.len() && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
                    index += 1;
                }
                tokens.push(Token::Ident(chars[start..index].iter().collect()));
            }
            _ => return None,
        }
    }
    Some(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    macros: &'a BTreeMap<String, f64>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn parse_expr(&mut self) -> Option<f64> {
        let mut value = self.parse_term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.pos += 1;
                    value += self.parse_term()?;
                }
                Some(Token::Minus) => {
                    self.pos += 1;
                    value -= self.parse_term()?;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_term(&mut self) -> Option<f64> {
        let mut value = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.pos += 1;
                    value *= self.parse_unary()?;
                }
                Some(Token::Slash) => {
                    self.pos += 1;
                    let divisor = self.parse_unary()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    value /= divisor;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(Token::Minus) => {
                self.pos += 1;
                Some(-self.parse_unary()?)
            }
            Some(Token::Plus) => {
                self.pos += 1;
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Option<f64> {
        match self.peek()?.clone() {
            Token::Number(value) => {
                self.pos += 1;
                Some(value)
            }
            Token::Open => {
                self.pos += 1;
                let value = self.parse_expr()?;
                if self.peek() != Some(&Token::Close) {
                    return None;
                }
                self.pos += 1;
                Some(value)
            }
            Token::Ident(name) => {
                self.pos += 1;
                self.macros.get(&name).copied()
            }
            _ => None,
        }
    }
}

/// Extracts `#define NAME <number>` macros so header-injected constants can be
/// folded at lint time.
fn collect_numeric_macros(source: &str) -> BTreeMap<String, f64> {
    let mut macros = BTreeMap::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("#define ") else {
            continue;
        };
        let rest = rest.trim();
        let mut parts = rest.splitn(2, char::is_whitespace);
        let (Some(name), Some(value)) = (parts.next(), parts.next()) else {
            continue;
        };
        if name.contains('(') {
            continue;
        }
        if let Ok(number) = value.trim().parse::<f64>() {
            macros.insert(name.to_string(), number);
        }
    }
    macros
}

/// Runs every source-level gate.
pub fn lint_source(source: &str) -> SourceLint {
    // Comments and string literals must not be scanned for `/` or `smoothstep`.
    let cleaned = strip_comments_and_strings(source);
    let source = cleaned.as_str();
    let macros = collect_numeric_macros(source);
    let eval = ConstEval::new(macros);
    let mut lint = SourceLint::default();

    lint.smoothstep_calls = check_smoothstep(source, &eval, &mut lint);
    check_divisions(source, &eval, &mut lint);
    check_loops(source, &mut lint);
    check_unsafe_directives(source, &mut lint);
    lint.texture_samples = count_texture_samples(source);

    lint
}

/// Runs the gates plus the TDR complexity budget for a profile.
pub fn lint_with_budget(source: &str, budget: Budget) -> SourceLint {
    let mut lint = lint_source(source);

    if source.len() > budget.max_source_bytes {
        lint.diagnostics.push(Diagnostic::error(
            "shader.budget.source-size",
            format!(
                "shader source is {} bytes, exceeding the {} byte budget",
                source.len(),
                budget.max_source_bytes
            ),
        ));
    }
    if lint.texture_samples > budget.max_texture_samples {
        lint.diagnostics.push(Diagnostic::error(
            "shader.budget.texture-samples",
            format!(
                "{} texture samples exceeds the budget of {}",
                lint.texture_samples, budget.max_texture_samples
            ),
        ));
    }
    if lint.max_loop_depth > budget.max_loop_depth {
        lint.diagnostics.push(Diagnostic::error(
            "shader.budget.loop-depth",
            format!(
                "loop nesting depth {} exceeds the budget of {}",
                lint.max_loop_depth, budget.max_loop_depth
            ),
        ));
    }
    if let Some(iterations) = lint.max_loop_iterations {
        if iterations > budget.max_loop_iterations {
            lint.diagnostics.push(Diagnostic::error(
                "shader.budget.loop-iterations",
                format!(
                    "constant loop bound {iterations} exceeds the budget of {}",
                    budget.max_loop_iterations
                ),
            ));
        }
    }
    lint
}

fn check_smoothstep(source: &str, eval: &ConstEval, lint: &mut SourceLint) -> usize {
    let mut count = 0;
    for (index, _) in source.match_indices("smoothstep") {
        // Reject `my_smoothstep(` style matches.
        if source[..index].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let Some(open) = source[index..].find('(') else {
            continue;
        };
        let args_start = index + open + 1;
        let Some(args) = split_call_arguments(source, args_start) else {
            continue;
        };
        count += 1;
        if args.len() < 3 {
            lint.diagnostics.push(Diagnostic::error(
                "shader.smoothstep.arity",
                "smoothstep requires exactly 3 arguments",
            ));
            continue;
        }
        let (Some(low), Some(high)) = (eval.eval(&args[0]), eval.eval(&args[1])) else {
            // Non-constant edges cannot be proven reversed; that is fine.
            continue;
        };
        if low >= high {
            let line = line_of(source, index);
            lint.diagnostics.push(
                Diagnostic::error(
                    "shader.smoothstep.reversed",
                    format!(
                        "reversed smoothstep at line {line}: smoothstep({low}, {high}, x) has edge0 >= edge1, which is undefined behaviour on some GPU drivers"
                    ),
                )
                .with_hint(format!(
                    "rewrite as 1.0 - smoothstep({high}, {low}, x) so the edges are ordered low -> high"
                )),
            );
        }
    }
    count
}

fn check_divisions(source: &str, eval: &ConstEval, lint: &mut SourceLint) {
    let (guarded, risky) = classify_identifiers(source);

    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'/' {
            index += 1;
            continue;
        }
        // Skip comment openers and `/=`.
        if matches!(bytes.get(index + 1), Some(b'/') | Some(b'*') | Some(b'=')) {
            index += 1;
            continue;
        }
        let mut cursor = index + 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let rest = &source[cursor..];
        let divisor = read_divisor(rest);
        if divisor.is_empty() {
            index += 1;
            continue;
        }

        if !is_guarded_divisor(&divisor, eval, &guarded, &risky) {
            let line = line_of(source, index);
            lint.diagnostics.push(
                Diagnostic::error(
                    "shader.division.unguarded",
                    format!(
                        "potential divide-by-zero at line {line}: `/ {divisor}` is not guarded"
                    ),
                )
                .with_hint(format!(
                    "guard the divisor, e.g. `/ max({divisor}, 0.001)`"
                )),
            );
        }
        index = cursor;
    }
}

/// Reads the full divisor expression starting at `rest`.
///
/// Consumes a balanced primary expression including `*`, `/` and unary sign,
/// so `/ (a * b)` and `/ KISEKI_PARAM * scale` are both judged as a whole.
fn read_divisor(rest: &str) -> String {
    let chars: Vec<char> = rest.chars().collect();
    let mut index = 0;
    let mut out = String::new();
    let mut depth = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '(' => {
                depth += 1;
                out.push(ch);
                index += 1;
            }
            ')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                out.push(ch);
                index += 1;
                if depth == 0 {
                    // A closed group may be followed by `* x` / `/ x`.
                    let (extra, next) = read_operator_tail(&chars, index);
                    out.push_str(&extra);
                    index = next;
                }
            }
            c if c.is_ascii_alphanumeric() || c == '_' => {
                while index < chars.len() && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
                    out.push(chars[index]);
                    index += 1;
                }
            }
            '.' => {
                out.push('.');
                index += 1;
            }
            '*' | '/' => {
                // Only a multiply/divide tail is part of the divisor; a binary
                // `+`/`-` starts the next term.
                out.push(ch);
                index += 1;
                while index < chars.len() && chars[index].is_ascii_whitespace() {
                    index += 1;
                }
            }
            c if c.is_whitespace() => {
                out.push(' ');
                index += 1;
            }
            // Anything still inside a parenthesised group belongs to the
            // divisor: `max(Scale, 1.0)` must not be truncated at the comma.
            _ if depth > 0 => {
                out.push(ch);
                index += 1;
            }
            _ => break,
        }
    }

    out.trim().to_string()
}

fn read_operator_tail(chars: &[char], from: usize) -> (String, usize) {
    let mut index = from;
    let mut out = String::new();
    while index < chars.len() && chars[index].is_ascii_whitespace() {
        index += 1;
    }
    if !matches!(chars.get(index), Some('*') | Some('/')) {
        return (out, from);
    }
    let mut depth = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '(' => {
                depth += 1;
                out.push(ch);
            }
            ')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                out.push(ch);
            }
            c if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '*' || c == '/' || c.is_whitespace() => {
                out.push(c);
            }
            _ => break,
        }
        index += 1;
    }
    (out.trim_end().to_string(), index)
}

/// True when a divisor expression is provably safe to divide by.
///
/// The RFC's rule is specifically about dividing by a raw texture channel
/// (`term.rgb / term.a`), where the channel can be exactly zero. The check is
/// therefore conservative about *derived* arithmetic and strict about member
/// access on a value that was never guarded.
fn is_guarded_divisor(divisor: &str, eval: &ConstEval, guarded: &BTreeSet<String>, risky: &BTreeSet<String>) -> bool {
    let divisor = divisor.trim();
    if divisor.is_empty() {
        return false;
    }
    if eval.eval(divisor).is_some_and(|value| value.abs() > f64::EPSILON) {
        return true;
    }
    // A parenthesised group is judged by its contents: `(a * b)` is exactly as
    // safe as `a * b`, and treating the parentheses as opaque produced false
    // positives for `/ (KISEKI_PARAM_X * max(Scale, 1.0))`.
    let inner = strip_outer_parens(divisor);
    if eval.eval(inner).is_some_and(|value| value.abs() > f64::EPSILON) {
        return true;
    }
    // Arithmetic at the top level (`a * b`, `a / b`) is a computed scalar, not
    // a raw channel.
    if split_multiplicative(inner).len() > 1 {
        return true;
    }
    let head = inner.trim();
    for safe in ["max(", "max (", "abs(", "saturate(", "length(", "sqrt("] {
        if head.starts_with(safe) {
            return true;
        }
    }
    if let Some((base, _)) = head.split_once('.') {
        // `term.a` / `s.a`: a raw channel, safe only when the base was guarded.
        return guarded.contains(base);
    }
    if guarded.contains(head) {
        return true;
    }
    // A bare identifier derived from a risky channel stays risky.
    !risky.contains(head)
}

/// Removes one pair of parentheses when they enclose the whole expression.
fn strip_outer_parens(expr: &str) -> &str {
    let trimmed = expr.trim();
    let bytes = trimmed.as_bytes();
    if bytes.first() != Some(&b'(') || bytes.last() != Some(&b')') {
        return trimmed;
    }
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && index != bytes.len() - 1 {
                    // The opening paren closes before the end: not a wrapper.
                    return trimmed;
                }
            }
            _ => {}
        }
    }
    if depth == 0 {
        trimmed[1..trimmed.len() - 1].trim()
    } else {
        trimmed
    }
}

/// Splits an expression on top-level `*` and `/` operators.
fn split_multiplicative(expr: &str) -> Vec<String> {
    let chars: Vec<char> = expr.chars().collect();
    let mut factors = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in chars {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            '*' | '/' if depth == 0 => {
                factors.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    factors.push(current.trim().to_string());
    factors.retain(|factor| !factor.is_empty());
    factors
}

/// Classifies identifiers into `guarded` (provably safe) and `risky`
/// (a raw channel that may be zero), following assignments transitively.
fn classify_identifiers(source: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut assignments: Vec<(String, String)> = Vec::new();
    for line in source.lines() {
        let Some((lhs, rhs)) = line.split_once('=') else {
            continue;
        };
        if rhs.trim_start().starts_with('=') || lhs.contains("==") {
            continue;
        }
        let Some(name) = lhs
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .rev()
            .find(|token| !token.is_empty())
        else {
            continue;
        };
        if matches!(
            name,
            "const" | "static" | "float" | "int" | "vec2" | "vec3" | "vec4" | "float2" | "float3" | "float4"
        ) {
            continue;
        }
        assignments.push((name.to_string(), rhs.trim().to_string()));
    }

    let mut guarded: BTreeSet<String> = BTreeSet::new();
    let mut risky: BTreeSet<String> = BTreeSet::new();
    for _ in 0..8 {
        let before = (guarded.len(), risky.len());
        for (name, rhs) in &assignments {
            let head = rhs.trim_start_matches('(').trim_end_matches(')').trim();
            let guarded_rhs = rhs.parse::<f64>().is_ok_and(|value| value.abs() > f64::EPSILON)
                || ["max(", "max (", "abs(", "saturate(", "length(", "sqrt("]
                    .iter()
                    .any(|prefix| head.starts_with(prefix))
                || split_multiplicative(rhs).len() > 1;
            if guarded_rhs {
                guarded.insert(name.clone());
                risky.remove(name);
                continue;
            }
            if let Some((base, _)) = head.split_once('.') {
                if !guarded.contains(base) {
                    risky.insert(name.clone());
                }
            } else if risky.contains(head) {
                risky.insert(name.clone());
            }
        }
        if (guarded.len(), risky.len()) == before {
            break;
        }
    }
    (guarded, risky)
}

/// Loop keyword recognised by the linter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopKind {
    For,
    While,
    Do,
}

/// The comparison direction of a `for` condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
}

/// The constant step of a `for` header.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Up(f64),
    Down(f64),
}

fn loop_keyword_at(bytes: &[u8], index: usize) -> Option<(LoopKind, usize)> {
    if !boundary_before(bytes, index) {
        return None;
    }
    for (text, kind) in [
        ("for", LoopKind::For),
        ("while", LoopKind::While),
        ("do", LoopKind::Do),
    ] {
        if bytes[index..].starts_with(text.as_bytes()) {
            let after = index + text.len();
            if bytes
                .get(after)
                .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'(' | b'{'))
            {
                return Some((kind, text.len()));
            }
        }
    }
    None
}

fn check_loops(source: &str, lint: &mut SourceLint) {
    let eval = ConstEval::new(collect_numeric_macros(source));
    let mut max_depth = 0usize;
    let mut max_iterations: Option<u32> = None;
    let mut index = 0;
    let bytes = source.as_bytes();
    while index < bytes.len() {
        let Some((kind, len)) = loop_keyword_at(bytes, index) else {
            index += 1;
            continue;
        };
        let after = index + len;
        let line = line_of(source, index);

        if kind == LoopKind::Do {
            // `do { ... } while (cond);` can never be proven finite, so it is
            // rejected outright; the trailing `while` is consumed with it.
            lint.diagnostics.push(
                Diagnostic::error(
                    "shader.loop.unbounded",
                    format!("`do` loop at line {line} is not allowed: only compile-time bounded `for` loops may run on the GPU"),
                )
                .with_hint("rewrite as `for (int i = 0; i < N; i++)` with a constant N"),
            );
            let body_start = source[after..]
                .find('{')
                .map(|offset| after + offset)
                .unwrap_or(after);
            let (nested_depth, nested_iterations) = scan_nested(source, body_start);
            max_depth = max_depth.max(1 + nested_depth);
            if let Some(iterations) = nested_iterations {
                max_iterations = Some(max_iterations.map_or(iterations, |current| current.max(iterations)));
            }
            index = body_start.max(index + len);
            continue;
        }

        // Find the closing paren of the loop header.
        let Some(open) = source[after..].find('(') else {
            index += 1;
            continue;
        };
        let args_start = after + open + 1;
        let Some(head) = split_for_header(source, args_start) else {
            index += 1;
            continue;
        };

        if kind == LoopKind::While {
            let condition = head.first().map(|clause| clause.trim().to_string()).unwrap_or_default();
            lint.diagnostics.push(
                Diagnostic::error(
                    "shader.loop.unbounded",
                    format!(
                        "`while ({condition})` at line {line} is not allowed: only compile-time bounded `for` loops may run on the GPU"
                    ),
                )
                .with_hint("use a `for` loop with a compile-time constant bound"),
            );
        } else {
            match for_trip_count(&head, &eval) {
                Ok(iterations) => {
                    max_iterations = Some(max_iterations.map_or(iterations, |current| current.max(iterations)));
                }
                Err(reason) => {
                    let condition = head
                        .get(1)
                        .map(|clause| clause.trim().to_string())
                        .unwrap_or_default();
                    lint.diagnostics.push(
                        Diagnostic::error(
                            "shader.loop.unbounded",
                            format!("`for` loop at line {line} cannot be proven finite: {reason} (condition `{condition}`)"),
                        )
                        .with_hint(
                            "write `for (int i = 0; i < N; i++)` with a literal or KISEKI_* macro bound and a matching step direction",
                        ),
                    );
                }
            }
        }

        // Walk the body to find nested loops.
        let body_start = find_body_start(source, args_start);
        let (nested_depth, nested_iterations) = scan_nested(source, body_start);
        max_depth = max_depth.max(1 + nested_depth);
        if let Some(iterations) = nested_iterations {
            max_iterations = Some(max_iterations.map_or(iterations, |current| current.max(iterations)));
        }
        index = body_start.max(index + len);
    }

    lint.max_loop_depth = max_depth;
    lint.max_loop_iterations = max_iterations;
}

/// Extracts the variable name from one side of a `for` header clause.
fn header_identifier(clause: &str) -> Option<String> {
    clause
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .rfind(|token| !token.is_empty() && !token.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

fn parse_init(clause: &str, eval: &ConstEval) -> Option<(String, f64)> {
    let (lhs, rhs) = clause.split_once('=')?;
    // `==` / `<=` / `>=` are comparisons, not assignments.
    if rhs.trim_start().starts_with('=') {
        return None;
    }
    let variable = header_identifier(lhs)?;
    let start = eval.eval(rhs)?;
    Some((variable, start))
}

fn parse_condition(clause: &str) -> Option<(String, CmpOp, &str)> {
    // Compound conditions cannot be proven with a single constant bound.
    if clause.contains("&&") || clause.contains("||") {
        return None;
    }
    let (operator, op) = if let Some(rest) = clause.split_once("<=") {
        (rest, CmpOp::Le)
    } else if let Some(rest) = clause.split_once(">=") {
        (rest, CmpOp::Ge)
    } else if let Some(rest) = clause.split_once('<') {
        (rest, CmpOp::Lt)
    } else {
        let rest = clause.split_once('>')?;
        (rest, CmpOp::Gt)
    };
    let lhs = operator.0.trim();
    // The whole left-hand side must be the induction variable itself: `i`,
    // not `i * 0` or `i + 1`. Otherwise the trip count is not provable.
    let variable = header_identifier(lhs)?;
    if lhs != variable {
        return None;
    }
    Some((variable, op, operator.1))
}

fn parse_step(clause: &str, variable: &str, eval: &ConstEval) -> Option<Step> {
    let clause = clause.trim();
    for (suffix, step) in [("++", Step::Up(1.0)), ("--", Step::Down(1.0))] {
        if clause == format!("{variable}{suffix}") || clause == format!("{suffix}{variable}") {
            return Some(step);
        }
    }
    for (operator, up) in [("+=", true), ("-=", false)] {
        let prefix = format!("{variable} {operator}");
        let compact = format!("{variable}{operator}");
        let Some(rest) = clause
            .strip_prefix(&prefix)
            .or_else(|| clause.strip_prefix(&compact))
        else {
            continue;
        };
        let magnitude = eval.eval(rest)?;
        if magnitude <= 0.0 {
            return None;
        }
        return Some(if up {
            Step::Up(magnitude)
        } else {
            Step::Down(magnitude)
        });
    }
    None
}

/// Computes the exact trip count of a `for (init; cond; step)` header.
fn for_trip_count(header: &[String], eval: &ConstEval) -> std::result::Result<u32, String> {
    if header.len() != 3 {
        return Err("the header must have init, condition and step clauses".to_string());
    }
    let (variable, start) =
        parse_init(&header[0], eval).ok_or_else(|| "the initialiser is not a constant assignment".to_string())?;
    let (condition_variable, op, bound_raw) =
        parse_condition(&header[1]).ok_or_else(|| "the condition is not a constant comparison".to_string())?;
    if condition_variable != variable {
        return Err(format!(
            "the condition tests `{condition_variable}` but the initialiser defines `{variable}`"
        ));
    }
    let bound = eval
        .eval(bound_raw)
        .ok_or_else(|| "the comparison bound is not a constant".to_string())?;
    let step = parse_step(&header[2], &variable, eval)
        .ok_or_else(|| "the step is not a constant `++`, `--`, `+= k` or `-= k`".to_string())?;

    let (magnitude, ascending) = match step {
        Step::Up(magnitude) => (magnitude, true),
        Step::Down(magnitude) => (magnitude, false),
    };
    if magnitude <= 0.0 || !magnitude.is_finite() {
        return Err("the step must be a non-zero constant".to_string());
    }
    let ascending_condition = matches!(op, CmpOp::Lt | CmpOp::Le);
    if ascending != ascending_condition {
        return Err(format!(
            "the step moves {} while the condition uses `{}`",
            if ascending { "up" } else { "down" },
            header[1].trim()
        ));
    }

    let distance = if ascending { bound - start } else { start - bound };
    let trip = match op {
        CmpOp::Lt | CmpOp::Gt => {
            if distance <= 0.0 {
                0.0
            } else {
                (distance / magnitude).ceil()
            }
        }
        CmpOp::Le | CmpOp::Ge => {
            if distance < 0.0 {
                0.0
            } else {
                (distance / magnitude).floor() + 1.0
            }
        }
    };
    if !trip.is_finite() || trip < 0.0 || trip > u32::MAX as f64 {
        return Err("the trip count cannot be represented".to_string());
    }
    Ok(trip as u32)
}

/// Scans a loop body for nested loops, returning `(extra_depth, max_iterations)`.
fn scan_nested(source: &str, from: usize) -> (usize, Option<u32>) {
    let Some(body) = extract_balanced_block(source, from) else {
        return (0, None);
    };
    let mut nested = SourceLint::default();
    check_loops(&body, &mut nested);
    (nested.max_loop_depth, nested.max_loop_iterations)
}

/// Extracts the `{ ... }` block that begins at or after `from`.
fn extract_balanced_block(source: &str, from: usize) -> Option<String> {
    let bytes = source.as_bytes();
    let start = source[from..].find('{')? + from;
    let mut depth = 0usize;
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(source[start..=index].to_string());
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn find_body_start(source: &str, args_start: usize) -> usize {
    // Skip past the header's closing paren, then find the next `{`.
    let bytes = source.as_bytes();
    let mut depth = 1usize;
    let mut index = args_start;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return source[index..].find('{').map(|offset| index + offset).unwrap_or(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    args_start
}

fn check_unsafe_directives(source: &str, lint: &mut SourceLint) {
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("#extension") {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        for forbidden in [
            "gl_compute_shader",
            "gl_shader_storage_buffer_object",
            "gl_shader_atomic",
            "gl_image_load_store",
            "gl_arb_shader_storage",
            "gl_nv_shader_atomic",
        ] {
            if lower.contains(forbidden) {
                lint.diagnostics.push(Diagnostic::error(
                    "shader.extension.forbidden",
                    format!(
                        "line {}: `{trimmed}` enables compute/storage/atomic features that are forbidden in a terminal post-processing shader",
                        index + 1
                    ),
                ));
            }
        }
    }
}

fn count_texture_samples(source: &str) -> usize {
    let mut count = 0;
    for needle in ["texture(", "textureLod(", "textureGrad(", "textureProj(", ".Sample(", ".SampleLevel(", ".Load("] {
        count += source.matches(needle).count();
    }
    count
}

/// Splits a `for (...)` header on its `;` separators.
///
/// The three clauses are init/condition/step; commas inside a clause (such as
/// `int i = 0, j = 0`) stay with their clause.
fn split_for_header(source: &str, start: usize) -> Option<Vec<String>> {
    let bytes = source.as_bytes();
    let mut depth = 1usize;
    let mut index = start;
    let mut current = String::new();
    let mut clauses = Vec::new();
    while index < bytes.len() {
        let ch = bytes[index] as char;
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    clauses.push(current.trim().to_string());
                    return Some(clauses);
                }
                current.push(ch);
            }
            ';' if depth == 1 => {
                clauses.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
        index += 1;
    }
    None
}

/// Splits call arguments starting just after `(`, returning `None` when the
/// parentheses are unbalanced.
fn split_call_arguments(source: &str, start: usize) -> Option<Vec<String>> {
    let bytes = source.as_bytes();
    let mut depth = 1usize;
    let mut index = start;
    let mut current = String::new();
    let mut args = Vec::new();
    while index < bytes.len() {
        let ch = bytes[index] as char;
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    args.push(current.trim().to_string());
                    return Some(args);
                }
                current.push(ch);
            }
            ',' if depth == 1 => {
                args.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
        index += 1;
    }
    None
}

/// Replaces `//`, `/* */` comments and string literals with spaces, keeping
/// newlines so reported line numbers stay accurate.
fn strip_comments_and_strings(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match (bytes[index], bytes.get(index + 1)) {
            (b'/', Some(b'/')) => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    out.push(b' ');
                    index += 1;
                }
            }
            (b'/', Some(b'*')) => {
                out.extend_from_slice(b"  ");
                index += 2;
                while index < bytes.len() {
                    if bytes[index] == b'\n' {
                        out.push(b'\n');
                    } else {
                        out.push(b' ');
                    }
                    if index + 1 < bytes.len() && bytes[index] == b'*' && bytes[index + 1] == b'/' {
                        out.extend_from_slice(b"  ");
                        index += 2;
                        break;
                    }
                    index += 1;
                }
            }
            (b'"', _) => {
                out.push(b' ');
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\n' {
                        out.push(b'\n');
                    } else {
                        out.push(b' ');
                    }
                    if bytes[index] == b'\\' {
                        out.push(b' ');
                        index += 1;
                    }
                    index += 1;
                }
                if index < bytes.len() {
                    out.push(b' ');
                    index += 1;
                }
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    // The input was valid UTF-8 and we only ever replaced ASCII bytes, so the
    // cleaned buffer is valid UTF-8 as well.
    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

fn boundary_before(bytes: &[u8], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let previous = bytes[index - 1];
    !(previous.is_ascii_alphanumeric() || previous == b'_')
}

fn line_of(source: &str, index: usize) -> usize {
    let limit = index.min(source.len());
    source.as_bytes()[..limit].iter().filter(|b| **b == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn const_eval_handles_arithmetic_and_macros() {
        let mut macros = BTreeMap::new();
        macros.insert("KISEKI_SAFE_ZONE_END_X".to_string(), 0.56);
        let eval = ConstEval::new(macros);
        assert_eq!(eval.eval("0.81"), Some(0.81));
        assert_eq!(eval.eval("KISEKI_SAFE_ZONE_END_X - 0.02"), Some(0.54));
        assert_eq!(eval.eval("(1.0 + 1.0) * 2.0"), Some(4.0));
        assert_eq!(eval.eval("-0.5"), Some(-0.5));
        assert_eq!(eval.eval("uv.x"), None);
        assert_eq!(eval.eval("0.5f"), Some(0.5));
    }

    #[test]
    fn detects_reversed_smoothstep() {
        let lint = lint_source("float a = smoothstep(0.86, 0.81, d);");
        assert!(lint.has_fatal());
        let diagnostic = &lint.diagnostics.items[0];
        assert_eq!(diagnostic.code, "shader.smoothstep.reversed");
        assert!(diagnostic.hint.as_ref().unwrap().contains("1.0 - smoothstep"));
    }

    #[test]
    fn detects_equal_smoothstep_edges() {
        let lint = lint_source("float a = smoothstep(0.5, 0.5, d);");
        assert!(lint.has_fatal());
    }

    #[test]
    fn detects_reversed_smoothstep_through_macros() {
        let lint = lint_source(
            "#define KISEKI_NARROW_FADE_ASPECT 0.5\nfloat a = smoothstep(0.65, KISEKI_NARROW_FADE_ASPECT, aspect);",
        );
        assert!(lint.has_fatal());
        assert_eq!(lint.diagnostics.items[0].code, "shader.smoothstep.reversed");
    }

    #[test]
    fn accepts_ordered_smoothstep_and_dynamic_edges() {
        for source in [
            "float a = smoothstep(0.0, 1.0, x);",
            "float a = smoothstep(0.81, 0.86, d);",
            "float a = smoothstep(lo, hi, x);",
            "float a = my_smoothstep(1.0, 0.0, x);",
        ] {
            let lint = lint_source(source);
            assert!(
                lint.diagnostics.items.iter().all(|d| d.code != "shader.smoothstep.reversed"),
                "{source} -> {:?}",
                lint.diagnostics
            );
        }
    }

    #[test]
    fn detects_unguarded_division() {
        let lint = lint_source("vec3 c = term.rgb / term.a;");
        assert!(lint.has_fatal());
        assert_eq!(lint.diagnostics.items[0].code, "shader.division.unguarded");
        assert!(lint.diagnostics.items[0].hint.as_ref().unwrap().contains("max(term.a, 0.001)"));
    }

    #[test]
    fn accepts_guarded_and_constant_divisions() {
        for source in [
            "vec3 c = term.rgb / max(term.a, 0.001);",
            "float x = 1.0 / 2.0;",
            "#define PERIOD 7.5\nfloat y = iTime / PERIOD;",
            "float visH = imgAspect / max(screenAspect, 0.0001);",
            "// a comment / with a slash\n",
        ] {
            let lint = lint_source(source);
            assert!(
                lint.diagnostics.items.iter().all(|d| d.code != "shader.division.unguarded"),
                "{source} -> {:?}",
                lint.diagnostics
            );
        }
    }

    #[test]
    fn accepts_division_by_locally_guarded_variable() {
        let source = "vec2 res = max(iResolution.xy, vec2(1.0));\nvec2 uv = fragCoord / res;";
        let lint = lint_source(source);
        assert!(!lint.has_fatal(), "{:?}", lint.diagnostics);
    }

    #[test]
    fn flags_while_true() {
        let lint = lint_source("void f() { while (true) { x += 1.0; } }");
        assert!(lint.has_fatal());
        assert!(lint
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "shader.loop.unbounded"));
    }

    #[test]
    fn rejects_every_while_loop_even_with_a_constant_condition() {
        for source in [
            "void f() { while (x < 5) { x += 1.0; } }",
            "void f() { while (i <= 8) { i++; } }",
            "void f() { do { x += 1.0; } while (x < 5); }",
        ] {
            let lint = lint_source(source);
            assert!(lint.has_fatal(), "{source} must be rejected");
            assert!(
                lint.diagnostics
                    .items
                    .iter()
                    .any(|d| d.code == "shader.loop.unbounded"),
                "{source} -> {:?}",
                lint.diagnostics
            );
        }
    }

    #[test]
    fn rejects_a_condition_that_is_not_the_bare_induction_variable() {
        for source in [
            "void f() { for (int i = 0; i * 0 < 8; i++) { x += 1.0; } }",
            "void f() { for (int i = 0; i + 1 < 8; i++) { x += 1.0; } }",
            "void f() { for (int i = 0; i - 1 < 8; i++) { x += 1.0; } }",
            "void f() { for (int i = 0; i * 2 <= 8; i++) { x += 1.0; } }",
        ] {
            let lint = lint_source(source);
            assert!(lint.has_fatal(), "{source} must be rejected");
            assert!(
                lint.diagnostics
                    .items
                    .iter()
                    .any(|d| d.code == "shader.loop.unbounded"),
                "{source} -> {:?}",
                lint.diagnostics
            );
        }
    }

    #[test]
    fn accepts_countdown_loops_with_a_matching_step() {
        let lint = lint_source("for (int i = 1000; i > 0; i--) { x += 1.0; }");
        assert!(!lint.has_fatal(), "{:?}", lint.diagnostics);
        assert_eq!(lint.max_loop_iterations, Some(1000));
        // The profile budget still applies on top of the proven trip count.
        let lint = lint_with_budget("for (int i = 1000; i > 0; i--) { x += 1.0; }", Budget::HIGH);
        assert!(lint
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "shader.budget.loop-iterations"));
    }

    #[test]
    fn rejects_a_step_that_moves_away_from_the_bound() {
        for source in [
            "for (int i = 0; i < 8; i--) { x += 1.0; }",
            "for (int i = 8; i > 0; i++) { x += 1.0; }",
            "for (int i = 0; i < 8; i += -1) { x += 1.0; }",
        ] {
            let lint = lint_source(source);
            assert!(lint.has_fatal(), "{source} must be rejected");
            assert!(
                lint.diagnostics
                    .items
                    .iter()
                    .any(|d| d.code == "shader.loop.unbounded"),
                "{source} -> {:?}",
                lint.diagnostics
            );
        }
    }

    #[test]
    fn exact_trip_counts_are_computed_for_every_operator() {
        for (source, expected) in [
            ("for (int i = 0; i < 8; i++) { x += 1.0; }", 8),
            ("for (int i = 0; i <= 8; i++) { x += 1.0; }", 9),
            ("for (int i = 8; i > 0; i--) { x += 1.0; }", 8),
            ("for (int i = 8; i >= 0; i--) { x += 1.0; }", 9),
            ("for (int i = 0; i < 8; i += 2) { x += 1.0; }", 4),
            ("for (int i = 10; i > 0; i -= 4) { x += 1.0; }", 3),
        ] {
            let lint = lint_source(source);
            assert!(!lint.has_fatal(), "{source} -> {:?}", lint.diagnostics);
            assert_eq!(lint.max_loop_iterations, Some(expected), "{source}");
        }
    }

    #[test]
    fn rejects_steps_that_cannot_be_proven_to_progress() {
        for source in [
            "for (int i = 0; i < 8; i += 0) { x += 1.0; }",
            "for (int i = 0; i < 8; i *= 2) { x += 1.0; }",
            "for (int i = 0; i < 8; i = i + 1) { x += 1.0; }",
        ] {
            let lint = lint_source(source);
            assert!(lint.has_fatal(), "{source} must be rejected");
        }
    }

    #[test]
    fn a_step_towards_an_already_false_condition_is_still_rejected() {
        // Direction mismatch means the loop can never be proven finite from the
        // header alone, even when the first evaluation is already false.
        let lint = lint_source("for (int i = 0; i > 5; i++) { x += 1.0; }");
        assert!(lint.has_fatal());
    }

    #[test]
    fn tracks_loop_depth_and_iterations() {
        let lint = lint_source(
            "void f() { for (int i = 0; i < 8; i++) { for (int j = 0; j < 4; j++) { x += 1.0; } } }",
        );
        assert_eq!(lint.max_loop_depth, 2);
        assert_eq!(lint.max_loop_iterations, Some(8));
        assert!(!lint.has_fatal(), "{:?}", lint.diagnostics);
    }

    #[test]
    fn flags_non_constant_loop_bounds() {
        let lint = lint_source("void f() { for (int i = 0; i < count; i++) { x += 1.0; } }");
        assert!(lint.has_fatal());
        assert!(lint
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "shader.loop.unbounded"));
    }

    #[test]
    fn flags_forbidden_extensions() {
        let lint = lint_source("#extension GL_ARB_shader_storage_buffer_object : require\n");
        assert!(lint.has_fatal());
        assert!(lint
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "shader.extension.forbidden"));
    }

    #[test]
    fn counts_texture_samples() {
        let source = "a = texture(s, uv); b = texture(s, uv2); c = tex.Sample(smp, uv3);";
        assert_eq!(lint_source(source).texture_samples, 3);
    }

    #[test]
    fn budget_rejects_oversized_source() {
        let source = "x".repeat(Budget::ECO.max_source_bytes + 1);
        let lint = lint_with_budget(&source, Budget::ECO);
        assert!(lint
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "shader.budget.source-size"));
    }

    #[test]
    fn budget_rejects_excessive_samples_and_depth() {
        let mut source = String::new();
        for _ in 0..5 {
            source.push_str("a = texture(s, uv);\n");
        }
        let lint = lint_with_budget(&source, Budget::ECO);
        assert!(lint
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "shader.budget.texture-samples"));

        let nested = "for (int i = 0; i < 4; i++) { for (int j = 0; j < 4; j++) { x += 1.0; } }";
        let lint = lint_with_budget(nested, Budget::ECO);
        assert!(lint
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "shader.budget.loop-depth"));
    }

    #[test]
    fn budget_rejects_long_constant_loops() {
        let source = "for (int i = 0; i < 16; i++) { x += 1.0; }";
        let lint = lint_with_budget(source, Budget::ECO);
        assert!(lint
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "shader.budget.loop-iterations"));
        // The same loop is fine at balanced/ultra.
        let lint = lint_with_budget(source, Budget::HIGH);
        assert!(!lint.has_fatal(), "{:?}", lint.diagnostics);
    }
}
