//! Transaction journal: manifest, per-file backups and SHA-256 fingerprints.
//!
//! A journal directory is written *before* any mutation, so even a hard crash
//! leaves enough information for `kiseki rollback <tx-id>` to reconstruct the
//! previous state of every file the transaction intended to touch.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kiseki_adapters::FileOperation;
use kiseki_platform::Platform;
use kiseki_state::State;
use kiseki_types::{KisekiError, RenderingProfile, Result};
use serde::{Deserialize, Serialize};

use crate::{sha256_hex, TargetResult, TargetStatus};

/// Version of `manifest.json`.
pub const JOURNAL_SCHEMA_VERSION: u32 = 1;

/// One backed-up file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub target: PathBuf,
    pub adapter: String,
    /// False means the file did not exist before the transaction.
    pub existed: bool,
    /// Path relative to the journal directory (`backups/0001-zshrc`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256_after: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl JournalEntry {
    fn backup_path(&self, journal_dir: &Path) -> Option<PathBuf> {
        self.backup
            .as_ref()
            .map(|relative| journal_dir.join(relative))
    }
}

/// `journal/<tx-id>/manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalManifest {
    pub schema_version: u32,
    pub tx_id: String,
    pub created_at: String,
    pub command: String,
    pub theme: String,
    pub profile: RenderingProfile,
    pub render_only: bool,
    pub state_before: State,
    pub state_after: State,
    pub entries: Vec<JournalEntry>,
}

impl JournalManifest {
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|error| KisekiError::Json(error.to_string()))
    }

    pub fn from_json(raw: &str) -> Result<Self> {
        serde_json::from_str(raw).map_err(|error| KisekiError::Json(error.to_string()))
    }
}

/// Lightweight view for `kiseki rollback --list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalSummary {
    pub tx_id: String,
    pub created_at: String,
    pub command: String,
    pub theme: String,
    pub profile: RenderingProfile,
    pub entries: usize,
    pub path: PathBuf,
}

/// Allocates the next transaction id from the wall clock plus a sequence.
pub fn next_tx_id(platform: &dyn Platform, paths: &kiseki_platform::KisekiPaths) -> Result<String> {
    let now = platform.now_rfc3339();
    let stamp = compact_timestamp(&now);
    let existing = platform.list_dir(&paths.journal_dir())?;
    let count = existing
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&format!("{stamp}-tx")))
        })
        .count();
    Ok(format!("{stamp}-tx{:04}", count + 1))
}

fn compact_timestamp(now: &str) -> String {
    // `2026-11-26T09:15:00Z` -> `20261126-091500`
    let cleaned: String = now
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '-' || *ch == 'T')
        .collect();
    if let Some((date, time)) = cleaned.split_once('T') {
        let time = time.trim_end_matches('Z');
        let date = date.replace('-', "");
        let time = time.replace(':', "");
        return format!("{date}-{time}");
    }
    cleaned.replace(['-', ':'], "")
}

/// Transaction metadata needed to render a manifest.
pub struct ManifestContext<'a> {
    pub command: &'a str,
    pub theme: &'a str,
    pub profile: RenderingProfile,
    pub render_only: bool,
    pub state_before: &'a State,
    pub state_after: &'a State,
}

/// Captures every target and writes its backup.
pub fn build_manifest(
    platform: &dyn Platform,
    paths: &kiseki_platform::KisekiPaths,
    tx_id: &str,
    context: &ManifestContext<'_>,
    plans: &[kiseki_adapters::AdapterPlan],
) -> Result<JournalManifest> {
    let journal_dir = paths.journal_dir().join(tx_id);
    platform.create_dir_all(&journal_dir.join("backups"))?;

    // Deduplicate targets while remembering which adapter planned them.
    let mut targets: Vec<(PathBuf, String)> = Vec::new();
    for plan in plans {
        for operation in &plan.operations {
            let target = crate::resolve_target(paths, operation);
            if !targets.iter().any(|(existing, _)| existing == &target) {
                targets.push((target, plan.adapter.to_string()));
            }
        }
    }

    let mut entries = Vec::new();
    for (index, (target, adapter)) in targets.into_iter().enumerate() {
        let mut notes = Vec::new();
        let (existed, bytes) = match platform.read_bytes(&target) {
            Ok(Some(bytes)) => (true, Some(bytes)),
            Ok(None) => (false, None),
            Err(error) => {
                notes.push(format!("unreadable before transaction: {error}"));
                (true, None)
            }
        };
        let backup = if let Some(bytes) = bytes.as_ref() {
            let name = target
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("target");
            let relative = PathBuf::from("backups").join(format!("{index:04}-{name}"));
            platform.write_preserving_symlinks(&journal_dir.join(&relative), bytes)?;
            Some(relative)
        } else {
            None
        };
        entries.push(JournalEntry {
            target,
            adapter,
            existed,
            backup,
            sha256_before: bytes.as_deref().map(sha256_hex),
            sha256_after: None,
            notes,
        });
    }

    Ok(JournalManifest {
        schema_version: JOURNAL_SCHEMA_VERSION,
        tx_id: tx_id.to_string(),
        created_at: platform.now_rfc3339(),
        command: context.command.to_string(),
        theme: context.theme.to_string(),
        profile: context.profile,
        render_only: context.render_only,
        state_before: context.state_before.clone(),
        state_after: context.state_after.clone(),
        entries,
    })
}

pub fn write_manifest(
    platform: &dyn Platform,
    paths: &kiseki_platform::KisekiPaths,
    manifest: &JournalManifest,
) -> Result<()> {
    let dir = paths.journal_dir().join(&manifest.tx_id);
    platform.create_dir_all(&dir)?;
    platform.write_preserving_symlinks(
        &dir.join("manifest.json"),
        manifest.to_json()?.as_bytes(),
    )?;
    Ok(())
}

/// Fills in `sha256_after` and per-target notes, then rewrites the manifest.
pub fn finalize_manifest(
    platform: &dyn Platform,
    paths: &kiseki_platform::KisekiPaths,
    manifest: &mut JournalManifest,
    targets: &[TargetResult],
) -> Result<()> {
    for entry in &mut manifest.entries {
        entry.sha256_after = platform
            .read_bytes(&entry.target)
            .ok()
            .flatten()
            .as_deref()
            .map(sha256_hex);
        for target in targets.iter().filter(|target| target.target == entry.target) {
            entry.notes.push(format!("{}: {}", target.status.as_str(), target.notes.join(", ")));
            if target.status == TargetStatus::RenderOnly {
                entry.notes.push("host write skipped (render-only)".to_string());
            }
        }
    }
    write_manifest(platform, paths, manifest)
}

pub fn read_manifest(
    platform: &dyn Platform,
    paths: &kiseki_platform::KisekiPaths,
    tx_id: &str,
) -> Result<JournalManifest> {
    let path = paths.journal_dir().join(tx_id).join("manifest.json");
    let raw = platform
        .read_to_string(&path)?
        .ok_or_else(|| KisekiError::State(format!("transaction {tx_id} not found at {}", path.display())))?;
    JournalManifest::from_json(&raw)
}

/// Restores one entry. Returns true when something was restored/removed.
pub fn restore_entry(
    platform: &dyn Platform,
    journal_dir: &Path,
    entry: &JournalEntry,
) -> Result<bool> {
    if let Some(backup) = entry.backup_path(journal_dir) {
        let Some(bytes) = platform.read_bytes(&backup)? else {
            return Err(KisekiError::State(format!(
                "journal backup missing: {}",
                backup.display()
            )));
        };
        match platform.write_preserving_symlinks(&entry.target, &bytes)? {
            kiseki_platform::WriteOutcome::Written { .. } => return Ok(true),
            kiseki_platform::WriteOutcome::SkippedReadOnly { path, reason } => {
                return Err(KisekiError::State(format!(
                    "cannot restore read-only {}: {reason}",
                    path.display()
                )));
            }
        }
    }
    if !entry.existed {
        platform.remove_file(&entry.target)?;
        return Ok(true);
    }
    Ok(false)
}

/// Lists journal summaries, newest first.
pub fn list(
    platform: &dyn Platform,
    paths: &kiseki_platform::KisekiPaths,
) -> Result<Vec<JournalSummary>> {
    let mut summaries = Vec::new();
    let mut dirs = platform.list_dir(&paths.journal_dir())?;
    dirs.sort();
    for dir in dirs.into_iter().rev() {
        if !dir.is_dir() {
            continue;
        }
        let Some(tx_id) = dir.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(manifest) = read_manifest(platform, paths, tx_id) else {
            continue;
        };
        summaries.push(JournalSummary {
            tx_id: manifest.tx_id,
            created_at: manifest.created_at,
            command: manifest.command,
            theme: manifest.theme,
            profile: manifest.profile,
            entries: manifest.entries.len(),
            path: dir,
        });
    }
    Ok(summaries)
}

/// Collects every distinct target path in a plan set (used by tests/tools).
pub fn targets(
    paths: &kiseki_platform::KisekiPaths,
    operations: &[FileOperation],
) -> BTreeSet<PathBuf> {
    operations
        .iter()
        .map(|operation| crate::resolve_target(paths, operation))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_timestamp_is_lexicographically_sortable() {
        assert_eq!(compact_timestamp("2026-11-26T09:15:00Z"), "20261126-091500");
        assert_eq!(compact_timestamp("2026-01-01T00:00:00Z"), "20260101-000000");
    }
}
