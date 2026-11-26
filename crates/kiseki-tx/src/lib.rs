//! Transaction engine: plan, journal, apply, roll back, recover and diagnose
//! (RFC-0003 §6).
//!
//! Every mutation goes through the same six phases:
//! 1. acquire `<kiseki_config>/.lock`;
//! 2. detect + plan every adapter (pure);
//! 3. validate the complete plan (fail fast, zero writes);
//! 4. create `journal/<tx-id>/manifest.json` and back up every target;
//! 5. apply generated artefacts first, then host anchors/patches per target,
//!    rolling back only the target that failed;
//! 6. commit `state.json` + `state.gen`, reload healthy adapters and report.

pub mod doctor;
pub mod journal;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kiseki_adapters::{
    AdapterPlan, AdapterRegistry, DetectionReport, FileOperation, PlanContext, PlanOptions,
    ValidationReport,
};
use kiseki_compiler::marker;
use kiseki_adapters::{
    capture_baseline, patch_settings, restore_settings, WtBaseline,
};
use kiseki_compiler::ArtifactMap;
use kiseki_platform::{PathKind, Platform, ReloadOutcome, WriteOutcome, LOCK_TIMEOUT};
use kiseki_schema::ResolvedTheme;
use kiseki_state::{PreviewSession, State};
use kiseki_types::{
    Diagnostic, DiagnosticReport, EnvContext, KisekiError, PromptPolicy, RenderingProfile, Result,
};
use sha2::{Digest, Sha256};

pub use doctor::{doctor, DoctorCheck, DoctorCheckStatus, DoctorReport};
pub use journal::{JournalEntry, JournalManifest, JournalSummary, JOURNAL_SCHEMA_VERSION};

/// Everything that influences a transaction but is not theme data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionOptions {
    /// Human command that produced the transaction (`use nerv-eva`).
    pub command: String,
    pub render_only: bool,
    pub dry_run: bool,
    pub only: Vec<String>,
    /// True when `only` is authoritative (an explicit `--only` or a configured
    /// `enabledAdapters` list), so an empty list runs zero adapters.
    pub explicit_only: bool,
    pub aliases: bool,
    pub prompt_policy: PromptPolicy,
    pub allow_create_host_files: bool,
    /// When true (used by `recover --safe`), any failed/rolled-back target or
    /// unexpected host `RenderOnly` downgrade aborts before committing state
    /// and before reloading adapters so a live shader is never masked as static.
    pub require_all_targets: bool,
    pub lock_timeout: Duration,
}

impl Default for TransactionOptions {
    fn default() -> Self {
        Self {
            command: "use".to_string(),
            render_only: false,
            dry_run: false,
            only: Vec::new(),
            explicit_only: false,
            aliases: false,
            prompt_policy: PromptPolicy::ManagedTemplate,
            allow_create_host_files: false,
            require_all_targets: false,
            lock_timeout: LOCK_TIMEOUT,
        }
    }
}

impl TransactionOptions {
    fn plan_options(&self) -> PlanOptions {
        PlanOptions {
            render_only: self.render_only,
            aliases: self.aliases,
            prompt_policy: self.prompt_policy,
            allow_create_host_files: self.allow_create_host_files,
            only: self.only.clone(),
            explicit_only: self.explicit_only,
        }
    }
}

/// One transaction request.
pub struct ApplyRequest<'a> {
    pub theme: &'a ResolvedTheme,
    pub artifacts: &'a ArtifactMap,
    pub state_before: State,
    pub state_after: State,
    pub env: &'a EnvContext,
    pub options: TransactionOptions,
}

/// Final status of one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetStatus {
    Applied,
    RenderOnly,
    Skipped,
    DryRun,
    RolledBack,
    Failed,
}

impl TargetStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::RenderOnly => "render-only",
            Self::Skipped => "skipped",
            Self::DryRun => "dry-run",
            Self::RolledBack => "rolled-back",
            Self::Failed => "failed",
        }
    }
}

/// Per-target outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetResult {
    pub adapter: String,
    pub target: PathBuf,
    pub status: TargetStatus,
    pub notes: Vec<String>,
}

/// Summary returned to the CLI.
#[derive(Debug, Clone, PartialEq)]
pub struct TransactionReport {
    pub tx_id: String,
    pub dry_run: bool,
    pub render_only: bool,
    pub targets: Vec<TargetResult>,
    pub diagnostics: DiagnosticReport,
    pub state: State,
    pub journal_dir: PathBuf,
    pub reload: Vec<(String, String)>,
}

impl TransactionReport {
    pub fn applied(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| target.status == TargetStatus::Applied)
            .count()
    }

    pub fn render_only(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| target.status == TargetStatus::RenderOnly)
            .count()
    }

    pub fn failures(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| matches!(target.status, TargetStatus::Failed | TargetStatus::RolledBack))
            .count()
    }

    pub fn has_failures(&self) -> bool {
        self.failures() > 0
    }
}

/// Report returned by `rollback`.
#[derive(Debug, Clone, PartialEq)]
pub struct RollbackReport {
    pub tx_id: String,
    pub restored: Vec<PathBuf>,
    pub state: State,
}

/// The transaction engine.
pub struct TransactionManager<'a> {
    platform: &'a dyn Platform,
    registry: AdapterRegistry,
}

impl<'a> TransactionManager<'a> {
    pub fn new(platform: &'a dyn Platform, registry: AdapterRegistry) -> Self {
        Self { platform, registry }
    }

    /// Runs the full transaction.
    pub fn apply(&self, request: ApplyRequest<'_>) -> Result<TransactionReport> {
        let paths = self.platform.paths();
        // `--dry-run` promises zero filesystem mutations. Acquiring the lock
        // creates `<config>/.lock` (and `<config>/` itself), so a preview must
        // never touch it.
        let lock = if request.options.dry_run {
            None
        } else {
            Some(
                self.platform
                    .acquire_lock(&paths.lock_file(), request.options.lock_timeout)?,
            )
        };

        // --- 0. Authoritative state under the lock ---------------------------
        // A concurrent process may have committed while we were planning, so
        // the generation we bump must start from what is actually on disk. A
        // corrupt document is tolerated here: `recover --safe` exists to
        // rewrite it from the planner's `state_after`.
        let locked_state = State::load(self.platform, paths).ok().flatten();
        let gen_file_floor = State::read_gen(self.platform, paths)
            .ok()
            .flatten()
            .unwrap_or(0);
        let state_before = locked_state
            .as_ref()
            .filter(|locked| locked.generation > request.state_before.generation)
            .cloned()
            .unwrap_or_else(|| request.state_before.clone());
        let generation_floor = locked_state
            .as_ref()
            .map(|locked| locked.generation)
            .unwrap_or(0)
            .max(gen_file_floor);

        // --- 1. Detect + plan ------------------------------------------------
        let plan_options = request.options.plan_options();
        let detections = self.detect(&request, &plan_options);
        let (mut plans, diagnostics) = self.plan(&request, &detections, &plan_options);
        // Every compiled artefact must reach `generated/`, even when the host
        // that consumes it is not detected (cross-platform packs, `--only`,
        // `--render-only`). Adapters may already declare some of these; the
        // baseline plan only fills the gaps.
        if let Some(baseline) = generated_baseline(&plans, request.artifacts) {
            plans.push(baseline);
        }
        if diagnostics.has_fatal() {
            return Err(KisekiError::transaction(
                "pending",
                format!(
                    "validation failed before any write: {}",
                    diagnostics
                        .iter()
                        .filter(|diagnostic| diagnostic.is_fatal())
                        .map(|diagnostic| diagnostic.message.clone())
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            ));
        }

        // --- 2. Validate fail-fast ------------------------------------------
        for plan in &plans {
            let Some(adapter) = self.registry.by_id(plan.adapter) else {
                continue;
            };
            let report = adapter.validate(plan, self.platform)?;
            if report.has_fatal() {
                return Err(KisekiError::transaction(
                    "pending",
                    format!(
                        "adapter {} failed validation: {}",
                        plan.adapter,
                        summarize(&report)
                    ),
                ));
            }
        }

        let tx_id = journal::next_tx_id(self.platform, paths)?;
        if request.options.dry_run {
            let targets = plans
                .iter()
                .flat_map(|plan| {
                    plan.operations.iter().map(move |operation| TargetResult {
                        adapter: plan.adapter.to_string(),
                        target: resolve_target(paths, operation),
                        status: TargetStatus::DryRun,
                        notes: vec![operation.label()],
                    })
                })
                .collect();
            drop(lock);
            return Ok(TransactionReport {
                tx_id: tx_id.clone(),
                dry_run: true,
                render_only: request.options.render_only,
                targets,
                diagnostics,
                state: request.state_after.clone(),
                journal_dir: paths.journal_dir().join(tx_id),
                reload: Vec::new(),
            });
        }

        // --- 3. Journal + backups -------------------------------------------
        let mut manifest = journal::build_manifest(
            self.platform,
            paths,
            &tx_id,
            &journal::ManifestContext {
                command: &request.options.command,
                theme: request.theme.id.as_str(),
                profile: request.theme.profile,
                render_only: request.options.render_only,
                state_before: &state_before,
                state_after: &request.state_after,
            },
            &plans,
        )?;
        journal::write_manifest(self.platform, paths, &manifest)?;

        // --- 4. Apply: generated first, then host targets --------------------
        let mut targets = Vec::new();
        let mut rollback_records = Vec::new();

        for plan in &plans {
            for operation in plan.operations.iter().filter(|operation| !operation.is_host_write()) {
                let target = resolve_target(paths, operation);
                let (mut status, notes) = self.apply_generated(operation);
                if status == TargetStatus::Failed {
                    // Generated files live under Kiseki's own directory, but a
                    // failed atomic write can still have removed a Windows
                    // destination; restore from the same journal backup.
                    if let Some(entry) = manifest
                        .entries
                        .iter()
                        .find(|entry| entry.target == target)
                    {
                        let _ = journal::restore_entry(
                            self.platform,
                            &paths.journal_dir().join(&tx_id),
                            entry,
                        );
                        status = TargetStatus::RolledBack;
                    }
                }
                targets.push(TargetResult {
                    adapter: plan.adapter.to_string(),
                    target: target.clone(),
                    status,
                    notes,
                });
            }
        }

        for plan in &plans {
            let Some(detection) = detections.get(plan.adapter) else {
                continue;
            };
            // A read-only host has no host operation at all, so surface the
            // render-only downgrade explicitly in the report.
            if plan.render_only && plan.host_writes().count() == 0 && detection.is_read_only() {
                for (path, kind) in detection.all_config_paths() {
                    if matches!(kind, PathKind::ReadOnlyFile | PathKind::ReadOnlySymlink(_)) {
                        targets.push(TargetResult {
                            adapter: plan.adapter.to_string(),
                            target: path.to_path_buf(),
                            status: TargetStatus::RenderOnly,
                            notes: plan
                                .diagnostics
                                .iter()
                                .map(|diagnostic| diagnostic.message.clone())
                                .collect(),
                        });
                    }
                }
            }
            for operation in plan.operations.iter().filter(|operation| operation.is_host_write()) {
                let target = resolve_target(paths, operation);
                let backup = manifest
                    .entries
                    .iter()
                    .find(|entry| entry.target == target)
                    .cloned();
                match self.apply_host(operation) {
                    Ok(HostApply::Applied(note)) => targets.push(TargetResult {
                        adapter: plan.adapter.to_string(),
                        target,
                        status: TargetStatus::Applied,
                        notes: vec![note],
                    }),
                    Ok(HostApply::RenderOnly(reason)) => {
                        targets.push(TargetResult {
                            adapter: plan.adapter.to_string(),
                            target,
                            status: TargetStatus::RenderOnly,
                            notes: vec![reason],
                        });
                    }
                    Ok(HostApply::Skipped(reason)) => targets.push(TargetResult {
                        adapter: plan.adapter.to_string(),
                        target,
                        status: TargetStatus::Skipped,
                        notes: vec![reason],
                    }),
                    Err(error) => {
                        let restored = backup
                            .as_ref()
                            .map(|entry| {
                            journal::restore_entry(self.platform, &paths.journal_dir().join(&tx_id), entry)
                        })
                            .transpose()?
                            .is_some();
                        rollback_records.push(target.clone());
                        targets.push(TargetResult {
                            adapter: plan.adapter.to_string(),
                            target,
                            status: if restored {
                                TargetStatus::RolledBack
                            } else {
                                TargetStatus::Failed
                            },
                            notes: vec![error.to_string()],
                        });
                        let _ = detection;
                    }
                }
            }
        }

        // --- 5. Strict target gate (`recover --safe`) + commit state ---------
        if request.options.require_all_targets {
            let has_blocking = targets.iter().any(|target| match target.status {
                TargetStatus::Failed | TargetStatus::RolledBack => true,
                TargetStatus::RenderOnly => !request.options.render_only,
                TargetStatus::Applied | TargetStatus::Skipped | TargetStatus::DryRun => false,
            });
            if has_blocking {
                manifest.state_after = state_before.clone();
                journal::finalize_manifest(self.platform, paths, &mut manifest, &targets)?;
                drop(lock);
                return Ok(TransactionReport {
                    tx_id: tx_id.clone(),
                    dry_run: false,
                    render_only: request.options.render_only,
                    targets,
                    diagnostics,
                    state: state_before,
                    journal_dir: paths.journal_dir().join(tx_id),
                    reload: Vec::new(),
                });
            }
        }

        let mut state = request.state_after.clone();
        if state.generation < generation_floor {
            state.generation = generation_floor;
        }
        state.commit(self.platform, paths)?;

        // --- 6. Reload healthy adapters --------------------------------------
        // `--render-only` never touched a host, so there is nothing to reload;
        // signalling a terminal here would be a visible side effect.
        let mut reload = Vec::new();
        for plan in &plans {
            if request.options.render_only {
                break;
            }
            let applied = targets.iter().any(|target| {
                target.adapter == plan.adapter && target.status == TargetStatus::Applied
            });
            if !applied {
                continue;
            }
            let Some(adapter) = self.registry.by_id(plan.adapter) else {
                continue;
            };
            let detection = detections
                .get(plan.adapter)
                .cloned()
                .unwrap_or_else(|| DetectionReport::not_installed(""));
            if !detection.installed && !detection.active_in_session {
                continue;
            }
            let caps = adapter.capabilities(&detection, request.env);
            let outcome = adapter.reload(&detection, caps, self.platform)?;
            reload.push((plan.adapter.to_string(), outcome.detail().to_string()));
            if let ReloadOutcome::Failed { detail } = outcome {
                manifest.entries.iter_mut().for_each(|entry| {
                    if entry.adapter == plan.adapter {
                        entry.notes.push(format!("reload failed: {detail}"));
                    }
                });
            }
        }

        // --- 7. Finalise the journal -----------------------------------------
        manifest.state_after = state.clone();
        journal::finalize_manifest(self.platform, paths, &mut manifest, &targets)?;
        drop(lock);

        Ok(TransactionReport {
            tx_id,
            dry_run: false,
            render_only: request.options.render_only,
            targets,
            diagnostics,
            state,
            journal_dir: paths.journal_dir(),
            reload,
        })
    }

    fn detect(
        &self,
        request: &ApplyRequest<'_>,
        plan_options: &PlanOptions,
    ) -> BTreeMap<String, DetectionReport> {
        let mut detections = BTreeMap::new();
        for (id, result) in self
            .registry
            .detect_all(request.env, self.platform, plan_options)
        {
            if let Ok(detection) = result {
                if detection.installed || detection.config_path.is_some() {
                    detections.insert(id, detection);
                }
            }
        }
        detections
    }

    fn plan(
        &self,
        request: &ApplyRequest<'_>,
        detections: &BTreeMap<String, DetectionReport>,
        plan_options: &PlanOptions,
    ) -> (Vec<AdapterPlan>, DiagnosticReport) {
        let mut plans = Vec::new();
        let mut diagnostics = DiagnosticReport::new();
        for adapter in self.registry.iter() {
            if !plan_options.allows(adapter.id()) {
                continue;
            }
            let Some(detection) = detections.get(adapter.id()) else {
                continue;
            };
            let caps = adapter.capabilities(detection, request.env);
            let context = PlanContext {
                theme: request.theme,
                artifacts: request.artifacts,
                options: plan_options,
                env: request.env,
                paths: self.platform.paths(),
            };
            match adapter.plan(&context, detection, caps) {
                Ok(plan) => {
                    diagnostics.extend(plan.diagnostics.items.clone());
                    plans.push(plan);
                }
                Err(error) => {
                    diagnostics.push(Diagnostic::error(
                        format!("adapter.{}.plan", adapter.id()),
                        error.to_string(),
                    ));
                }
            }
        }
        (plans, diagnostics)
    }

    fn apply_generated(&self, operation: &FileOperation) -> (TargetStatus, Vec<String>) {
        let FileOperation::WriteGenerated { relative, content } = operation else {
            return (TargetStatus::Skipped, vec!["not a generated write".to_string()]);
        };
        let path = self.platform.paths().generated(relative.as_str());
        match self.platform.write_preserving_symlinks(&path, content.as_bytes()) {
            Ok(WriteOutcome::Written { .. }) => (TargetStatus::Applied, Vec::new()),
            // `generated/` is owned by Kiseki, not a user host config, so a
            // read-only destination is a real failure: silently degrading to
            // render-only would leave a stale live shader on disk.
            Ok(WriteOutcome::SkippedReadOnly { reason, .. }) => (TargetStatus::Failed, vec![reason]),
            Err(error) => (TargetStatus::Failed, vec![error.to_string()]),
        }
    }

    fn apply_host(&self, operation: &FileOperation) -> Result<HostApply> {
        match operation {
            FileOperation::WriteGenerated { .. } => Ok(HostApply::Skipped(
                "generated write handled separately".to_string(),
            )),
            FileOperation::UpsertMarkerBlock {
                target,
                begin,
                end,
                body,
            } => {
                let original = self.platform.read_to_string(target)?.unwrap_or_default();
                let updated = marker::upsert_marker_block(&original, begin, end, body);
                self.write_host(target, updated.as_bytes())
            }
            FileOperation::RemoveMarkerBlock { target, begin, end } => {
                let Some(original) = self.platform.read_to_string(target)? else {
                    return Ok(HostApply::Skipped(format!(
                        "{} does not exist",
                        target.display()
                    )));
                };
                let updated = marker::remove_marker_block(&original, begin, end);
                self.write_host(target, updated.as_bytes())
            }
            FileOperation::PatchWindowsTerminal {
                target,
                payload,
                baseline,
            } => {
                let original = self.platform.read_to_string(target)?.unwrap_or_default();
                // Only reuse a baseline captured from *this* file; a stale one
                // (e.g. a different WT settings path) would restore the wrong
                // values on uninstall.
                let reusable = WtBaseline::load(baseline, self.platform)?
                    .filter(|captured| same_file(&captured.source, target));
                if reusable.is_none() {
                    let captured = capture_baseline(
                        &original,
                        &canonicalize_or_self(target),
                        &self.platform.now_rfc3339(),
                    );
                    captured.save(baseline, self.platform)?;
                }
                let patched = patch_settings(&original, payload)?;
                self.write_host(target, patched.as_bytes())
            }
            FileOperation::RestoreWindowsTerminal { target, baseline } => {
                let Some(captured) = WtBaseline::load(baseline, self.platform)?
                    .filter(|captured| same_file(&captured.source, target))
                else {
                    return Ok(HostApply::Skipped(
                        "no Windows Terminal baseline was recorded for this file".to_string(),
                    ));
                };
                let original = self.platform.read_to_string(target)?.unwrap_or_default();
                let restored = restore_settings(&original, &captured)?;
                let outcome = self.write_host(target, restored.as_bytes())?;
                if matches!(outcome, HostApply::Applied(_)) {
                    // Retire the baseline so a later reinstall captures the
                    // user's fresh defaults instead of the pre-Kiseki ones.
                    self.platform.remove_file(baseline)?;
                }
                Ok(outcome)
            }
        }
    }

    fn write_host(&self, target: &Path, contents: &[u8]) -> Result<HostApply> {
        match self.platform.write_preserving_symlinks(target, contents)? {
            WriteOutcome::Written { path, .. } => {
                Ok(HostApply::Applied(format!("wrote {}", path.display())))
            }
            WriteOutcome::SkippedReadOnly { reason, .. } => Ok(HostApply::RenderOnly(reason)),
        }
    }

    /// Applies only host-unhook operations (`uninstall`, v1 cleanup).
    pub fn apply_unhooks(
        &self,
        state: &State,
        plans: Vec<AdapterPlan>,
        command: &str,
        dry_run: bool,
    ) -> Result<TransactionReport> {
        let paths = self.platform.paths();
        // Same contract as `apply`: a dry run must not create the lock file.
        let lock = if dry_run {
            None
        } else {
            Some(self.platform.acquire_lock(&paths.lock_file(), LOCK_TIMEOUT)?)
        };
        let tx_id = journal::next_tx_id(self.platform, paths)?;
        if dry_run {
            let targets = plans
                .iter()
                .flat_map(|plan| {
                    plan.operations.iter().map(move |operation| TargetResult {
                        adapter: plan.adapter.to_string(),
                        target: resolve_target(paths, operation),
                        status: TargetStatus::DryRun,
                        notes: vec![operation.label()],
                    })
                })
                .collect();
            drop(lock);
            return Ok(TransactionReport {
                tx_id: tx_id.clone(),
                dry_run: true,
                render_only: true,
                targets,
                diagnostics: DiagnosticReport::new(),
                state: state.clone(),
                journal_dir: paths.journal_dir().join(tx_id),
                reload: Vec::new(),
            });
        }
        let mut manifest = journal::build_manifest(
            self.platform,
            paths,
            &tx_id,
            &journal::ManifestContext {
                command,
                theme: state.active_theme.as_str(),
                profile: state.rendering.profile,
                render_only: true,
                state_before: state,
                state_after: state,
            },
            &plans,
        )?;
        journal::write_manifest(self.platform, paths, &manifest)?;

        let mut targets = Vec::new();
        for plan in &plans {
            for operation in &plan.operations {
                if !operation.is_host_write() {
                    continue;
                }
                let target = resolve_target(paths, operation);
                let backup = manifest
                    .entries
                    .iter()
                    .find(|entry| entry.target == target)
                    .cloned();
                match self.apply_host(operation) {
                    Ok(HostApply::Applied(note)) => targets.push(TargetResult {
                        adapter: plan.adapter.to_string(),
                        target,
                        status: TargetStatus::Applied,
                        notes: vec![note],
                    }),
                    Ok(HostApply::RenderOnly(reason)) => targets.push(TargetResult {
                        adapter: plan.adapter.to_string(),
                        target,
                        status: TargetStatus::RenderOnly,
                        notes: vec![reason],
                    }),
                    Ok(HostApply::Skipped(reason)) => targets.push(TargetResult {
                        adapter: plan.adapter.to_string(),
                        target,
                        status: TargetStatus::Skipped,
                        notes: vec![reason],
                    }),
                    Err(error) => {
                        if let Some(entry) = backup.as_ref() {
                            let _ = journal::restore_entry(
                                self.platform,
                                &paths.journal_dir().join(&tx_id),
                                entry,
                            );
                        }
                        targets.push(TargetResult {
                            adapter: plan.adapter.to_string(),
                            target,
                            status: TargetStatus::RolledBack,
                            notes: vec![error.to_string()],
                        });
                    }
                }
            }
        }
        journal::finalize_manifest(self.platform, paths, &mut manifest, &targets)?;
        drop(lock);
        Ok(TransactionReport {
            tx_id,
            dry_run: false,
            render_only: true,
            targets,
            diagnostics: DiagnosticReport::new(),
            state: state.clone(),
            journal_dir: paths.journal_dir(),
            reload: Vec::new(),
        })
    }

    /// Rolls a journal entry set back, newest first.
    pub fn rollback(&self, tx_id: &str) -> Result<RollbackReport> {
        let paths = self.platform.paths();
        let _lock = self
            .platform
            .acquire_lock(&paths.lock_file(), LOCK_TIMEOUT)?;
        let manifest = journal::read_manifest(self.platform, paths, tx_id)?;
        let mut restored = Vec::new();
        for entry in manifest.entries.iter().rev() {
            if journal::restore_entry(self.platform, &paths.journal_dir().join(&manifest.tx_id), entry)? {
                restored.push(entry.target.clone());
            }
        }

        // The restored document is authoritative for colours/theme, but the
        // generation must stay strictly monotonic: shell hooks compare
        // `state.gen` against the value they cached, so reusing an old number
        // would silently skip reloading `env.*`.
        //
        // A corrupt `state.json` is exactly what `rollback` is expected to
        // repair, so every generation source is read fault-tolerantly instead
        // of failing the whole rollback.
        let mut state = manifest.state_before.clone();
        let disk_generation = [
            State::load(self.platform, paths)
                .ok()
                .flatten()
                .map(|current| current.generation),
            State::read_gen(self.platform, paths).ok().flatten(),
            Some(manifest.state_before.generation),
            Some(manifest.state_after.generation),
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(0);
        state.generation = disk_generation.saturating_add(1);
        state.updated_at = self.platform.now_rfc3339();
        state.save(self.platform, paths)?;
        state.write_gen(self.platform, paths)?;
        Ok(RollbackReport {
            tx_id: tx_id.to_string(),
            restored,
            state,
        })
    }

    /// Reads the `state_before` snapshot of the newest journal without
    /// restoring anything.
    ///
    /// `recover --safe` uses this when `state.json` is corrupt: recovery must
    /// be able to read a known-good state without mutating host files (and
    /// especially not during `--dry-run`).
    pub fn latest_journal_state(&self) -> Result<Option<State>> {
        let paths = self.platform.paths();
        let Some(summary) = journal::list(self.platform, paths)?.into_iter().next() else {
            return Ok(None);
        };
        let manifest = journal::read_manifest(self.platform, paths, &summary.tx_id)?;
        Ok(Some(manifest.state_before))
    }

    pub fn list_journals(&self) -> Result<Vec<JournalSummary>> {
        journal::list(self.platform, self.platform.paths())
    }

    /// Emergency recovery: abort any preview and force the safe profile.
    ///
    /// Deliberately pure with respect to `state.json`: the caller renders and
    /// mounts the static artefacts *before* committing, so a damaged pack can
    /// never leave a half-recovered state on disk.
    pub fn prepare_safe_state(&self, state: &mut State) -> Result<()> {
        let paths = self.platform.paths();
        let _lock = self
            .platform
            .acquire_lock(&paths.lock_file(), LOCK_TIMEOUT)?;
        let _ = PreviewSession::abort(self.platform, paths)?;
        state.rendering.profile = RenderingProfile::Static;
        state.rendering.crt = false;
        state.rendering.bg_opacity_override = None;
        Ok(())
    }

    /// Emergency recovery: abort any preview, force static, clear CRT/opacity
    /// and commit the recovered state.
    pub fn recover_safe(&self, state: &mut State) -> Result<()> {
        self.prepare_safe_state(state)?;
        state.commit(self.platform, self.platform.paths())?;
        Ok(())
    }
}

enum HostApply {
    Applied(String),
    RenderOnly(String),
    Skipped(String),
}

/// True when two paths point at the same file (canonicalised when possible).
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Resolves a path to its canonical spelling, falling back to the input.
///
/// Baseline sources are stored canonically so a symlinked `settings.json` and
/// its real target compare equal instead of spuriously recapturing a baseline.
fn canonicalize_or_self(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Plans any compiled artefact that no adapter declared itself.
fn generated_baseline(plans: &[AdapterPlan], artifacts: &ArtifactMap) -> Option<AdapterPlan> {
    let declared: std::collections::BTreeSet<&str> = plans
        .iter()
        .flat_map(|plan| plan.generated_writes().map(|(path, _)| path))
        .collect();
    let mut baseline = AdapterPlan::new("generated");
    for (path, content) in artifacts.iter() {
        let path = path.as_str();
        if declared.contains(path) {
            continue;
        }
        if let Ok(relative) = kiseki_compiler::ArtifactPath::new(path) {
            baseline.push(FileOperation::WriteGenerated {
                relative,
                content: content.clone(),
            });
        }
    }
    (!baseline.operations.is_empty()).then_some(baseline)
}

fn summarize(report: &ValidationReport) -> String {
    report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_fatal())
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Resolves an operation to the file it touches.
pub fn resolve_target(paths: &kiseki_platform::KisekiPaths, operation: &FileOperation) -> PathBuf {
    match operation {
        FileOperation::WriteGenerated { relative, .. } => paths.generated(relative.as_str()),
        FileOperation::UpsertMarkerBlock { target, .. }
        | FileOperation::RemoveMarkerBlock { target, .. }
        | FileOperation::PatchWindowsTerminal { target, .. }
        | FileOperation::RestoreWindowsTerminal { target, .. } => target.clone(),
    }
}

/// SHA-256 hex digest used by the journal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// True when a path is one of the read-only kinds that force render-only.
pub fn is_read_only(kind: &PathKind) -> bool {
    matches!(kind, PathKind::ReadOnlyFile | PathKind::ReadOnlySymlink(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_adapters::AdapterRegistry;
    use kiseki_compiler::{compile, CompileInput, CompileOptions};
    use kiseki_platform::MockPlatform;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_state::{Config, State};
    use kiseki_types::ThemeId;

    const GLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.glsl");
    const HLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.hlsl");

    fn pack() -> ThemePack {
        ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
        }
    }

    fn theme() -> ResolvedTheme {
        ResolvedTheme::from_pack(&pack(), RenderingProfile::Balanced)
    }

    fn compiled(config_dir: &std::path::Path) -> kiseki_compiler::CompileOutput {
        let theme = theme();
        compile(
            &CompileInput::new(
                &theme,
                CompileOptions::new(config_dir, "/data/themes/nerv-eva")
                    .with_font_families(vec!["JetBrainsMono Nerd Font".to_string()]),
            )
            .with_glsl(Some(GLSL))
            .with_hlsl(Some(HLSL)),
        )
        .unwrap()
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        platform: MockPlatform,
        zshrc: PathBuf,
        ghostty_config: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let zshrc = dir.path().join("dotfiles").join(".zshrc");
        let ghostty_config = dir.path().join("ghostty").join("config");
        platform.create_dir_all(zshrc.parent().unwrap()).unwrap();
        platform.create_dir_all(ghostty_config.parent().unwrap()).unwrap();
        platform.write_preserving_symlinks(&zshrc, b"# user zshrc\nexport FOO=1\n").unwrap();
        platform
            .write_preserving_symlinks(&ghostty_config, b"# user ghostty\nfont-size = 12\n")
            .unwrap();
        platform.set_env("KISEKI_ZSHRC", &zshrc.to_string_lossy());
        platform.set_env("KISEKI_GHOSTTY_CONFIG", &ghostty_config.to_string_lossy());
        platform.set_env("KISEKI_PWSH_PROFILE", &dir.path().join("profile.ps1").to_string_lossy());
        Fixture {
            _dir: dir,
            platform,
            zshrc,
            ghostty_config,
        }
    }

    fn request<'a>(
        theme: &'a ResolvedTheme,
        artifacts: &'a ArtifactMap,
        state_before: State,
        state_after: State,
        env: &'a EnvContext,
        options: TransactionOptions,
    ) -> ApplyRequest<'a> {
        ApplyRequest {
            theme,
            artifacts,
            state_before,
            state_after,
            env,
            options,
        }
    }

    fn initial_state(platform: &MockPlatform) -> State {
        State::new(ThemeId::new("nerv-eva").unwrap(), &platform.now_rfc3339())
    }

    #[test]
    fn apply_writes_generated_then_anchors_and_commits_state() {
        let fixture = fixture();
        let _config = Config::default();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state_before = initial_state(&fixture.platform);
        let mut state_after = state_before.clone();
        state_after.rendering.profile = RenderingProfile::Ultra;

        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state_before,
                state_after,
                &env,
                TransactionOptions {
                    command: "use nerv-eva".to_string(),
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();

        assert!(!report.has_failures(), "{:?}", report.targets);
        assert!(report.targets.iter().any(|target| target.target.ends_with("generated/env.zsh") && target.status == TargetStatus::Applied));
        assert!(report.targets.iter().any(|target| target.target == fixture.zshrc && target.status == TargetStatus::Applied));
        assert!(report.targets.iter().any(|target| target.target == fixture.ghostty_config && target.status == TargetStatus::Applied));
        assert!(report.state.generation >= 2);
        assert_eq!(State::read_gen(&fixture.platform, fixture.platform.paths()).unwrap(), Some(report.state.generation));

        let zshrc = fixture.platform.read_to_string(&fixture.zshrc).unwrap().unwrap();
        assert!(zshrc.contains("# user zshrc"));
        assert!(zshrc.contains("# >>> kiseki >>>"));
        assert!(zshrc.contains("_kiseki_sync_hook"));
        let ghostty = fixture.platform.read_to_string(&fixture.ghostty_config).unwrap().unwrap();
        assert!(ghostty.contains("# user ghostty"));
        assert!(ghostty.contains("config-file = ?"));
        assert!(!ghostty.contains("shell-integration"));

        let generated = fixture
            .platform
            .paths()
            .generated(kiseki_compiler::artifact_names::ENV_ZSH);
        assert!(fixture.platform.exists(&generated));

        let manifest_path = report.journal_dir.join(&report.tx_id).join("manifest.json");
        assert!(fixture.platform.exists(&manifest_path));
        let raw = fixture.platform.read_to_string(&manifest_path).unwrap().unwrap();
        let manifest = JournalManifest::from_json(&raw).unwrap();
        assert_eq!(manifest.tx_id, report.tx_id);
        assert!(manifest.entries.iter().any(|entry| entry.target == fixture.zshrc && entry.backup.is_some()));
        assert!(manifest.entries.iter().all(|entry| entry.sha256_before.is_none() || entry.sha256_before.as_ref().unwrap().len() == 64));
    }

    #[test]
    fn fail_fast_validation_writes_nothing() {
        let fixture = fixture();
        let mut output = compiled(fixture.platform.paths().config_dir());
        output
            .artifacts
            .insert(
                kiseki_compiler::artifact_names::GHOSTTY_CONF,
                "shell-integration-features = no-cursor\n",
            )
            .unwrap();
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let error = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions::default(),
            ))
            .unwrap_err();
        assert!(error.to_string().contains("failed validation"), "{error}");
        assert!(!fixture.platform.exists(&fixture.platform.paths().generated("ghostty.conf")));
        assert!(!fixture.platform.exists(&fixture.platform.paths().journal_dir()));
        assert_eq!(
            fixture.platform.read_to_string(&fixture.zshrc).unwrap().unwrap(),
            "# user zshrc\nexport FOO=1\n"
        );
    }

    #[test]
    fn dry_run_writes_nothing() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let before = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                before.clone(),
                before.clone(),
                &env,
                TransactionOptions {
                    dry_run: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        assert!(report.dry_run);
        assert!(report.targets.iter().all(|target| target.status == TargetStatus::DryRun));
        assert!(!fixture.platform.exists(&fixture.platform.paths().generated("env.zsh")));
        assert!(!fixture.platform.exists(&before_gen_path(&fixture.platform)));
        assert!(!fixture.platform.exists(&fixture.platform.paths().journal_dir()));
        // Not even `<config>/` (created by the lock) may appear.
        assert!(
            !fixture.platform.exists(fixture.platform.paths().config_dir()),
            "a dry run must not create the config directory"
        );
        assert!(!fixture
            .platform
            .read_to_string(&fixture.zshrc)
            .unwrap()
            .unwrap()
            .contains("kiseki"));
    }

    #[test]
    fn dry_run_unhooks_do_not_create_the_config_directory() {
        let fixture = fixture();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply_unhooks(&state, Vec::new(), "uninstall", true)
            .unwrap();
        assert!(report.dry_run);
        assert!(!fixture.platform.exists(fixture.platform.paths().config_dir()));
        assert!(!fixture.platform.exists(&fixture.platform.paths().lock_file()));
    }

    fn before_gen_path(platform: &MockPlatform) -> PathBuf {
        platform.paths().gen_file()
    }

    #[test]
    fn render_only_never_touches_host_files() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions {
                    render_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        assert!(report.render_only);
        // Render-only schedules no host operations at all.
        assert_eq!(
            report
                .targets
                .iter()
                .filter(|target| target.target == fixture.zshrc)
                .count(),
            0
        );
        assert_eq!(
            fixture.platform.read_to_string(&fixture.zshrc).unwrap().unwrap(),
            "# user zshrc\nexport FOO=1\n"
        );
        assert!(fixture
            .platform
            .exists(&fixture.platform.paths().generated(kiseki_compiler::artifact_names::ENV_ZSH)));
    }

    #[test]
    fn one_failing_target_does_not_block_the_others() {
        let fixture = fixture();
        // Invalid UTF-8 passes detection (a regular writable file) but fails
        // when the adapter tries to read it, which is exactly the per-target
        // failure isolation path.
        std::fs::write(&fixture.zshrc, b"\xFF\xFE not utf-8\n").unwrap();

        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();

        let zsh = report.targets.iter().find(|target| target.target == fixture.zshrc).unwrap();
        assert!(matches!(zsh.status, TargetStatus::RolledBack | TargetStatus::Failed));
        let ghostty = report
            .targets
            .iter()
            .find(|target| target.target == fixture.ghostty_config)
            .unwrap();
        assert_eq!(ghostty.status, TargetStatus::Applied);
        assert!(fixture
            .platform
            .read_to_string(&fixture.ghostty_config)
            .unwrap()
            .unwrap()
            .contains("config-file = ?"));
        assert_eq!(State::read_gen(&fixture.platform, fixture.platform.paths()).unwrap(), Some(report.state.generation));
    }

    #[test]
    fn rollback_restores_every_backup_and_the_state() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state_before = initial_state(&fixture.platform);
        let mut state_after = state_before.clone();
        state_after.rendering.profile = RenderingProfile::Eco;
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state_before.clone(),
                state_after,
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();
        let applied_generation = report.state.generation;

        let rollback = manager.rollback(&report.tx_id).unwrap();
        // Content is restored, but the generation stays strictly monotonic so
        // shell hooks always reload `env.*`.
        assert_eq!(rollback.state.active_theme, state_before.active_theme);
        assert_eq!(rollback.state.rendering.profile, state_before.rendering.profile);
        assert!(rollback.state.generation > applied_generation, "{}", rollback.state.generation);
        assert_eq!(
            fixture.platform.read_to_string(&fixture.zshrc).unwrap().unwrap(),
            "# user zshrc\nexport FOO=1\n"
        );
        assert_eq!(
            fixture.platform.read_to_string(&fixture.ghostty_config).unwrap().unwrap(),
            "# user ghostty\nfont-size = 12\n"
        );
        assert_eq!(
            State::read_gen(&fixture.platform, fixture.platform.paths()).unwrap(),
            Some(rollback.state.generation)
        );
        assert!(rollback.restored.iter().any(|path| path == &fixture.zshrc));
    }

    #[test]
    fn rollback_generation_is_greater_than_a_newer_on_disk_generation() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let first = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state.clone(),
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();
        let second = manager
            .apply(request(
                &theme,
                &output.artifacts,
                first.state.clone(),
                first.state.clone(),
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();
        // Rolling back the *first* journal must still jump ahead of the newer
        // disk generation instead of rewinding it.
        let rollback = manager.rollback(&first.tx_id).unwrap();
        assert!(rollback.state.generation > second.state.generation);
        assert_eq!(
            State::read_gen(&fixture.platform, fixture.platform.paths()).unwrap(),
            Some(rollback.state.generation)
        );
    }

    #[test]
    fn generation_never_goes_backwards_when_state_before_is_stale() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let mut stale = initial_state(&fixture.platform);
        stale.generation = 98;
        stale.save(&fixture.platform, fixture.platform.paths()).unwrap();
        stale.write_gen(&fixture.platform, fixture.platform.paths()).unwrap();
        // A caller that assembled `state_after` from an older snapshot.
        let mut state_after = initial_state(&fixture.platform);
        state_after.generation = 3;
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                initial_state(&fixture.platform),
                state_after,
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();
        assert_eq!(report.state.generation, 99);
        assert_eq!(
            State::read_gen(&fixture.platform, fixture.platform.paths()).unwrap(),
            Some(99)
        );
    }

    #[test]
    fn render_only_never_reloads_an_adapter() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions {
                    render_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        assert!(report.reload.is_empty(), "{:?}", report.reload);
        assert!(fixture.platform.commands().is_empty());
    }

    #[test]
    fn an_empty_authoritative_allow_list_runs_zero_adapters() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions {
                    only: Vec::new(),
                    explicit_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        // Only the generated artefacts are written; no host file is touched.
        assert!(report
            .targets
            .iter()
            .all(|target| target.adapter == "generated"));
        assert_eq!(
            fixture.platform.read_to_string(&fixture.zshrc).unwrap().unwrap(),
            "# user zshrc\nexport FOO=1\n"
        );
        assert!(!fixture
            .platform
            .read_to_string(&fixture.ghostty_config)
            .unwrap()
            .unwrap()
            .contains("kiseki"));
    }

    #[test]
    fn windows_terminal_baseline_is_scoped_and_retired() {
        let fixture = fixture();
        let settings = fixture._dir.path().join("wt-settings.json");
        fixture
            .platform
            .write_preserving_symlinks(
                &settings,
                b"{\n  // user comment\n  \"profiles\": { \"defaults\": { \"font\": { \"face\": \"Cascadia Code\" } }, \"list\": [] }\n}\n",
            )
            .unwrap();
        fixture
            .platform
            .set_env("KISEKI_WT_SETTINGS", &settings.to_string_lossy());
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let baseline = fixture.platform.paths().journal_dir().join("wt-baseline.json");

        manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state.clone(),
                &env,
                TransactionOptions {
                    only: vec!["windows-terminal".to_string()],
                    explicit_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        let captured = WtBaseline::load(&baseline, &fixture.platform).unwrap().unwrap();
        assert_eq!(captured.source, std::fs::canonicalize(&settings).unwrap());

        // A baseline recorded for a *different* settings file is recaptured.
        let mut stale = captured.clone();
        stale.source = PathBuf::from("/elsewhere/settings.json");
        stale
            .defaults
            .insert("font.face".to_string(), Some(serde_json::json!("Stale")));
        stale.save(&baseline, &fixture.platform).unwrap();
        manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state.clone(),
                &env,
                TransactionOptions {
                    only: vec!["windows-terminal".to_string()],
                    explicit_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        let recaptured = WtBaseline::load(&baseline, &fixture.platform).unwrap().unwrap();
        assert_eq!(recaptured.source, std::fs::canonicalize(&settings).unwrap());
        assert_eq!(
            recaptured.defaults.get("font.face"),
            Some(&Some(serde_json::json!("JetBrainsMono Nerd Font")))
        );

    }

    #[test]
    fn restoring_windows_terminal_retires_the_baseline() {
        let fixture = fixture();
        let settings = fixture._dir.path().join("wt-settings.json");
        fixture
            .platform
            .write_preserving_symlinks(
                &settings,
                b"{\n  // user comment\n  \"profiles\": { \"defaults\": { \"font\": { \"face\": \"Cascadia Code\" } }, \"list\": [] }\n}\n",
            )
            .unwrap();
        fixture
            .platform
            .set_env("KISEKI_WT_SETTINGS", &settings.to_string_lossy());
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let baseline = fixture.platform.paths().journal_dir().join("wt-baseline.json");

        manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state.clone(),
                &env,
                TransactionOptions {
                    only: vec!["windows-terminal".to_string()],
                    explicit_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        assert!(fixture.platform.exists(&baseline));

        // Restoring retires the baseline so a reinstall captures fresh defaults.
        let mut plans = Vec::new();
        for adapter in AdapterRegistry::milestone1().iter() {
            let detection = adapter.detect(&env, &fixture.platform).unwrap();
            if detection.installed || detection.config_path.is_some() {
                plans.push(adapter.plan_unhook(&detection, fixture.platform.paths()).unwrap());
            }
        }
        manager
            .apply_unhooks(&state, plans, "uninstall", false)
            .unwrap();
        assert!(!fixture.platform.exists(&baseline));
        let restored = fixture.platform.read_to_string(&settings).unwrap().unwrap();
        assert!(restored.contains("// user comment"), "{restored}");
        assert!(restored.contains("Cascadia Code"), "{restored}");
        assert!(!restored.contains("Kiseki: NERV UNIT-02"), "{restored}");
    }

    #[test]
    fn dry_run_unhooks_write_nothing() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state.clone(),
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();
        let zshrc_before = fixture.platform.read_to_string(&fixture.zshrc).unwrap();
        let ghostty_before = fixture.platform.read_to_string(&fixture.ghostty_config).unwrap();
        let mut plans = Vec::new();
        for adapter in AdapterRegistry::milestone1().iter() {
            let detection = adapter.detect(&env, &fixture.platform).unwrap();
            if detection.installed || detection.config_path.is_some() {
                plans.push(adapter.plan_unhook(&detection, fixture.platform.paths()).unwrap());
            }
        }
        let report = manager
            .apply_unhooks(&state, plans, "uninstall", true)
            .unwrap();
        assert!(report.dry_run);
        assert!(report.targets.iter().all(|target| target.status == TargetStatus::DryRun));
        assert_eq!(fixture.platform.read_to_string(&fixture.zshrc).unwrap(), zshrc_before);
        assert_eq!(
            fixture.platform.read_to_string(&fixture.ghostty_config).unwrap(),
            ghostty_before
        );
    }

    #[test]
    fn prepare_safe_state_commits_nothing() {
        let fixture = fixture();
        let mut state = initial_state(&fixture.platform);
        state.rendering.profile = RenderingProfile::Ultra;
        state.rendering.crt = true;
        state.save(&fixture.platform, fixture.platform.paths()).unwrap();
        state.write_gen(&fixture.platform, fixture.platform.paths()).unwrap();
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        manager.prepare_safe_state(&mut state).unwrap();
        assert_eq!(state.rendering.profile, RenderingProfile::Static);
        // Nothing was written: the previous document is still authoritative.
        let loaded = State::load(&fixture.platform, fixture.platform.paths()).unwrap().unwrap();
        assert_eq!(loaded.rendering.profile, RenderingProfile::Ultra);
    }

    #[test]
    fn list_journals_is_newest_first() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let first = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state.clone(),
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();
        let second = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();
        let journals = manager.list_journals().unwrap();
        assert_eq!(journals.len(), 2);
        assert_eq!(journals[0].tx_id, second.tx_id);
        assert_eq!(journals[1].tx_id, first.tx_id);
    }

    #[test]
    fn recover_safe_forces_static_and_clears_crt() {
        let fixture = fixture();
        let mut state = initial_state(&fixture.platform);
        state.rendering.profile = RenderingProfile::Ultra;
        state.rendering.crt = true;
        state.rendering.bg_opacity_override = Some(0.4);
        state.save(&fixture.platform, fixture.platform.paths()).unwrap();
        state.write_gen(&fixture.platform, fixture.platform.paths()).unwrap();

        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        manager.recover_safe(&mut state).unwrap();
        assert_eq!(state.rendering.profile, RenderingProfile::Static);
        assert!(!state.rendering.crt);
        assert_eq!(state.rendering.bg_opacity_override, None);
        let loaded = State::load(&fixture.platform, fixture.platform.paths()).unwrap().unwrap();
        assert_eq!(loaded.rendering.profile, RenderingProfile::Static);
    }

    #[test]
    fn symlinked_dotfiles_are_written_through_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let target = dir.path().join("dotfiles").join("zshrc");
        platform.create_dir_all(target.parent().unwrap()).unwrap();
        platform.write_preserving_symlinks(&target, b"# dotfiles zshrc\n").unwrap();
        let link = dir.path().join(".zshrc");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        match std::os::windows::fs::symlink_file(&target, &link) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(1314) => return,
            Err(error) => panic!("symlink_file failed: {error}"),
        }
        platform.set_env("KISEKI_ZSHRC", &link.to_string_lossy());

        let output = compiled(platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&platform);
        let manager = TransactionManager::new(&platform, AdapterRegistry::milestone1());
        manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions {
                    only: vec!["zsh".to_string()],
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();

        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(platform.read_to_string(&target).unwrap().unwrap().contains("kiseki"));
    }

    #[test]
    fn rollback_repairs_a_corrupt_state_file() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();

        // A corrupt document is precisely what a rollback must survive.
        fixture
            .platform
            .write_preserving_symlinks(&fixture.platform.paths().state_file(), b"{ not json")
            .unwrap();
        let rollback = manager.rollback(&report.tx_id).unwrap();
        assert!(rollback.state.generation > report.state.generation);
        let loaded = State::load(&fixture.platform, fixture.platform.paths())
            .unwrap()
            .expect("rollback must rewrite state.json");
        assert_eq!(loaded.generation, rollback.state.generation);
        assert_eq!(
            State::read_gen(&fixture.platform, fixture.platform.paths()).unwrap(),
            Some(rollback.state.generation)
        );
    }

    #[test]
    fn latest_journal_state_is_read_only() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state_before = initial_state(&fixture.platform);
        let mut state_after = state_before.clone();
        state_after.rendering.crt = true;
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state_before.clone(),
                state_after,
                &env,
                TransactionOptions::default(),
            ))
            .unwrap();
        let state_path = fixture.platform.paths().state_file();
        let before = fixture.platform.read_bytes(&state_path).unwrap();
        let recovered = manager
            .latest_journal_state()
            .unwrap()
            .expect("a journal exists");
        assert_eq!(recovered.active_theme, state_before.active_theme);
        assert!(!recovered.rendering.crt);
        assert_eq!(recovered.generation, state_before.generation);
        // Reading the journal never rewrites state.json.
        assert_eq!(fixture.platform.read_bytes(&state_path).unwrap(), before);
        assert_eq!(recovered.generation, report.state.generation - 1);
    }

    #[test]
    fn read_only_generated_artifacts_fail_instead_of_rendering_only() {
        let fixture = fixture();
        let output = compiled(fixture.platform.paths().config_dir());
        let generated = fixture
            .platform
            .paths()
            .generated(kiseki_compiler::artifact_names::GHOSTTY_CONF);
        fixture.platform.create_dir_all(generated.parent().unwrap()).unwrap();
        fixture
            .platform
            .write_preserving_symlinks(&generated, b"# old\n")
            .unwrap();
        let mut permissions = std::fs::metadata(&generated).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&generated, permissions).unwrap();

        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions {
                    only: vec!["ghostty".to_string()],
                    explicit_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        let target = report
            .targets
            .iter()
            .find(|target| target.target == generated)
            .expect("the generated ghostty.conf is planned");
        assert!(
            matches!(target.status, TargetStatus::Failed | TargetStatus::RolledBack),
            "{:?}",
            target.status
        );
    }

    #[test]
    fn windows_terminal_baseline_records_a_canonical_source() {
        let fixture = fixture();
        let real = fixture._dir.path().join("real-settings.json");
        fixture
            .platform
            .write_preserving_symlinks(
                &real,
                b"{\n  \"profiles\": { \"defaults\": { \"font\": { \"face\": \"Cascadia Code\" } }, \"list\": [] }\n}\n",
            )
            .unwrap();
        let link = fixture._dir.path().join("linked-settings.json");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        match std::os::windows::fs::symlink_file(&real, &link) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(1314) => return,
            Err(error) => panic!("symlink_file failed: {error}"),
        }
        fixture
            .platform
            .set_env("KISEKI_WT_SETTINGS", &link.to_string_lossy());
        let output = compiled(fixture.platform.paths().config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions {
                    only: vec!["windows-terminal".to_string()],
                    explicit_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        let baseline = fixture.platform.paths().journal_dir().join("wt-baseline.json");
        let captured = WtBaseline::load(&baseline, &fixture.platform)
            .unwrap()
            .expect("a baseline was captured");
        assert_eq!(captured.source, std::fs::canonicalize(&real).unwrap());
        // The real target (not the symlink spelling) received the patch.
        let patched = fixture.platform.read_to_string(&real).unwrap().unwrap();
        assert!(patched.contains("Kiseki: NERV UNIT-02"), "{patched}");
    }

    #[test]
    fn sha256_helper_matches_known_value() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn apply_respects_state_gen_floor_when_state_json_is_corrupt() {
        let fixture = fixture();
        let paths = fixture.platform.paths().clone();
        fixture
            .platform
            .write_preserving_symlinks(&paths.state_file(), b"{ corrupt")
            .unwrap();
        fixture
            .platform
            .write_preserving_symlinks(&paths.gen_file(), b"12\n")
            .unwrap();

        let output = compiled(paths.config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let mut stale = initial_state(&fixture.platform);
        stale.generation = 8;
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                stale.clone(),
                stale,
                &env,
                TransactionOptions {
                    render_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        assert_eq!(report.state.generation, 13);
    }

    #[cfg(unix)]
    #[test]
    fn require_all_targets_skips_state_commit_when_generated_target_is_read_only() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = fixture();
        let paths = fixture.platform.paths().clone();
        let output = compiled(paths.config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let mut before = initial_state(&fixture.platform);
        before.rendering.profile = RenderingProfile::Ultra;
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let first = manager
            .apply(request(
                &theme,
                &output.artifacts,
                before.clone(),
                before,
                &env,
                TransactionOptions {
                    render_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        assert_eq!(first.state.rendering.profile, RenderingProfile::Ultra);

        let ghostty_gen = paths.generated_dir().join("ghostty.conf");
        std::fs::set_permissions(&ghostty_gen, std::fs::Permissions::from_mode(0o444)).unwrap();

        let mut after = first.state.clone();
        after.rendering.profile = RenderingProfile::Static;
        let report = manager
            .apply(request(
                &theme,
                &output.artifacts,
                first.state.clone(),
                after,
                &env,
                TransactionOptions {
                    render_only: true,
                    require_all_targets: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();
        std::fs::set_permissions(&ghostty_gen, std::fs::Permissions::from_mode(0o644)).unwrap();

        // Blocked target must prevent committing `static` to state.json.
        assert!(report
            .targets
            .iter()
            .any(|t| matches!(t.status, TargetStatus::Failed | TargetStatus::RolledBack)));
        let on_disk = State::load(&fixture.platform, &paths).unwrap().unwrap();
        assert_eq!(on_disk.rendering.profile, RenderingProfile::Ultra);
        assert_eq!(on_disk.generation, first.state.generation);
    }

    #[cfg(unix)]
    #[test]
    fn rollback_errors_when_host_target_became_read_only() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = fixture();
        let paths = fixture.platform.paths().clone();
        let host = fixture._dir.path().join("ghostty-config");
        fixture
            .platform
            .write_preserving_symlinks(&host, b"# user config\n")
            .unwrap();
        fixture
            .platform
            .set_env("KISEKI_GHOSTTY_CONFIG", &host.to_string_lossy());

        let output = compiled(paths.config_dir());
        let theme = theme();
        let env = EnvContext::local();
        let state = initial_state(&fixture.platform);
        let manager = TransactionManager::new(&fixture.platform, AdapterRegistry::milestone1());
        let first = manager
            .apply(request(
                &theme,
                &output.artifacts,
                state.clone(),
                state,
                &env,
                TransactionOptions {
                    only: vec!["ghostty".to_string()],
                    explicit_only: true,
                    ..TransactionOptions::default()
                },
            ))
            .unwrap();

        std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o444)).unwrap();
        let result = manager.rollback(&first.tx_id);
        std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(result.is_err(), "read-only host cannot silently count as restored");
    }
}
