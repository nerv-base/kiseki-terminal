//! Windows Terminal adapter: comment-preserving JSONC patching (RFC-0003 §5.1).
//!
//! Windows Terminal has no include mechanism, so Kiseki has to edit
//! `settings.json`. The CST-based editor in this module preserves `//` and
//! `/* */` comments, blank lines, key order, indentation and trailing commas,
//! touches only the managed visual keys, and records a baseline the first time
//! it takes over the file so `uninstall` can restore it precisely.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jsonc_parser::cst::{CstArray, CstInputValue, CstObject, CstRootNode};
use jsonc_parser::{CollectOptions, ParseOptions};
use kiseki_compiler::marker::{MARKER_BEGIN, MARKER_END};
use kiseki_compiler::windows_terminal::{
    managed_leaves, WtVisualPayload, FORBIDDEN_KEYS, MANAGED_DEFAULTS_KEYS, MANAGED_NESTED_KEYS,
    OWNED_PREFIX,
};
use kiseki_platform::{PathKind, Platform, ReloadOutcome};
use kiseki_types::{CapabilityFlags, Diagnostic, EnvContext, KisekiError, OsKind, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    render_only_diagnostic, Adapter, AdapterKind, AdapterPlan, DetectionReport, FileOperation,
    IntegrationTier, PlanContext, ValidationReport,
};

/// Baseline captured before Kiseki's first visual patch.
///
/// Stored at `<kiseki_data>/journal/wt-baseline.json`; `uninstall` restores
/// exactly the values present (or absent) at capture time.
///
/// Object-valued keys are captured leaf by leaf (`font.face`, `font.size`, ...)
/// so a user's sibling properties and comments are never overwritten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WtBaseline {
    pub schema_version: u32,
    pub created_at: String,
    pub source: PathBuf,
    /// Every managed leaf: `None` means "was absent".
    pub defaults: BTreeMap<String, Option<Value>>,
    /// Previous Kiseki-owned `schemes[]` entries (normally empty).
    #[serde(default)]
    pub schemes: Vec<Value>,
    /// Previous Kiseki-owned `themes[]` entries (normally empty).
    #[serde(default)]
    pub themes: Vec<Value>,
    pub top_level_theme_present: bool,
    pub top_level_theme: Option<Value>,
}

/// Version of the baseline document. Bumped when the key layout changes.
pub const WT_BASELINE_SCHEMA_VERSION: u32 = 2;

impl WtBaseline {
    /// Loads a baseline, returning `None` when it is missing or from an
    /// incompatible layout (which silently forces a fresh capture).
    pub fn load(path: &Path, platform: &dyn Platform) -> Result<Option<Self>> {
        let Some(raw) = platform.read_to_string(path)? else {
            return Ok(None);
        };
        let baseline: Self = serde_json::from_str(&raw)
            .map_err(|error| KisekiError::State(format!("wt-baseline.json: {error}")))?;
        if baseline.schema_version != WT_BASELINE_SCHEMA_VERSION {
            return Ok(None);
        }
        Ok(Some(baseline))
    }

    pub fn save(&self, path: &Path, platform: &dyn Platform) -> Result<()> {
        let encoded = serde_json::to_string_pretty(self)
            .map_err(|error| KisekiError::Json(error.to_string()))?;
        platform.write_preserving_symlinks(path, format!("{encoded}\n").as_bytes())?;
        Ok(())
    }
}

/// A profile in `profiles.list` that hard-codes a visual key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileConflict {
    pub profile: String,
    pub key: String,
    pub line: usize,
}

impl ProfileConflict {
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::warning(
            "adapter.wt.subprofile-override",
            format!(
                "profile {:?} overrides `{}` at line {}; the Kiseki color scheme will not apply there",
                self.profile, self.key, self.line
            ),
        )
        .with_hint(format!(
            "remove `{}` from that profile, or delete the profile overrides",
            self.key
        ))
    }
}

/// Windows Terminal adapter.
pub struct WindowsTerminalAdapter;

impl WindowsTerminalAdapter {
    fn baseline_path(paths: &kiseki_platform::KisekiPaths) -> PathBuf {
        paths.journal_dir().join("wt-baseline.json")
    }
}

impl Adapter for WindowsTerminalAdapter {
    fn id(&self) -> &'static str {
        "windows-terminal"
    }

    fn display_name(&self) -> &'static str {
        "Windows Terminal"
    }

    fn kind(&self) -> AdapterKind {
        AdapterKind::Terminal
    }

    fn tier(&self) -> IntegrationTier {
        IntegrationTier::Tier1Managed
    }

    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport> {
        let config_path = wt_settings_path(platform);
        let path_kind = config_path
            .as_deref()
            .map(|path| platform.inspect(path))
            .unwrap_or(PathKind::Missing);
        let mut notes = Vec::new();
        if let Some(path) = config_path.as_ref() {
            if let Ok(Some(raw)) = platform.read_to_string(path) {
                for conflict in detect_profile_conflicts(&raw) {
                    notes.push(format!(
                        "profile {:?} overrides `{}` (line {})",
                        conflict.profile, conflict.key, conflict.line
                    ));
                }
            }
        } else {
            notes.push("no Windows Terminal settings.json found".to_string());
        }
        Ok(DetectionReport {
            installed: path_kind != PathKind::Missing || env.terminal_is("windows-terminal"),
            version: None,
            config_path,
            path_kind,
            extra_config_paths: Vec::new(),
            active_in_session: env.terminal_is("windows-terminal"),
            notes,
        })
    }

    fn capabilities(&self, _detection: &DetectionReport, _env: &EnvContext) -> CapabilityFlags {
        CapabilityFlags::PALETTE_16
            | CapabilityFlags::TRUECOLOR
            | CapabilityFlags::WINDOW_OPACITY
            | CapabilityFlags::WINDOW_BLUR
            | CapabilityFlags::WALLPAPER_STATIC
            | CapabilityFlags::WALLPAPER_ALIGNMENT
            | CapabilityFlags::FONT_SINGLE_ONLY
            | CapabilityFlags::UI_CHROME
            | CapabilityFlags::SHADER_LAYERED_BG
            | CapabilityFlags::HOT_RELOAD_WATCH
            | CapabilityFlags::REQUIRES_JSONC_PATCH
    }

    fn plan(
        &self,
        ctx: &PlanContext<'_>,
        detection: &DetectionReport,
        _caps: CapabilityFlags,
    ) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        plan.add_generated(ctx.artifacts, kiseki_compiler::artifact_names::WT_VISUAL);

        for note in &detection.notes {
            plan.diagnostic(
                Diagnostic::warning("adapter.wt.subprofile-override", note.clone())
                    .with_hint("remove `colorScheme` / `backgroundImage` from that profile"),
            );
        }

        let Some(payload_raw) = ctx
            .artifacts
            .get(kiseki_compiler::artifact_names::WT_VISUAL)
        else {
            return Ok(plan);
        };
        let payload = WtVisualPayload::from_json(payload_raw)?;
        plan.diagnostics.extend(payload.lint());

        let host_writes = ctx.options.host_writes_allowed(ctx.env);
        let Some(config_path) = detection.config_path.as_ref() else {
            return Ok(plan);
        };
        if !host_writes {
            plan.render_only = true;
            plan.diagnostic(Diagnostic::info(
                "adapter.wt.render-only",
                "host writes suppressed; WT visual payload generated only",
            ));
            return Ok(plan);
        }
        if detection.path_kind == PathKind::Missing {
            plan.diagnostic(Diagnostic::info(
                "adapter.wt.no-settings",
                "no Windows Terminal settings.json to patch; payload generated only",
            ));
            return Ok(plan);
        }
        if !detection.path_kind.is_writable() {
            plan.render_only = true;
            plan.diagnostic(render_only_diagnostic(
                self.id(),
                config_path,
                detection.path_kind.label(),
                format!(
                    "copy the managed keys from {} into your Nix/Home Manager WT config",
                    ctx.paths
                        .config_dir()
                        .join("generated")
                        .join(kiseki_compiler::artifact_names::WT_VISUAL)
                        .display()
                ),
            ));
            return Ok(plan);
        }

        plan.push(FileOperation::PatchWindowsTerminal {
            target: config_path.clone(),
            payload,
            baseline: Self::baseline_path(ctx.paths),
        });
        Ok(plan)
    }

    fn validate(&self, plan: &AdapterPlan, platform: &dyn Platform) -> Result<ValidationReport> {
        let mut report = ValidationReport::ok();
        for diagnostic in plan.diagnostics.iter().filter(|diagnostic| diagnostic.is_fatal()) {
            report.push(diagnostic.clone());
        }
        let Some(payload) = plan.operations.iter().find_map(|operation| match operation {
            FileOperation::PatchWindowsTerminal { payload, .. } => Some(payload),
            _ => None,
        }) else {
            return Ok(report);
        };
        for diagnostic in payload.lint() {
            report.push(diagnostic);
        }
        let Some(target) = plan.operations.iter().find_map(|operation| match operation {
            FileOperation::PatchWindowsTerminal { target, .. } => Some(target),
            _ => None,
        }) else {
            return Ok(report);
        };
        let Some(original) = platform.read_to_string(target)? else {
            return Ok(report);
        };
        match patch_settings(&original, payload) {
            Ok(patched) => {
                if let Err(error) = parse_value(&patched) {
                    report.push(Diagnostic::error(
                        "adapter.wt.invalid-patch",
                        format!("patched settings.json does not parse: {error}"),
                    ));
                }
                let before = count_comments(&original);
                let after = count_comments(&patched);
                if after < before {
                    report.push(Diagnostic::error(
                        "adapter.wt.comment-loss",
                        format!("patch lost comments ({before} -> {after})"),
                    ));
                }
            }
            Err(error) => report.push(Diagnostic::error(
                "adapter.wt.invalid-settings",
                format!("cannot patch {}: {error}", target.display()),
            )),
        }
        Ok(report)
    }

    fn reload(
        &self,
        _detection: &DetectionReport,
        _caps: CapabilityFlags,
        _platform: &dyn Platform,
    ) -> Result<ReloadOutcome> {
        Ok(ReloadOutcome::AutoReload {
            detail: "Windows Terminal watches settings.json and reloads automatically".to_string(),
        })
    }

    fn plan_unhook(
        &self,
        detection: &DetectionReport,
        paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan> {
        let mut plan = AdapterPlan::new(self.id());
        if let Some(target) = detection.config_path.as_ref() {
            plan.push(FileOperation::RestoreWindowsTerminal {
                target: target.clone(),
                baseline: Self::baseline_path(paths),
            });
        }
        Ok(plan)
    }
}

/// Resolves `settings.json` for the host, honouring the test/env override.
pub fn wt_settings_path(platform: &dyn Platform) -> Option<PathBuf> {
    if let Some(explicit) = platform.env_var("KISEKI_WT_SETTINGS") {
        return Some(PathBuf::from(explicit));
    }
    if platform.os() != OsKind::Windows {
        return None;
    }
    let local = platform.env_var("LOCALAPPDATA").map(PathBuf::from)?;
    let packaged = local
        .join("Packages")
        .join("Microsoft.WindowsTerminal_8wekyb3d8bbwe")
        .join("LocalState")
        .join("settings.json");
    if packaged.exists() {
        return Some(packaged);
    }
    let preview = local
        .join("Packages")
        .join("Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe")
        .join("LocalState")
        .join("settings.json");
    if preview.exists() {
        return Some(preview);
    }
    let unpackaged = local
        .join("Microsoft")
        .join("Windows Terminal")
        .join("settings.json");
    if unpackaged.exists() {
        return Some(unpackaged);
    }
    Some(packaged)
}

fn parse_options() -> ParseOptions {
    ParseOptions::default()
}

fn parse_value(settings: &str) -> Result<Value> {
    jsonc_parser::parse_to_serde_value(settings, &parse_options())
        .map_err(|error| KisekiError::Json(error.to_string()))
}

fn to_cst(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(boolean) => CstInputValue::Bool(*boolean),
        Value::Number(number) => CstInputValue::Number(number.to_string()),
        Value::String(text) => CstInputValue::String(text.clone()),
        Value::Array(items) => CstInputValue::Array(items.iter().map(to_cst).collect()),
        Value::Object(map) => CstInputValue::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), to_cst(value)))
                .collect(),
        ),
    }
}

fn object_root(root: &CstRootNode) -> Result<CstObject> {
    if let Some(object) = root.object_value() {
        return Ok(object);
    }
    if root.value().is_some() {
        return Err(KisekiError::adapter(
            "windows-terminal",
            "settings.json root must be a JSON object",
        ));
    }
    Ok(root.object_value_or_set())
}

fn upsert_owned_array_entry(
    object: &CstObject,
    key: &str,
    value: &Value,
    prefix: &str,
) -> Result<()> {
    let array: CstArray = match object.get(key) {
        Some(property) => property.array_value().ok_or_else(|| {
            KisekiError::adapter(
                "windows-terminal",
                format!("`{key}` exists but is not an array; refusing to replace user data"),
            )
        })?,
        None => object.array_value_or_set(key),
    };
    let stale: Vec<_> = array
        .elements()
        .into_iter()
        .filter(|node| {
            node.to_serde_value()
                .and_then(|value| {
                    value
                        .get("name")
                        .and_then(Value::as_str)
                        .map(|name| name.starts_with(prefix))
                })
                .unwrap_or(false)
        })
        .collect();
    for node in stale {
        node.remove();
    }
    array.append(to_cst(value));
    Ok(())
}

fn child_object(parent: &CstObject, key: &str) -> Result<CstObject> {
    match parent.get(key) {
        Some(property) => property.object_value().ok_or_else(|| {
            KisekiError::adapter(
                "windows-terminal",
                format!("`{key}` exists but is not an object; refusing to replace user data"),
            )
        }),
        None => Ok(parent.object_value_or_set(key)),
    }
}

/// True when `key` is a dotted leaf path (`font.face`) of a leaf-managed object.
fn is_nested_leaf_path(key: &str) -> bool {
    MANAGED_NESTED_KEYS.iter().any(|(parent, leaves)| {
        key.strip_prefix(parent)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|leaf| leaves.contains(&leaf))
    })
}

/// True when `object` has no remaining properties (after comment/trivia).
fn is_empty_object(object: &CstObject) -> bool {
    object.children_exclude_trivia_and_tokens().is_empty()
}

/// Sets a managed leaf inside an object, creating it when absent.
fn set_leaf(object: &CstObject, key: &str, value: &Value) {
    if let Some(property) = object.get(key) {
        property.set_value(to_cst(value));
    } else {
        object.append(key, to_cst(value));
    }
}

/// Removes a managed leaf, returning true when something was actually removed.
fn remove_leaf(object: &CstObject, key: &str) -> bool {
    match object.get(key) {
        Some(property) => {
            property.remove();
            true
        }
        None => false,
    }
}

/// Applies a visual payload while preserving every comment and formatting byte.
///
/// Object-valued managed keys (`font`, `unfocusedAppearance`) are patched leaf
/// by leaf so user siblings such as `font.features` and their comments survive.
pub fn patch_settings(original: &str, payload: &WtVisualPayload) -> Result<String> {
    let root = CstRootNode::parse(original, &parse_options())
        .map_err(|error| KisekiError::adapter("windows-terminal", format!("settings.json: {error}")))?;
    let object = object_root(&root)?;

    // Validate every payload key before touching a single byte.
    for (key, value) in &payload.defaults {
        if FORBIDDEN_KEYS.contains(&key.as_str()) || !MANAGED_DEFAULTS_KEYS.contains(&key.as_str()) {
            return Err(KisekiError::adapter(
                "windows-terminal",
                format!("payload key `{key}` is not in the managed whitelist"),
            ));
        }
        if managed_leaves(key).is_some() && !value.is_object() {
            return Err(KisekiError::adapter(
                "windows-terminal",
                format!("payload key `{key}` must be an object"),
            ));
        }
    }

    upsert_owned_array_entry(&object, "schemes", &payload.scheme, OWNED_PREFIX)?;
    if let Some(ui_theme) = payload.ui_theme.as_ref() {
        upsert_owned_array_entry(&object, "themes", ui_theme, OWNED_PREFIX)?;
    }

    let profiles = child_object(&object, "profiles")?;
    let defaults = child_object(&profiles, "defaults")?;

    // --- 1. Leaf-managed objects -----------------------------------------
    for (parent, leaves) in MANAGED_NESTED_KEYS {
        let wanted = payload.defaults.get(*parent).and_then(Value::as_object);
        let existing = match defaults.get(parent) {
            Some(property) => {
                let Some(object) = property.object_value() else {
                    return Err(KisekiError::adapter(
                        "windows-terminal",
                        format!("`{parent}` exists but is not an object; refusing to replace user data"),
                    ));
                };
                Some(object)
            }
            None => None,
        };

        let Some(object) = existing else {
            // Parent absent: only create it when the payload actually has a
            // leaf to write, and write just the managed leaves.
            if let Some(map) = wanted {
                let created = defaults.object_value_or_set(parent);
                for leaf in *leaves {
                    if let Some(value) = map.get(*leaf) {
                        created.append(leaf, to_cst(value));
                    }
                }
                if is_empty_object(&created) {
                    if let Some(property) = defaults.get(parent) {
                        property.remove();
                    }
                }
            }
            continue;
        };

        let mut removed = false;
        for leaf in *leaves {
            match wanted.and_then(|map| map.get(*leaf)) {
                Some(value) => set_leaf(&object, leaf, value),
                None => removed |= remove_leaf(&object, leaf),
            }
        }
        if removed && is_empty_object(&object) {
            if let Some(property) = defaults.get(parent) {
                property.remove();
            }
        }
    }

    // --- 2. Scalar managed keys ------------------------------------------
    for (key, value) in &payload.defaults {
        if managed_leaves(key).is_some() {
            continue;
        }
        set_leaf(&defaults, key, value);
    }

    // --- 3. Reconcile: drop managed keys this payload no longer manages ---
    for key in MANAGED_DEFAULTS_KEYS {
        if payload.defaults.contains_key(*key) || managed_leaves(key).is_some() {
            continue;
        }
        remove_leaf(&defaults, key);
    }

    if let Some(theme) = payload.top_level_theme.as_ref() {
        if let Some(property) = object.get("theme") {
            property.set_value(CstInputValue::String(theme.clone()));
        } else {
            object.append("theme", CstInputValue::String(theme.clone()));
        }
    }

    Ok(root.to_string())
}

/// Captures the pre-patch values of every managed leaf.
pub fn capture_baseline(
    settings: &str,
    source: &Path,
    created_at: &str,
) -> WtBaseline {
    let value = parse_value(settings).unwrap_or(Value::Null);
    let defaults = value
        .get("profiles")
        .and_then(|profiles| profiles.get("defaults"));
    let mut captured = BTreeMap::new();
    for key in MANAGED_DEFAULTS_KEYS {
        match managed_leaves(key) {
            Some(leaves) => {
                for leaf in leaves {
                    let value = defaults
                        .and_then(|defaults| defaults.get(*key))
                        .and_then(Value::as_object)
                        .and_then(|object| object.get(*leaf))
                        .cloned();
                    captured.insert(format!("{key}.{leaf}"), value);
                }
            }
            None => {
                captured.insert(
                    (*key).to_string(),
                    defaults.and_then(|defaults| defaults.get(*key)).cloned(),
                );
            }
        }
    }
    let owned = |array: Option<&Value>| -> Vec<Value> {
        array
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| {
                        item.get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|name| name.starts_with(OWNED_PREFIX))
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    WtBaseline {
        schema_version: WT_BASELINE_SCHEMA_VERSION,
        created_at: created_at.to_string(),
        source: source.to_path_buf(),
        defaults: captured,
        schemes: owned(value.get("schemes")),
        themes: owned(value.get("themes")),
        top_level_theme_present: value.get("theme").is_some(),
        top_level_theme: value.get("theme").cloned(),
    }
}

/// Restores every managed leaf to its baseline value.
pub fn restore_settings(original: &str, baseline: &WtBaseline) -> Result<String> {
    let root = CstRootNode::parse(original, &parse_options())
        .map_err(|error| KisekiError::adapter("windows-terminal", format!("settings.json: {error}")))?;
    let object = object_root(&root)?;

    restore_owned_array(&object, "schemes", &baseline.schemes)?;
    restore_owned_array(&object, "themes", &baseline.themes)?;

    if let Some(profiles) = object.get("profiles").and_then(|property| property.object_value()) {
        if let Some(defaults) = profiles.get("defaults").and_then(|property| property.object_value()) {
            // --- 1. Leaf-managed objects ---------------------------------
            for (parent, leaves) in MANAGED_NESTED_KEYS {
                // A profile transform (`accessibility`, static, ...) can remove
                // the whole parent object. When the baseline still has a leaf
                // to restore, recreate the parent instead of dropping it.
                let has_baseline_leaf = leaves.iter().any(|leaf| {
                    matches!(
                        baseline.defaults.get(&format!("{parent}.{leaf}")),
                        Some(Some(_))
                    )
                });
                let child = match defaults.get(parent).and_then(|property| property.object_value()) {
                    Some(child) => Some(child),
                    None if has_baseline_leaf => Some(child_object(&defaults, parent)?),
                    None => None,
                };
                let Some(child) = child else {
                    continue;
                };
                let mut removed = false;
                for leaf in *leaves {
                    match baseline.defaults.get(&format!("{parent}.{leaf}")) {
                        Some(Some(value)) => set_leaf(&child, leaf, value),
                        Some(None) => removed |= remove_leaf(&child, leaf),
                        // Absent entry: no baseline information, leave as is.
                        None => {}
                    }
                }
                // Only drop the parent when Kiseki created it: an empty object
                // means every remaining property was ours.
                if removed && is_empty_object(&child) {
                    if let Some(property) = defaults.get(parent) {
                        property.remove();
                    }
                }
            }

            // --- 2. Scalar managed keys --------------------------------
            for (key, value) in &baseline.defaults {
                if managed_leaves(key).is_some() || is_nested_leaf_path(key) {
                    continue;
                }
                match value {
                    Some(value) => set_leaf(&defaults, key, value),
                    None => {
                        remove_leaf(&defaults, key);
                    }
                }
            }
        }
    }

    match (&baseline.top_level_theme_present, &baseline.top_level_theme) {
        (true, Some(theme)) => {
            if let Some(property) = object.get("theme") {
                property.set_value(to_cst(theme));
            } else {
                object.append("theme", to_cst(theme));
            }
        }
        _ => {
            if let Some(property) = object.get("theme") {
                property.remove();
            }
        }
    }

    Ok(root.to_string())
}

fn restore_owned_array(object: &CstObject, key: &str, entries: &[Value]) -> Result<()> {
    let Some(property) = object.get(key) else {
        return Ok(());
    };
    let Some(array) = property.array_value() else {
        return Ok(());
    };
    let stale: Vec<_> = array
        .elements()
        .into_iter()
        .filter(|node| {
            node.to_serde_value()
                .and_then(|value| {
                    value
                        .get("name")
                        .and_then(Value::as_str)
                        .map(|name| name.starts_with(OWNED_PREFIX))
                })
                .unwrap_or(false)
        })
        .collect();
    for node in stale {
        node.remove();
    }
    for entry in entries {
        array.append(to_cst(entry));
    }
    Ok(())
}

/// Scans `profiles.list[*]` for per-profile visual overrides.
pub fn detect_profile_conflicts(settings: &str) -> Vec<ProfileConflict> {
    let result = match jsonc_parser::parse_to_ast(
        settings,
        &CollectOptions {
            comments: jsonc_parser::CommentCollectionStrategy::Off,
            tokens: false,
        },
        &parse_options(),
    ) {
        Ok(result) => result,
        Err(_) => return Vec::new(),
    };
    let Some(root) = result.value.as_ref().and_then(|value| value.as_object()) else {
        return Vec::new();
    };
    let Some(profiles) = root
        .get("profiles")
        .and_then(|property| property.value.as_object())
    else {
        return Vec::new();
    };
    let Some(list) = profiles
        .get("list")
        .and_then(|property| property.value.as_array())
    else {
        return Vec::new();
    };

    let mut conflicts = Vec::new();
    for profile in &list.elements {
        let Some(profile) = profile.as_object() else {
            continue;
        };
        let name = profile
            .get_string("name")
            .map(|name| name.value.to_string())
            .unwrap_or_else(|| "<unnamed>".to_string());
        for key in ["colorScheme", "backgroundImage", "font", "cursorShape"] {
            if let Some(property) = profile.get(key) {
                conflicts.push(ProfileConflict {
                    profile: name.clone(),
                    key: key.to_string(),
                    line: line_of(settings, property.range.start),
                });
            }
        }
    }
    conflicts
}

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn count_comments(text: &str) -> usize {
    text.match_indices("//").count() + text.match_indices("/*").count()
}

/// Marker constants, re-exported for tests and the transaction engine.
pub const fn markers() -> (&'static str, &'static str) {
    (MARKER_BEGIN, MARKER_END)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_compiler::windows_terminal::build;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_types::RenderingProfile;

    const SETTINGS: &str = r##"{
  // Managed by hand; do not lose this comment.
  "$schema": "https://aka.ms/terminal-profiles-schema",
  "defaultProfile": "{00000000-0000-0000-0000-000000000001}",
  "profiles": {
    "defaults": {
      // PowerShell 7
      "font": { "face": "Cascadia Code", "size": 12 }
    },
    "list": [
      {
        "name": "PowerShell",
        "colorScheme": "One Half Dark" // pinned by the user
      },
      {
        "name": "WSL",
        "backgroundImage": "C:\\wall.png"
      }
    ]
  },
  "schemes": [
    { "name": "One Half Dark", "background": "#282C34" }
  ],
  "theme": "system",
  "keybindings": [
    { "keys": "ctrl+c", "command": "copy" }
  ] /* trailing block comment */
}
"##;

    fn payload() -> WtVisualPayload {
        let pack = ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
        };
        let theme = ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced);
        build(
            &theme,
            &["JetBrainsMono Nerd Font".to_string()],
            Some(Path::new("/data/themes/nerv-eva/assets/wallpaper.png")),
            Some(Path::new("/data/generated/shaders/active.hlsl")),
        )
    }

    #[test]
    fn patch_preserves_comments_key_order_and_trailing_commas() {
        let patched = patch_settings(SETTINGS, &payload()).unwrap();
        // Comments survive verbatim.
        assert!(patched.contains("// Managed by hand; do not lose this comment."));
        assert!(patched.contains("// PowerShell 7"));
        assert!(patched.contains("// pinned by the user"));
        assert!(patched.contains("/* trailing block comment */"));
        // Key order survives.
        let schema = patched.find("\"$schema\"").unwrap();
        let default_profile = patched.find("\"defaultProfile\"").unwrap();
        let profiles = patched.find("\"profiles\"").unwrap();
        assert!(schema < default_profile && default_profile < profiles);
        // Unrelated keys and values survive byte-for-byte.
        assert!(patched.contains("\"keys\": \"ctrl+c\""));
        assert!(patched.contains("\"One Half Dark\""));
        assert!(patched.contains("\"defaultProfile\": \"{00000000-0000-0000-0000-000000000001}\""));
        // Patch applies.
        assert!(patched.contains("\"colorScheme\": \"Kiseki: NERV UNIT-02\""));
        assert!(patched.contains("\"theme\": \"Kiseki: NERV UNIT-02\""));
        assert!(patched.contains("\"cursorShape\": \"filledBox\""));
        assert!(patched.contains("\"experimental.pixelShaderPath\": \"/data/generated/shaders/active.hlsl\""));
        // Still valid JSONC.
        parse_value(&patched).unwrap();
    }

    #[test]
    fn patch_never_adds_or_modifies_forbidden_keys() {
        let original = parse_value(SETTINGS).unwrap();
        let patched = parse_value(&patch_settings(SETTINGS, &payload()).unwrap()).unwrap();
        for forbidden in FORBIDDEN_KEYS {
            assert_eq!(
                patched.get(*forbidden),
                original.get(*forbidden),
                "{forbidden} must be preserved exactly"
            );
        }
        // The payload itself never carries a forbidden key.
        let payload = payload();
        for forbidden in FORBIDDEN_KEYS {
            assert!(!payload.defaults.contains_key(*forbidden));
        }
    }

    #[test]
    fn trailing_commas_and_blank_lines_survive() {
        let settings = r##"{
  // leading comment
  "profiles": {
    "defaults": {
      // font comment
      "font": { "face": "Cascadia Code", "size": 12 },
    },
    "list": [],
  },
  "schemes": [
    { "name": "One Half Dark", "background": "#282C34" },
  ],
}
"##;
        let patched = patch_settings(settings, &payload()).unwrap();
        assert!(patched.contains("// leading comment"));
        assert!(patched.contains("// font comment"));
        // The user's trailing commas are still accepted by the parser and the
        // original lines are not rewritten.
        assert!(patched.contains("\"list\": [],"));
        parse_value(&patched).unwrap();
    }

    #[test]
    fn patch_is_idempotent_for_owned_entries() {
        let once = patch_settings(SETTINGS, &payload()).unwrap();
        let twice = patch_settings(&once, &payload()).unwrap();
        assert_eq!(once.matches("\"name\": \"Kiseki: NERV UNIT-02\"").count(), 2);
        assert_eq!(twice.matches("\"name\": \"Kiseki: NERV UNIT-02\"").count(), 2);
        assert!(twice.contains("// Managed by hand"));
    }

    #[test]
    fn sub_profile_conflicts_are_reported_with_line_numbers() {
        let conflicts = detect_profile_conflicts(SETTINGS);
        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts[0].profile, "PowerShell");
        assert_eq!(conflicts[0].key, "colorScheme");
        assert!(conflicts[0].line > 1);
        assert_eq!(conflicts[1].profile, "WSL");
        assert_eq!(conflicts[1].key, "backgroundImage");
        let diagnostic = conflicts[0].to_diagnostic();
        assert!(diagnostic.message.contains("PowerShell"));
        assert!(diagnostic.message.contains("colorScheme"));
    }

    #[test]
    fn baseline_restore_reverts_exactly() {
        let baseline = capture_baseline(SETTINGS, Path::new("/tmp/settings.json"), "2026-11-26T09:15:00Z");
        let patched = patch_settings(SETTINGS, &payload()).unwrap();
        let restored = restore_settings(&patched, &baseline).unwrap();

        let original_value = parse_value(SETTINGS).unwrap();
        let restored_value = parse_value(&restored).unwrap();
        assert_eq!(restored_value["profiles"]["defaults"], original_value["profiles"]["defaults"]);
        assert_eq!(restored_value["theme"], original_value["theme"]);
        // Only the user's own scheme remains.
        let schemes = restored_value["schemes"].as_array().unwrap();
        assert_eq!(schemes.len(), 1);
        assert_eq!(schemes[0]["name"], "One Half Dark");
        // Comments are still there.
        assert!(restored.contains("// Managed by hand"));
        assert!(restored.contains("/* trailing block comment */"));
    }

    #[test]
    fn baseline_captures_absent_keys_as_none() {
        let baseline = capture_baseline("{}", Path::new("/tmp/settings.json"), "now");
        assert!(baseline.defaults.values().all(Option::is_none));
        assert!(!baseline.top_level_theme_present);
        assert!(baseline.schemes.is_empty());
        // Object-valued keys are captured per managed leaf.
        assert!(baseline.defaults.contains_key("font.face"));
        assert!(baseline.defaults.contains_key("unfocusedAppearance.opacity"));
    }

    #[test]
    fn patch_refuses_a_non_object_root() {
        let error = patch_settings("[1, 2, 3]", &payload()).unwrap_err();
        assert!(error.to_string().contains("root must be a JSON object"), "{error}");
    }

    #[test]
    fn patch_refuses_to_replace_a_non_array_schemes_value() {
        let error = patch_settings(r#"{ "schemes": { "name": "x" } }"#, &payload()).unwrap_err();
        assert!(error.to_string().contains("not an array"), "{error}");
    }

    #[test]
    fn patch_rejects_unmanaged_payload_keys() {
        let mut payload = payload();
        payload
            .defaults
            .insert("scrollbarState".to_string(), Value::String("hidden".to_string()));
        let error = patch_settings(SETTINGS, &payload).unwrap_err();
        assert!(error.to_string().contains("not in the managed whitelist"), "{error}");
    }

    #[test]
    fn absent_managed_keys_are_removed_from_the_patched_settings() {
        // A full `balanced` patch first, then the same payload stripped the way
        // `accessibility` / `recover --safe` strip it.
        let full = patch_settings(SETTINGS, &payload()).unwrap();
        let mut lean = payload();
        for key in [
            "backgroundImage",
            "backgroundImageOpacity",
            "backgroundImageAlignment",
            "backgroundImageStretchMode",
            "experimental.pixelShaderPath",
            "experimental.pixelShaderImagePath",
            "unfocusedAppearance",
        ] {
            lean.defaults.remove(key);
        }
        let lean = patch_settings(&full, &lean).unwrap();
        let value = parse_value(&lean).unwrap();
        let defaults = &value["profiles"]["defaults"];
        for key in [
            "backgroundImage",
            "backgroundImageOpacity",
            "backgroundImageAlignment",
            "backgroundImageStretchMode",
            "experimental.pixelShaderPath",
            "experimental.pixelShaderImagePath",
            "unfocusedAppearance",
        ] {
            assert!(defaults.get(key).is_none(), "{key} must be removed: {defaults}");
        }
        // Managed keys that are still present survive.
        assert_eq!(defaults["colorScheme"], "Kiseki: NERV UNIT-02");
        assert!(defaults["font"]["face"].is_string());
        // Nothing is dropped from the user's profile list.
        assert_eq!(value["profiles"]["list"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn leaf_patching_preserves_user_siblings_and_comments() {
        let settings = r##"{
  "profiles": {
    "defaults": {
      // my font tweaks
      "font": {
        "face": "Cascadia Code",
        // feature settings must survive
        "features": { "calt": true },
        "axes": { "wght": 450 }
      },
      "unfocusedAppearance": {
        "opacity": 40,
        "cursorShape": "outline"
      }
    },
    "list": []
  }
}
"##;
        let patched = patch_settings(settings, &payload()).unwrap();
        let value = parse_value(&patched).unwrap();
        let defaults = &value["profiles"]["defaults"];
        assert_eq!(defaults["font"]["face"], "JetBrainsMono Nerd Font");
        assert_eq!(defaults["font"]["size"], 11.0);
        assert_eq!(defaults["font"]["weight"], "normal");
        // Siblings and their comments survive untouched.
        assert_eq!(defaults["font"]["features"]["calt"], true);
        assert_eq!(defaults["font"]["axes"]["wght"], 450);
        assert!(patched.contains("// feature settings must survive"), "{patched}");
        assert!(patched.contains("// my font tweaks"), "{patched}");
        assert_eq!(defaults["unfocusedAppearance"]["opacity"], 88);
        assert_eq!(defaults["unfocusedAppearance"]["cursorShape"], "outline");

        // Restoring puts the leaves back without touching the siblings.
        let baseline = capture_baseline(settings, Path::new("/tmp/settings.json"), "now");
        let restored = restore_settings(&patched, &baseline).unwrap();
        let value = parse_value(&restored).unwrap();
        let defaults = &value["profiles"]["defaults"];
        assert_eq!(defaults["font"]["face"], "Cascadia Code");
        assert!(defaults["font"].get("size").is_none());
        assert!(defaults["font"].get("weight").is_none());
        assert_eq!(defaults["font"]["features"]["calt"], true);
        assert_eq!(defaults["unfocusedAppearance"]["opacity"], 40);
        assert_eq!(defaults["unfocusedAppearance"]["cursorShape"], "outline");
        assert!(restored.contains("// feature settings must survive"), "{restored}");
    }

    #[test]
    fn restoring_recreates_a_nested_parent_pruned_by_accessibility() {
        let settings = r##"{
  "profiles": {
    "defaults": {
      "unfocusedAppearance": { "opacity": 40 }
    },
    "list": []
  }
}
"##;
        let baseline = capture_baseline(settings, Path::new("/tmp/settings.json"), "now");

        // `accessibility` drops transparency, so the payload no longer carries
        // `unfocusedAppearance` and the patch prunes the now-empty object.
        let mut accessible = payload();
        accessible.defaults.remove("unfocusedAppearance");
        let patched = patch_settings(settings, &accessible).unwrap();
        let value = parse_value(&patched).unwrap();
        assert!(
            value["profiles"]["defaults"]
                .get("unfocusedAppearance")
                .is_none(),
            "{patched}"
        );

        // Uninstall must restore the user's original leaf, not lose it because
        // the parent object no longer exists.
        let restored = restore_settings(&patched, &baseline).unwrap();
        let value = parse_value(&restored).unwrap();
        assert_eq!(value["profiles"]["defaults"]["unfocusedAppearance"]["opacity"], 40);
        assert_eq!(
            baseline.defaults.get("unfocusedAppearance.opacity"),
            Some(&Some(Value::from(40)))
        );
    }

    #[test]
    fn objects_created_by_the_patch_are_removed_again() {
        let settings = r#"{ "profiles": { "defaults": {}, "list": [] } }"#;
        let patched = patch_settings(settings, &payload()).unwrap();
        let mut lean = payload();
        lean.defaults.remove("unfocusedAppearance");
        let lean = patch_settings(&patched, &lean).unwrap();
        let value = parse_value(&lean).unwrap();
        assert!(value["profiles"]["defaults"].get("unfocusedAppearance").is_none());
        // The font object is still managed, so it stays.
        assert!(value["profiles"]["defaults"].get("font").is_some());

        let baseline = capture_baseline(settings, Path::new("/tmp/settings.json"), "now");
        let restored = restore_settings(&lean, &baseline).unwrap();
        let value = parse_value(&restored).unwrap();
        assert!(value["profiles"]["defaults"].get("font").is_none(), "{value}");
        assert_eq!(value["profiles"]["defaults"], Value::Object(Default::default()));
    }

    #[test]
    fn baseline_captures_leaf_paths_only() {
        let baseline = capture_baseline(SETTINGS, Path::new("/tmp/settings.json"), "now");
        assert_eq!(baseline.schema_version, WT_BASELINE_SCHEMA_VERSION);
        assert_eq!(
            baseline.defaults.get("font.face"),
            Some(&Some(Value::String("Cascadia Code".to_string())))
        );
        assert_eq!(baseline.defaults.get("font.size"), Some(&Some(Value::from(12))));
        assert_eq!(baseline.defaults.get("font.weight"), Some(&None));
        assert_eq!(baseline.defaults.get("unfocusedAppearance.opacity"), Some(&None));
        assert!(!baseline.defaults.contains_key("font"));
    }

    #[test]
    fn an_older_baseline_layout_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let platform = kiseki_platform::MockPlatform::new(dir.path());
        let path = dir.path().join("wt-baseline.json");
        platform
            .write_preserving_symlinks(
                &path,
                br#"{"schemaVersion":1,"createdAt":"now","source":"/tmp/settings.json","defaults":{},"topLevelThemePresent":false,"topLevelTheme":null}"#,
            )
            .unwrap();
        assert_eq!(WtBaseline::load(&path, &platform).unwrap(), None);
    }

    #[test]
    fn empty_settings_file_becomes_an_object() {
        let patched = patch_settings("", &payload()).unwrap();
        let value = parse_value(&patched).unwrap();
        assert!(value.get("schemes").is_some());
        assert!(value.get("profiles").is_some());
    }

    #[test]
    fn detect_uses_env_override_and_reports_installed() {
        let dir = tempfile::tempdir().unwrap();
        let platform = kiseki_platform::MockPlatform::new(dir.path());
        let settings = dir.path().join("settings.json");
        platform.write_preserving_symlinks(&settings, SETTINGS.as_bytes()).unwrap();
        platform.set_env("KISEKI_WT_SETTINGS", &settings.to_string_lossy());
        let detection = WindowsTerminalAdapter
            .detect(&EnvContext::local(), &platform)
            .unwrap();
        assert!(detection.installed);
        assert_eq!(detection.path_kind, PathKind::Regular);
        assert!(detection.notes.iter().any(|note| note.contains("PowerShell")));
    }

    #[test]
    fn validate_accepts_a_comment_preserving_patch() {
        let dir = tempfile::tempdir().unwrap();
        let platform = kiseki_platform::MockPlatform::new(dir.path());
        let settings = dir.path().join("settings.json");
        platform.write_preserving_symlinks(&settings, SETTINGS.as_bytes()).unwrap();
        let mut plan = AdapterPlan::new("windows-terminal");
        plan.push(FileOperation::PatchWindowsTerminal {
            target: settings,
            payload: payload(),
            baseline: dir.path().join("wt-baseline.json"),
        });
        let report = WindowsTerminalAdapter.validate(&plan, &platform).unwrap();
        assert!(!report.has_fatal(), "{:?}", report.diagnostics);
    }
}
