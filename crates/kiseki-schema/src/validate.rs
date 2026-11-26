//! Schema-level validation beyond what `serde` can express.
//!
//! `serde` enforces shape and types; this module enforces the *rules*: regexes
//! from the JSON Schema, layer consistency, path safety and label hygiene.

use std::collections::BTreeSet;

use kiseki_types::{Diagnostic, DiagnosticReport, DiagnosticSeverity};

use crate::security;
use crate::tokens::{
    LayerKind, OverrideTokens, ThemeManifest, ThemePack, ThemeTokens, SCHEMA_VERSION,
};

/// Result of validating one artefact, carrying every finding rather than
/// stopping at the first error so `doctor` can report a full picture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationOutcome {
    pub diagnostics: DiagnosticReport,
}

impl ValidationOutcome {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn error(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.push(Diagnostic::error(code, message));
    }

    pub fn warning(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.push(Diagnostic::warning(code, message));
    }

    pub fn info(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.push(Diagnostic::info(code, message));
    }

    /// True when at least one finding blocks installation or activation.
    pub fn has_fatal(&self) -> bool {
        self.diagnostics.has_fatal()
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Merges another outcome into this one.
    pub fn absorb(&mut self, other: ValidationOutcome) {
        self.diagnostics.extend(other.diagnostics);
    }

    /// Findings at a given severity, for CLI rendering.
    pub fn with_severity(&self, severity: DiagnosticSeverity) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(move |d| d.severity == severity)
    }
}

/// Validates a theme manifest against the RFC-0002 constraints.
pub fn validate_manifest(manifest: &ThemeManifest) -> ValidationOutcome {
    let mut outcome = ValidationOutcome::new();

    if manifest.schema_version != SCHEMA_VERSION {
        outcome.error(
            "schema.manifest.version",
            format!(
                "manifest schemaVersion is {} but this engine supports {SCHEMA_VERSION}",
                manifest.schema_version
            ),
        );
    }

    if !is_semver(&manifest.version) {
        outcome.error(
            "schema.manifest.version-invalid",
            format!("version {:?} is not a valid semver string", manifest.version),
        );
    }

    if manifest.authors.is_empty() {
        outcome.error("schema.manifest.authors", "at least one author is required");
    }
    for author in &manifest.authors {
        if author.name.trim().is_empty() {
            outcome.error("schema.manifest.author-name", "author name must not be blank");
        }
        if author.name.chars().any(char::is_control) {
            outcome.error(
                "schema.manifest.author-name",
                format!("author {:?} contains control characters", author.name),
            );
        }
        if let Some(url) = &author.url {
            if !url.starts_with("https://") {
                outcome.error(
                    "schema.manifest.author-url",
                    format!("author url {url:?} must be an https:// URL"),
                );
            }
        }
    }

    if !is_spdx_expression(&manifest.license) {
        outcome.error(
            "schema.manifest.license",
            format!("license {:?} is not a valid SPDX expression", manifest.license),
        );
    }

    if let Err(reason) = security::validate_relative_path(&manifest.credits_file) {
        outcome.error(
            "schema.manifest.credits-path",
            format!("creditsFile {:?} is unsafe: {reason}", manifest.credits_file),
        );
    }
    if let Some(preview) = &manifest.preview {
        if let Err(reason) = security::validate_relative_path(preview) {
            outcome.error(
                "schema.manifest.preview-path",
                format!("preview {preview:?} is unsafe: {reason}"),
            );
        } else if let Err(reason) = security::validate_extension(preview, security::ALLOWED_ASSET_EXTENSIONS) {
            outcome.error(
                "schema.manifest.preview-extension",
                format!("preview {preview:?} is not an allowed image: {reason}"),
            );
        }
    }

    if manifest.layers.is_empty() {
        outcome.error("schema.manifest.layers", "at least one layer must be declared");
    }
    if !manifest.has_layer(LayerKind::Palette) {
        outcome.error(
            "schema.manifest.layers",
            "the 'palette' layer is mandatory and must be declared",
        );
    }
    let mut seen = BTreeSet::new();
    for layer in &manifest.layers {
        if !seen.insert(layer.as_str()) {
            outcome.error(
                "schema.manifest.layers",
                format!("layer '{}' is declared more than once", layer.as_str()),
            );
        }
    }

    if let Some(tags) = Some(&manifest.tags) {
        if tags.len() > 12 {
            outcome.error("schema.manifest.tags", format!("{} tags exceeds the limit of 12", tags.len()));
        }
        let mut unique = BTreeSet::new();
        for tag in tags {
            if !unique.insert(tag.clone()) {
                outcome.error("schema.manifest.tags", format!("duplicate tag {tag:?}"));
            }
            let valid = tag.len() <= 24
                && tag.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
                && tag
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !valid {
                outcome.error(
                    "schema.manifest.tag-invalid",
                    format!("tag {tag:?} must be lowercase kebab-case, max 24 chars"),
                );
            }
        }
    }

    if let Some(min) = &manifest.min_engine_version {
        if !is_semver(min) {
            outcome.error(
                "schema.manifest.min-engine-version",
                format!("minEngineVersion {min:?} is not a valid semver string"),
            );
        }
    }

    if let Some(paired) = &manifest.paired_theme {
        if *paired == manifest.id {
            outcome.error(
                "schema.manifest.paired-theme",
                "pairedTheme must not point at the theme itself",
            );
        }
    }

    if let Some(provenance) = &manifest.provenance {
        if let Some(repo) = &provenance.source_repo {
            if !repo.starts_with("https://") {
                outcome.error(
                    "schema.manifest.provenance",
                    format!("provenance.sourceRepo {repo:?} must be an https:// URL"),
                );
            }
        }
    }

    outcome
}

/// Validates the five token layers.
pub fn validate_tokens(tokens: &ThemeTokens) -> ValidationOutcome {
    let mut outcome = ValidationOutcome::new();

    if tokens.schema_version != SCHEMA_VERSION {
        outcome.error(
            "schema.tokens.version",
            format!(
                "tokens schemaVersion is {} but this engine supports {SCHEMA_VERSION}",
                tokens.schema_version
            ),
        );
    }

    if let Some(atmosphere) = &tokens.atmosphere {
        if let Some(window) = &atmosphere.window {
            if !(0.2..=1.0).contains(&window.opacity) {
                outcome.error(
                    "schema.tokens.window-opacity",
                    format!("window.opacity {} must be within 0.2..=1.0", window.opacity),
                );
            }
            if let Some(unfocused) = window.unfocused_opacity {
                if !(0.2..=1.0).contains(&unfocused) {
                    outcome.error(
                        "schema.tokens.window-opacity",
                        format!("window.unfocusedOpacity {unfocused} must be within 0.2..=1.0"),
                    );
                }
            }
        }

        if let Some(wallpaper) = &atmosphere.wallpaper {
            match security::validate_relative_path(&wallpaper.path) {
                Err(reason) => outcome.error(
                    "schema.tokens.wallpaper-path",
                    format!("wallpaper.path {:?} is unsafe: {reason}", wallpaper.path),
                ),
                Ok(()) => {
                    if let Err(reason) =
                        security::validate_extension(&wallpaper.path, security::ALLOWED_ASSET_EXTENSIONS)
                    {
                        outcome.error(
                            "schema.tokens.wallpaper-extension",
                            format!("wallpaper.path {:?} is invalid: {reason}", wallpaper.path),
                        );
                    }
                }
            }
            for opacity in [wallpaper.opacity, wallpaper.unfocused_opacity].into_iter().flatten() {
                if !(0.0..=1.0).contains(&opacity) {
                    outcome.error(
                        "schema.tokens.wallpaper-opacity",
                        format!("wallpaper opacity {opacity} must be within 0.0..=1.0"),
                    );
                }
            }
            if let Some(zone) = &wallpaper.code_safe_zone {
                if !(0.0..=1.0).contains(&zone.start_x) || !(0.0..=1.0).contains(&zone.end_x) {
                    outcome.error(
                        "schema.tokens.safe-zone",
                        "codeSafeZone startX/endX must be within 0.0..=1.0",
                    );
                }
                if zone.start_x >= zone.end_x {
                    outcome.error(
                        "schema.tokens.safe-zone",
                        format!(
                            "codeSafeZone startX ({}) must be less than endX ({})",
                            zone.start_x, zone.end_x
                        ),
                    );
                }
            }
            if let Some(variants) = &wallpaper.variants {
                for (name, path) in [("portrait", &variants.portrait), ("compact", &variants.compact)] {
                    if let Some(path) = path {
                        if let Err(reason) = security::validate_relative_path(path) {
                            outcome.error(
                                "schema.tokens.wallpaper-variant",
                                format!("wallpaper variants.{name} {path:?} is unsafe: {reason}"),
                            );
                        } else if let Err(reason) =
                            security::validate_extension(path, security::ALLOWED_ASSET_EXTENSIONS)
                        {
                            outcome.error(
                                "schema.tokens.wallpaper-variant",
                                format!("wallpaper variants.{name} {path:?} is invalid: {reason}"),
                            );
                        }
                    }
                }
            }
        }

        if let Some(shader) = &atmosphere.shader {
            for (label, path) in [("glsl", &shader.glsl), ("hlsl", &shader.hlsl)] {
                if let Some(path) = path {
                    if let Err(reason) = security::validate_relative_path(path) {
                        outcome.error(
                            "schema.tokens.shader-path",
                            format!("shader.{label} {path:?} is unsafe: {reason}"),
                        );
                    } else if let Err(reason) = security::validate_extension(path, &[label]) {
                        outcome.error(
                            "schema.tokens.shader-extension",
                            format!("shader.{label} {path:?} is invalid: {reason}"),
                        );
                    }
                }
            }
            if shader.params.len() > 24 {
                outcome.error(
                    "schema.tokens.shader-params",
                    format!("{} shader params exceeds the limit of 24", shader.params.len()),
                );
            }
            for name in shader.params.keys() {
                let valid = name.len() <= 32
                    && name.starts_with(|c: char| c.is_ascii_alphabetic())
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !valid {
                    outcome.error(
                        "schema.tokens.shader-param-name",
                        format!("shader param name {name:?} must match ^[a-zA-Z][a-zA-Z0-9_]{{0,31}}$"),
                    );
                }
            }
        }
    }

    if let Some(typography) = &tokens.typography {
        if typography.families.is_empty() {
            outcome.error("schema.tokens.typography", "typography.families must not be empty");
        }
        if typography.families.len() > 6 {
            outcome.error(
                "schema.tokens.typography",
                format!("{} families exceeds the limit of 6", typography.families.len()),
            );
        }
        for family in &typography.families {
            let valid = !family.is_empty()
                && family.len() <= 64
                && family
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'));
            if !valid {
                outcome.error(
                    "schema.tokens.font-family",
                    format!("font family {family:?} contains disallowed characters"),
                );
            }
        }
        if let Some(size) = typography.size_pt {
            if !(6.0..=36.0).contains(&size) {
                outcome.error(
                    "schema.tokens.font-size",
                    format!("typography.sizePt {size} must be within 6.0..=36.0"),
                );
            }
        }
        if let Some(line_height) = typography.line_height {
            if !(1.0..=2.0).contains(&line_height) {
                outcome.error(
                    "schema.tokens.line-height",
                    format!("typography.lineHeight {line_height} must be within 1.0..=2.0"),
                );
            }
        }
    }

    if let Some(overrides) = &tokens.overrides {
        outcome.absorb(validate_overrides(overrides));
    }

    outcome
}

/// Validates the L5 safe overrides, which is where injection payloads would
/// try to hide.
pub fn validate_overrides(overrides: &OverrideTokens) -> ValidationOutcome {
    let mut outcome = ValidationOutcome::new();

    if let Some(prompt) = &overrides.prompt {
        for (field, value) in [
            ("prompt.badgeText", &prompt.badge_text),
            ("prompt.okLabel", &prompt.ok_label),
            ("prompt.errorLabel", &prompt.error_label),
            ("prompt.consoleTitlePrefix", &prompt.console_title_prefix),
        ] {
            if let Some(value) = value {
                if let Err(reason) = security::validate_safe_label(value, field, 48) {
                    outcome.error("security.label-injection", reason);
                }
            }
        }
        if let Some(symbol) = &prompt.prompt_symbol {
            if let Err(reason) = security::validate_safe_glyph(symbol, "prompt.promptSymbol") {
                outcome.error("security.label-injection", reason);
            }
        }
    }

    if let Some(fzf) = &overrides.fzf {
        for (field, value) in [
            ("fzf.prompt", &fzf.prompt),
            ("fzf.pointer", &fzf.pointer),
            ("fzf.marker", &fzf.marker),
        ] {
            if let Some(value) = value {
                if let Err(reason) = security::validate_safe_glyph(value, field) {
                    outcome.error("security.label-injection", reason);
                }
            }
        }
    }

    if let Some(fastfetch) = &overrides.fastfetch {
        if let Some(logo_path) = &fastfetch.logo_path {
            match security::validate_relative_path(logo_path) {
                Err(reason) => outcome.error(
                    "security.logo-path",
                    format!("fastfetch.logoPath {logo_path:?} is unsafe: {reason}"),
                ),
                Ok(()) => {
                    if let Err(reason) = security::validate_extension(logo_path, &["txt"]) {
                        outcome.error(
                            "security.logo-path",
                            format!("fastfetch.logoPath {logo_path:?} is invalid: {reason}"),
                        );
                    }
                }
            }
        }
        if let Some(title) = &fastfetch.header_title {
            if let Err(reason) = security::validate_safe_label(title, "fastfetch.headerTitle", 48) {
                outcome.error("security.label-injection", reason);
            }
        }
        if let Some(labels) = &fastfetch.field_labels {
            let fields = [
                ("title", &labels.title),
                ("os", &labels.os),
                ("shell", &labels.shell),
                ("terminal", &labels.terminal),
                ("cpu", &labels.cpu),
                ("memory", &labels.memory),
                ("uptime", &labels.uptime),
            ];
            for (name, value) in fields {
                if let Some(value) = value {
                    let field = format!("fastfetch.fieldLabels.{name}");
                    if let Err(reason) = security::validate_safe_label(value, &field, 48) {
                        outcome.error("security.label-injection", reason);
                    }
                }
            }
        }
    }

    outcome
}

/// Content of optional pack assets, supplied by the caller that owns IO.
#[derive(Debug, Clone, Default)]
pub struct PackAssets {
    /// Raw `assets/logo.txt` text, when the pack ships one.
    pub logo_text: Option<String>,
    /// Every pack-relative file path in the archive.
    pub file_listing: Vec<String>,
}

/// Validates a complete pack: manifest, tokens, declared-layer consistency and
/// any asset content the caller could load.
pub fn validate_pack(pack: &ThemePack, assets: &PackAssets) -> ValidationOutcome {
    let mut outcome = validate_manifest(&pack.manifest);
    outcome.absorb(validate_tokens(&pack.tokens));

    let declared = &pack.manifest.layers;
    let tokens = &pack.tokens;
    let expected = [
        (LayerKind::Syntax, tokens.syntax.is_some() || tokens.diff.is_some()),
        (LayerKind::Ui, tokens.ui.is_some()),
        (LayerKind::Atmosphere, tokens.atmosphere.is_some()),
        (LayerKind::Typography, tokens.typography.is_some()),
        (LayerKind::Overrides, tokens.overrides.is_some()),
    ];
    for (layer, present) in expected {
        if present && !declared.contains(&layer) {
            outcome.warning(
                "schema.layers.undeclared",
                format!(
                    "tokens.json authors the '{}' layer but manifest.layers does not declare it",
                    layer.as_str()
                ),
            );
        }
    }
    if declared.contains(&LayerKind::Typography) && tokens.typography.is_none() {
        outcome.warning(
            "schema.layers.missing",
            "manifest declares the 'typography' layer but tokens.json omits it",
        );
    }

    // Wallpaper/shader files referenced by L3 must actually ship.
    if !assets.file_listing.is_empty() {
        let listing: BTreeSet<&str> = assets.file_listing.iter().map(String::as_str).collect();
        if let Some(atmosphere) = &tokens.atmosphere {
            let mut referenced = Vec::new();
            if let Some(wallpaper) = &atmosphere.wallpaper {
                referenced.push(("wallpaper.path", wallpaper.path.as_str()));
                if let Some(variants) = &wallpaper.variants {
                    if let Some(path) = &variants.portrait {
                        referenced.push(("wallpaper.variants.portrait", path.as_str()));
                    }
                    if let Some(path) = &variants.compact {
                        referenced.push(("wallpaper.variants.compact", path.as_str()));
                    }
                }
            }
            if let Some(shader) = &atmosphere.shader {
                if let Some(path) = &shader.glsl {
                    referenced.push(("shader.glsl", path.as_str()));
                }
                if let Some(path) = &shader.hlsl {
                    referenced.push(("shader.hlsl", path.as_str()));
                }
            }
            for (field, path) in referenced {
                if !listing.contains(path) {
                    outcome.error(
                        "schema.asset.missing",
                        format!("{field} references {path:?} which is not present in the pack"),
                    );
                }
            }
        }
    }

    if let Some(logo) = &assets.logo_text {
        if let Err(reason) = security::audit_logo_text(logo, 32 * 1024) {
            outcome.error("security.logo-escape", reason);
        }
    }

    if !assets.file_listing.is_empty() {
        for diagnostic in security::lint_pack_file_listing(&assets.file_listing) {
            outcome.push(diagnostic);
        }
    }

    outcome
}

fn is_semver(raw: &str) -> bool {
    let core = raw.split(['-', '+']).next().unwrap_or_default();
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part.chars().all(|c| c.is_ascii_digit())
            && (part == &"0" || !part.starts_with('0'))
    })
}

fn is_spdx_expression(raw: &str) -> bool {
    if raw.len() < 2 || raw.len() > 64 {
        return false;
    }
    // Alternate `id (AND|OR id)* (WITH id)?`; never start or end on an operator.
    let mut expect_id = true;
    for token in raw.split(' ') {
        match token {
            "AND" | "OR" => {
                if expect_id {
                    return false;
                }
                expect_id = true;
            }
            "WITH" => {
                if expect_id {
                    return false;
                }
                // `WITH` is always followed by a single exception id.
                expect_id = true;
            }
            id => {
                if !expect_id || !is_spdx_id(id) {
                    return false;
                }
                expect_id = false;
            }
        }
    }
    !expect_id
}

fn is_spdx_id(raw: &str) -> bool {
    !raw.is_empty() && raw.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{nerv_eva_manifest, nerv_eva_pack, nerv_eva_tokens};
    use crate::tokens::PromptOverrides;

    #[test]
    fn nerv_eva_passes_manifest_validation() {
        let outcome = validate_manifest(&nerv_eva_manifest());
        assert!(!outcome.has_fatal(), "{:?}", outcome.diagnostics);
    }

    #[test]
    fn nerv_eva_passes_token_validation() {
        let outcome = validate_tokens(&nerv_eva_tokens());
        assert!(!outcome.has_fatal(), "{:?}", outcome.diagnostics);
    }

    #[test]
    fn nerv_eva_pack_validates_with_asset_listing() {
        let pack = nerv_eva_pack();
        let assets = PackAssets {
            logo_text: Some(include_str!("../../../themes/nerv-eva/assets/logo.txt").to_string()),
            file_listing: vec![
                "manifest.json".into(),
                "tokens.json".into(),
                "assets/wallpaper.png".into(),
                "assets/logo.txt".into(),
                "shaders/live.glsl".into(),
                "shaders/live.hlsl".into(),
                "CREDITS.md".into(),
                "LICENSE".into(),
            ],
        };
        let outcome = validate_pack(&pack, &assets);
        assert!(!outcome.has_fatal(), "{:?}", outcome.diagnostics);
    }

    #[test]
    fn missing_declared_asset_is_fatal() {
        let pack = nerv_eva_pack();
        let assets = PackAssets {
            logo_text: None,
            file_listing: vec!["manifest.json".into(), "tokens.json".into()],
        };
        let outcome = validate_pack(&pack, &assets);
        assert!(outcome.has_fatal());
        let codes: Vec<&str> = outcome
            .with_severity(DiagnosticSeverity::Error)
            .map(|d| d.code.as_str())
            .collect();
        assert!(codes.contains(&"schema.asset.missing"), "{codes:?}");
    }

    #[test]
    fn script_in_listing_is_fatal() {
        let pack = nerv_eva_pack();
        let assets = PackAssets {
            logo_text: None,
            file_listing: vec!["manifest.json".into(), "hooks/evil.sh".into()],
        };
        let outcome = validate_pack(&pack, &assets);
        assert!(outcome.has_fatal());
        assert!(outcome
            .with_severity(DiagnosticSeverity::Error)
            .any(|d| d.code == "security.forbidden-extension"));
    }

    #[test]
    fn injected_prompt_label_is_fatal() {
        let mut tokens = nerv_eva_tokens();
        let overrides = tokens.overrides.as_mut().unwrap();
        overrides.prompt = Some(PromptOverrides {
            badge_text: Some("02\ncommand = evil".into()),
            ..Default::default()
        });
        let outcome = validate_tokens(&tokens);
        assert!(outcome.has_fatal());
        assert!(outcome
            .with_severity(DiagnosticSeverity::Error)
            .any(|d| d.code == "security.label-injection"));
    }

    #[test]
    fn traversal_wallpaper_path_is_fatal() {
        let mut tokens = nerv_eva_tokens();
        tokens.atmosphere.as_mut().unwrap().wallpaper.as_mut().unwrap().path =
            "../../etc/passwd.png".into();
        let outcome = validate_tokens(&tokens);
        assert!(outcome.has_fatal());
        assert!(outcome
            .with_severity(DiagnosticSeverity::Error)
            .any(|d| d.code == "schema.tokens.wallpaper-path"));
    }

    #[test]
    fn safe_zone_requires_ordered_bounds() {
        let mut tokens = nerv_eva_tokens();
        let zone = tokens
            .atmosphere
            .as_mut()
            .unwrap()
            .wallpaper
            .as_mut()
            .unwrap()
            .code_safe_zone
            .as_mut()
            .unwrap();
        zone.start_x = 0.8;
        zone.end_x = 0.2;
        let outcome = validate_tokens(&tokens);
        assert!(outcome.has_fatal());
        assert!(outcome
            .with_severity(DiagnosticSeverity::Error)
            .any(|d| d.code == "schema.tokens.safe-zone"));
    }

    #[test]
    fn bad_spdx_and_version_are_rejected() {
        let mut manifest = nerv_eva_manifest();
        manifest.license = "MIT; rm -rf /".into();
        manifest.version = "1.0".into();
        let outcome = validate_manifest(&manifest);
        assert!(outcome.has_fatal());
        let codes: Vec<&str> = outcome
            .with_severity(DiagnosticSeverity::Error)
            .map(|d| d.code.as_str())
            .collect();
        assert!(codes.contains(&"schema.manifest.license"));
        assert!(codes.contains(&"schema.manifest.version-invalid"));
    }

    #[test]
    fn spdx_accepts_compound_expressions() {
        assert!(is_spdx_expression("MIT"));
        assert!(is_spdx_expression("MIT OR Apache-2.0"));
        assert!(is_spdx_expression("CC-BY-4.0 AND MIT"));
        assert!(!is_spdx_expression("M"));
        assert!(!is_spdx_expression("MIT OR"));
    }

    #[test]
    fn undeclared_layer_is_a_warning_not_an_error() {
        let mut manifest = nerv_eva_manifest();
        manifest.layers.retain(|layer| *layer != LayerKind::Atmosphere);
        let pack = ThemePack {
            manifest,
            tokens: nerv_eva_tokens(),
        };
        let outcome = validate_pack(&pack, &PackAssets::default());
        assert!(!outcome.has_fatal());
        assert!(outcome
            .with_severity(DiagnosticSeverity::Warning)
            .any(|d| d.code == "schema.layers.undeclared"));
    }

    #[test]
    fn missing_palette_layer_is_fatal() {
        let mut manifest = nerv_eva_manifest();
        manifest.layers.retain(|layer| *layer != LayerKind::Palette);
        let outcome = validate_manifest(&manifest);
        assert!(outcome.has_fatal());
    }
}
