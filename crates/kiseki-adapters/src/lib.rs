//! Adapter contract and capability negotiation (RFC-0003).
//!
//! Adapters are pure planners: `plan()` returns a list of [`FileOperation`]
//! values and never touches the host. Symlink resolution, read-only downgrade,
//! journaling, snapshotting, locking and rollback all live in `kiseki-tx`, so
//! every adapter inherits the same safety guarantees.

pub mod fastfetch;
pub mod ghostty;
pub mod prompt;
pub mod registry;
pub mod shell;
pub mod windows_terminal;

use std::path::PathBuf;

use kiseki_compiler::ArtifactMap;
use kiseki_platform::{PathKind, Platform};
use kiseki_schema::ResolvedTheme;
use kiseki_types::{CapabilityFlags, Diagnostic, DiagnosticReport, EnvContext, PromptPolicy, Result};

pub use fastfetch::FastfetchAdapter;
pub use ghostty::GhosttyAdapter;
pub use prompt::PromptAdapter;
pub use registry::AdapterRegistry;
pub use shell::{PowerShellAdapter, ZshAdapter};
pub use windows_terminal::{
    capture_baseline, detect_profile_conflicts, patch_settings, restore_settings, ProfileConflict, WtBaseline,
    WindowsTerminalAdapter,
};

/// Broad category an adapter belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdapterKind {
    Terminal,
    Shell,
    Prompt,
    Editor,
    Multiplexer,
    AgentTui,
    DevTool,
    SystemMonitor,
}

impl AdapterKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Shell => "shell",
            Self::Prompt => "prompt",
            Self::Editor => "editor",
            Self::Multiplexer => "multiplexer",
            Self::AgentTui => "agent-tui",
            Self::DevTool => "dev-tool",
            Self::SystemMonitor => "system-monitor",
        }
    }
}

/// How deeply an adapter integrates with the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntegrationTier {
    /// First-class auto hot-swap with a one-line anchor.
    Tier1Managed,
    /// Compiled artefact plus explicit opt-in mounting.
    Tier2OptInArtifact,
    /// ANSI passthrough with `doctor` guidance only.
    Tier3AnsiPassthrough,
}

impl IntegrationTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tier1Managed => "tier1",
            Self::Tier2OptInArtifact => "tier2",
            Self::Tier3AnsiPassthrough => "tier3",
        }
    }
}

/// What detection learned about one host application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectionReport {
    pub installed: bool,
    pub version: Option<String>,
    pub config_path: Option<PathBuf>,
    pub path_kind: PathKind,
    /// Additional host config files managed by the same adapter (for example
    /// Windows PowerShell 5.1 alongside PowerShell 7 on Windows).
    pub extra_config_paths: Vec<(PathBuf, PathKind)>,
    pub active_in_session: bool,
    pub notes: Vec<String>,
}

impl DetectionReport {
    /// A report for an application that is not installed at all.
    pub fn not_installed(note: impl Into<String>) -> Self {
        Self {
            installed: false,
            version: None,
            config_path: None,
            path_kind: PathKind::Missing,
            extra_config_paths: Vec::new(),
            active_in_session: false,
            notes: vec![note.into()],
        }
    }

    /// Every config path (primary + extras) with its inspected [`PathKind`].
    pub fn all_config_paths(&self) -> Vec<(&std::path::Path, &PathKind)> {
        let mut paths = Vec::new();
        if let Some(primary) = self.config_path.as_deref() {
            paths.push((primary, &self.path_kind));
        }
        for (extra, kind) in &self.extra_config_paths {
            paths.push((extra.as_path(), kind));
        }
        paths
    }

    /// True when a writable host config file is present.
    pub fn has_writable_config(&self) -> bool {
        self.all_config_paths()
            .into_iter()
            .any(|(_, kind)| kind.is_writable() && *kind != PathKind::Missing)
    }

    /// True when any config exists but cannot be rewritten.
    pub fn is_read_only(&self) -> bool {
        self.all_config_paths()
            .into_iter()
            .any(|(_, kind)| matches!(kind, PathKind::ReadOnlyFile | PathKind::ReadOnlySymlink(_)))
    }
}

/// One declarative change the transaction engine may apply.
#[derive(Debug, Clone, PartialEq)]
pub enum FileOperation {
    /// Write `<kiseki_config>/generated/<relative>`.
    WriteGenerated {
        relative: kiseki_compiler::ArtifactPath,
        content: String,
    },
    /// Idempotently insert or replace a comment-delimited anchor block.
    UpsertMarkerBlock {
        target: PathBuf,
        begin: String,
        end: String,
        body: String,
    },
    /// Remove our anchor block, leaving every other byte untouched.
    RemoveMarkerBlock {
        target: PathBuf,
        begin: String,
        end: String,
    },
    /// Comment-preserving JSONC patch of a Windows Terminal settings file.
    PatchWindowsTerminal {
        target: PathBuf,
        payload: kiseki_compiler::WtVisualPayload,
        baseline: PathBuf,
    },
    /// Restore the values captured the first time we took over the file.
    RestoreWindowsTerminal { target: PathBuf, baseline: PathBuf },
}

impl FileOperation {
    /// True when applying the operation modifies a host (non-Kiseki) file.
    pub fn is_host_write(&self) -> bool {
        !matches!(self, Self::WriteGenerated { .. })
    }

    /// Stable label used by reports and the journal.
    pub fn label(&self) -> String {
        match self {
            Self::WriteGenerated { relative, .. } => format!("generated/{relative}"),
            Self::UpsertMarkerBlock { target, .. } => format!("anchor:{}", target.display()),
            Self::RemoveMarkerBlock { target, .. } => format!("unanchor:{}", target.display()),
            Self::PatchWindowsTerminal { target, .. } => format!("jsonc-patch:{}", target.display()),
            Self::RestoreWindowsTerminal { target, .. } => format!("jsonc-restore:{}", target.display()),
        }
    }
}

/// Options that influence planning but are not part of the theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOptions {
    /// Global artifacts-only mode.
    pub render_only: bool,
    /// Opt-in `theme` / `themes` aliases.
    pub aliases: bool,
    /// Prompt integration policy.
    pub prompt_policy: PromptPolicy,
    /// `kiseki init` may create a missing profile; `kiseki use` may not.
    pub allow_create_host_files: bool,
    /// Adapter allow-list.
    pub only: Vec<String>,
    /// True when `only` is authoritative (an explicit `--only` or a configured
    /// `enabledAdapters` list). An empty authoritative list runs zero adapters
    /// instead of falling back to "every detected adapter".
    pub explicit_only: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            render_only: false,
            aliases: false,
            prompt_policy: PromptPolicy::ManagedTemplate,
            allow_create_host_files: false,
            only: Vec::new(),
            explicit_only: false,
        }
    }
}

impl PlanOptions {
    /// True when `adapter` passes the allow-list.
    pub fn allows(&self, adapter: &str) -> bool {
        if !self.explicit_only && self.only.is_empty() {
            return true;
        }
        self.only.iter().any(|only| only == adapter)
    }

    /// True when host files may be modified in this environment.
    pub fn host_writes_allowed(&self, env: &EnvContext) -> bool {
        !self.render_only && !env.should_suppress_host_writes()
    }
}

/// Everything a planner needs.
pub struct PlanContext<'a> {
    pub theme: &'a ResolvedTheme,
    pub artifacts: &'a ArtifactMap,
    pub options: &'a PlanOptions,
    pub env: &'a EnvContext,
    pub paths: &'a kiseki_platform::KisekiPaths,
}

/// A complete, side-effect-free plan for one adapter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdapterPlan {
    pub adapter: &'static str,
    pub operations: Vec<FileOperation>,
    /// True when host writes were downgraded; generated files are still written.
    pub render_only: bool,
    pub diagnostics: DiagnosticReport,
}

impl AdapterPlan {
    pub fn new(adapter: &'static str) -> Self {
        Self {
            adapter,
            operations: Vec::new(),
            render_only: false,
            diagnostics: DiagnosticReport::new(),
        }
    }

    pub fn push(&mut self, operation: FileOperation) -> &mut Self {
        self.operations.push(operation);
        self
    }

    pub fn diagnostic(&mut self, diagnostic: Diagnostic) -> &mut Self {
        self.diagnostics.push(diagnostic);
        self
    }

    pub fn has_fatal(&self) -> bool {
        self.diagnostics.has_fatal()
    }

    pub fn host_writes(&self) -> impl Iterator<Item = &FileOperation> {
        self.operations.iter().filter(|operation| operation.is_host_write())
    }

    pub fn generated_writes(&self) -> impl Iterator<Item = (&str, &str)> {
        self.operations.iter().filter_map(|operation| match operation {
            FileOperation::WriteGenerated { relative, content } => {
                Some((relative.as_str(), content.as_str()))
            }
            _ => None,
        })
    }

    /// Adds a write for an artefact produced by the compiler, when present.
    pub fn add_generated(&mut self, artifacts: &ArtifactMap, relative: &str) -> bool {
        let Some(content) = artifacts.get(relative) else {
            return false;
        };
        if let Ok(path) = kiseki_compiler::ArtifactPath::new(relative) {
            self.push(FileOperation::WriteGenerated {
                relative: path,
                content: content.to_string(),
            });
        }
        true
    }

    /// True when a generated artefact is already planned.
    pub fn has_generated(&self, relative: &str) -> bool {
        self.generated_writes()
            .any(|(path, _)| path == relative)
    }
}

/// Static validation result for a plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub diagnostics: DiagnosticReport,
}

impl ValidationReport {
    pub fn ok() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn has_fatal(&self) -> bool {
        self.diagnostics.has_fatal()
    }
}

/// The adapter contract (RFC-0003 §3).
pub trait Adapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn kind(&self) -> AdapterKind;
    fn tier(&self) -> IntegrationTier;

    fn detect(&self, env: &EnvContext, platform: &dyn Platform) -> Result<DetectionReport>;

    fn capabilities(&self, detection: &DetectionReport, env: &EnvContext) -> CapabilityFlags;

    fn plan(
        &self,
        ctx: &PlanContext<'_>,
        detection: &DetectionReport,
        caps: CapabilityFlags,
    ) -> Result<AdapterPlan>;

    fn validate(&self, plan: &AdapterPlan, platform: &dyn Platform) -> Result<ValidationReport>;

    fn reload(
        &self,
        detection: &DetectionReport,
        caps: CapabilityFlags,
        platform: &dyn Platform,
    ) -> Result<kiseki_platform::ReloadOutcome>;

    /// Builds the clean-removal plan for `uninstall` and v1 migration.
    fn plan_unhook(
        &self,
        detection: &DetectionReport,
        paths: &kiseki_platform::KisekiPaths,
    ) -> Result<AdapterPlan>;

    /// Session-only preview; unsupported hosts return `Ok(false)`.
    fn preview_session(&self, _theme: &ResolvedTheme, _platform: &dyn Platform) -> Result<bool> {
        Ok(false)
    }
}

/// Helper for adapters: build the render-only diagnostic with a copyable path.
pub(crate) fn render_only_diagnostic(
    adapter: &str,
    target: &std::path::Path,
    reason: &str,
    hint: String,
) -> Diagnostic {
    Diagnostic::warning(
        format!("adapter.{adapter}.render-only"),
        format!(
            "{} was not modified ({reason}); generated artefacts are still up to date",
            target.display()
        ),
    )
    .with_hint(hint)
}
