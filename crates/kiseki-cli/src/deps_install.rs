//! Native parallel dependency installer (`kiseki deps install` / `kiseki fonts install`).
//!
//! Fetches and SHA-256-verifies the tagged release's `deps.tsv` manifest against
//! `checksums.txt`, skips already-installed tools and fonts (including ZIP font
//! families via per-family receipts), downloads remaining assets concurrently
//! across a bounded synchronous thread pool (`ureq` + `rustls`, zero `tokio`),
//! renders a live multi-line progress UI on interactive terminals, verifies
//! every download before extraction, and installs atomically.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kiseki_platform::{deps, KisekiPaths, Platform};
use kiseki_types::{KisekiError, OsKind, Result};
use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const DEFAULT_REPO: &str = "nerv-base/kiseki-terminal";
pub const CN_ORIGIN: &str = "https://kiseki-dl.nerv-base.com";
pub const GLOBAL_ORIGIN: &str = "https://dl.kisekiterm.dev";
pub const DEFAULT_CONCURRENCY: usize = 4;

const MAX_CONCURRENCY: usize = 8;
const MAX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LICENSE_BYTES: u64 = 1024 * 1024;
const MAX_SINGLE_EXTRACTED_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TOTAL_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_RESUME_ATTEMPTS: usize = 3;
const CHUNK_SIZE: usize = 64 * 1024;
const USER_AGENT: &str = concat!("kiseki-cli/", env!("CARGO_PKG_VERSION"));

/// Configuration for running the parallel dependency installer.
#[derive(Debug, Clone)]
pub struct DepsInstallOptions {
    /// Explicit release tag (e.g. `v2.0.0-beta.1`). When `None`, tries the
    /// current binary's tag first and falls back to the latest published v2 tag.
    pub version: Option<String>,
    /// GitHub repository (`owner/repo`) used as final upstream fallback.
    pub repo: String,
    /// Explicit mirror origin(s) (comma-separated URLs) taking highest priority.
    pub dl_base: Option<String>,
    /// Mirror preference (`"cn"` or `"global"`).
    pub mirror: Option<String>,
    /// Skip all font assets (`font` and `font-raw`).
    pub skip_fonts: bool,
    /// Install only font assets.
    pub only_fonts: bool,
    /// Install only tool assets.
    pub only_tools: bool,
    /// Restrict font installation to these specific font catalog IDs.
    pub fonts: Vec<String>,
    /// Force re-download and re-installation even when receipts/checksums match.
    pub force: bool,
    /// Maximum number of concurrent downloads (clamped to `1..=8`).
    pub concurrency: usize,
    /// Optional local `deps.tsv` manifest file (skips remote manifest fetch).
    pub manifest_file: Option<PathBuf>,
    /// Optional local `checksums.txt` file (skips remote checksums fetch).
    pub checksums_file: Option<PathBuf>,
    /// Plan without downloading or modifying any files.
    pub dry_run: bool,
    /// Suppress interactive progress UI (JSON output is handled by caller).
    pub json: bool,
    /// Suppress human-readable output.
    pub quiet: bool,
}

impl Default for DepsInstallOptions {
    fn default() -> Self {
        Self {
            version: None,
            repo: DEFAULT_REPO.to_string(),
            dl_base: None,
            mirror: None,
            skip_fonts: false,
            only_fonts: false,
            only_tools: false,
            fonts: Vec::new(),
            force: false,
            concurrency: DEFAULT_CONCURRENCY,
            manifest_file: None,
            checksums_file: None,
            dry_run: false,
            json: false,
            quiet: false,
        }
    }
}

/// Machine-readable report returned by [`install_dependencies`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepsInstallReport {
    pub ok: bool,
    pub tag: String,
    pub platform: String,
    pub dry_run: bool,
    pub mirrors: Vec<String>,
    pub summary: DepsInstallSummary,
    pub items: Vec<DepItemOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepsInstallSummary {
    pub total: usize,
    pub installed: usize,
    pub up_to_date: usize,
    pub planned: usize,
    pub failed: usize,
    pub downloaded_bytes: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepItemOutcome {
    pub id: String,
    pub kind: String,
    pub dest: String,
    /// `"installed"` | `"up-to-date"` | `"planned"` | `"failed"`
    pub status: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Kind of dependency row in `deps.tsv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Tool,
    FontRaw,
    FontZip,
}

impl DepKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::FontRaw => "font-raw",
            Self::FontZip => "font",
        }
    }

    pub const fn is_font(self) -> bool {
        matches!(self, Self::FontRaw | Self::FontZip)
    }
}

/// One validated row parsed from `deps.tsv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepRow {
    pub kind: DepKind,
    pub platform: String,
    pub url: String,
    pub sha256: String,
    pub dest: String,
    pub mode: String,
}

/// Parsed `deps.tsv` manifest.
#[derive(Debug, Clone)]
pub struct DepManifest {
    pub font_dirs: BTreeMap<String, String>,
    pub font_patterns: BTreeMap<String, String>,
    pub rows: Vec<DepRow>,
}

/// Resolved target asset to install (or skip).
#[derive(Debug, Clone)]
struct PlannedJob {
    /// Human-friendly display label (e.g. `oh-my-posh`, `maple-mono-nf-cn`, `JetBrainsMonoNerdFont-Regular.ttf`).
    label: String,
    /// Catalog ID for fonts (e.g. `jetbrains-mono-nf`), or tool ID (`oh-my-posh`).
    id: String,
    row: DepRow,
    font_pattern: Option<String>,
    /// True if already verified on disk and `!force`.
    up_to_date: bool,
    /// Existing verified files when `up_to_date` is true.
    existing_files: Vec<PathBuf>,
}

/// Runtime timeouts (can be shortened in unit/integration tests via `KISEKI_TEST_IO_TIMEOUT_MS`).
#[derive(Debug, Clone, Copy)]
struct IoTimeouts {
    resolve: Duration,
    connect: Duration,
    send_request: Duration,
    recv_response: Duration,
    per_read: Duration,
    meta_global: Duration,
}

impl IoTimeouts {
    fn from_env(platform: &dyn Platform) -> Self {
        if let Some(ms) = platform
            .env_var("KISEKI_TEST_IO_TIMEOUT_MS")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&v| v > 0)
        {
            let d = Duration::from_millis(ms);
            return Self {
                resolve: d,
                connect: d,
                send_request: d,
                recv_response: d,
                per_read: d,
                meta_global: d.max(Duration::from_millis(500)),
            };
        }
        Self {
            resolve: Duration::from_secs(8),
            connect: Duration::from_secs(10),
            send_request: Duration::from_secs(15),
            recv_response: Duration::from_secs(30),
            // Per-socket-read stall timeout: restarts on every read chunk,
            // so healthy large files (e.g. 159.5 MB MapleMono-NF-CN.zip) never
            // get killed by an arbitrary total transfer timeout, while genuinely
            // stalled mirrors fail fast and trigger resume/failover.
            per_read: Duration::from_secs(30),
            meta_global: Duration::from_secs(15),
        }
    }

    fn meta_agent(self) -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_resolve(Some(self.resolve))
            .timeout_connect(Some(self.connect))
            .timeout_global(Some(self.meta_global))
            .http_status_as_error(false)
            .build()
            .into()
    }

    fn stream_agent(self) -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_resolve(Some(self.resolve))
            .timeout_connect(Some(self.connect))
            .timeout_send_request(Some(self.send_request))
            .timeout_recv_response(Some(self.recv_response))
            .timeout_per_read(Some(self.per_read))
            .timeout_recv_body(None)
            .timeout_global(None)
            .http_status_as_error(false)
            .build()
            .into()
    }
}

/// Executes the parallel dependency installation pipeline.
pub fn install_dependencies(
    platform: &dyn Platform,
    paths: &KisekiPaths,
    options: &DepsInstallOptions,
) -> Result<DepsInstallReport> {
    let started = Instant::now();
    let timeouts = IoTimeouts::from_env(platform);
    let dep_platform = detect_dep_platform(platform)?;
    let font_os = font_os_key(&dep_platform);
    let mirrors = build_mirror_origins(
        options.dl_base.as_deref(),
        options.mirror.as_deref(),
        platform,
    );
    let loopback_only = is_loopback_only(options.dl_base.as_deref());

    let (tag, manifest_text) = resolve_tag_and_manifest(
        options,
        &mirrors,
        loopback_only,
        timeouts,
    )?;
    let manifest = parse_deps_tsv(&manifest_text)?;

    let font_dir = resolve_font_dir(platform, &manifest, font_os);
    let vendor_bin_dir = deps::installer_bin(platform);
    let vendor_root = vendor_root_dir(platform, paths);
    let receipts_dir = vendor_root.join("receipts");

    let filtered_rows = select_rows(&manifest, &dep_platform, options)?;
    let jobs = plan_jobs(
        &filtered_rows,
        &manifest,
        &vendor_bin_dir,
        font_dir.as_deref(),
        &receipts_dir,
        options.force,
    )?;

    let mut warnings = Vec::new();
    let wants_tools = !options.only_fonts && options.fonts.is_empty();
    let wants_fonts = !options.skip_fonts && !options.only_tools;
    if dep_platform == "linux-musl-arm64" && wants_tools {
        warnings.push("Fastfetch 的上游发布暂无 Linux musl ARM64 二进制。".to_string());
    }

    if options.dry_run {
        let items: Vec<DepItemOutcome> = jobs
            .iter()
            .map(|job| DepItemOutcome {
                id: job.id.clone(),
                kind: job.row.kind.as_str().to_string(),
                dest: job.row.dest.clone(),
                status: if job.up_to_date {
                    "up-to-date".to_string()
                } else {
                    "planned".to_string()
                },
                bytes: 0,
                mirror: None,
                files: job.existing_files.clone(),
                error: None,
            })
            .collect();
        let up_to_date = items.iter().filter(|i| i.status == "up-to-date").count();
        let planned = items.iter().filter(|i| i.status == "planned").count();
        return Ok(DepsInstallReport {
            ok: true,
            tag,
            platform: dep_platform,
            dry_run: true,
            mirrors,
            summary: DepsInstallSummary {
                total: items.len(),
                installed: 0,
                up_to_date,
                planned,
                failed: 0,
                downloaded_bytes: 0,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
            items,
            warnings,
        });
    }

    let concurrency = options.concurrency.clamp(1, MAX_CONCURRENCY);
    let show_ui = !options.json && !options.quiet;
    let is_tty = show_ui
        && io::stderr().is_terminal()
        && std::env::var_os("TERM").is_none_or(|t| t != "dumb");
    let use_color = is_tty && std::env::var_os("NO_COLOR").is_none();

    if show_ui {
        let primary = mirrors
            .first()
            .map(|m| strip_scheme(m))
            .unwrap_or("github.com");
        let pending_count = jobs.iter().filter(|j| !j.up_to_date).count();
        if use_color {
            eprintln!(
                "\x1b[1;36m▸\x1b[0m \x1b[1mKiseki 依赖安装\x1b[0m \x1b[2m{} · {} · 镜像 {} · 并发 {}\x1b[0m",
                tag,
                dep_platform,
                primary,
                concurrency.min(pending_count.max(1))
            );
        } else {
            eprintln!(
                "▸ Kiseki 依赖安装 ({tag} · {dep_platform} · 镜像 {primary} · 并发 {})",
                concurrency.min(pending_count.max(1))
            );
        }
    }

    // Build shared progress state for live multi-line UI & outcomes.
    let progress = Arc::new(Mutex::new(ProgressState::new(&jobs)));
    let outcomes: Arc<Mutex<Vec<Option<DepItemOutcome>>>> =
        Arc::new(Mutex::new(vec![None; jobs.len()]));

    // Pre-populate already up-to-date items and construct LPT-prioritized work queue
    // (heavy ZIP fonts first so they overlap completely with smaller tools/fonts).
    let mut queue_indices: Vec<usize> = Vec::new();
    for (idx, job) in jobs.iter().enumerate() {
        if job.up_to_date {
            {
                let mut st = progress.lock().unwrap();
                st.items[idx].phase = ItemPhase::UpToDate;
            }
            {
                let mut out = outcomes.lock().unwrap();
                out[idx] = Some(DepItemOutcome {
                    id: job.id.clone(),
                    kind: job.row.kind.as_str().to_string(),
                    dest: job.row.dest.clone(),
                    status: "up-to-date".to_string(),
                    bytes: 0,
                    mirror: None,
                    files: job.existing_files.clone(),
                    error: None,
                });
            }
            if show_ui && !is_tty {
                eprintln!("  ✓ {} 已就绪（跳过）", job.label);
            }
        } else {
            queue_indices.push(idx);
        }
    }
    queue_indices.sort_by_key(|&idx| match jobs[idx].row.kind {
        DepKind::FontZip => {
            if jobs[idx].id == "maple-mono-nf-cn" {
                0
            } else if jobs[idx].id == "fusion-pixel-12-zh" {
                1
            } else {
                2
            }
        }
        DepKind::Tool => 3,
        DepKind::FontRaw => 4,
    });

    let queue = Arc::new(Mutex::new(VecDeque::from(queue_indices)));
    let staging_root = tempfile::Builder::new()
        .prefix("kiseki-deps-")
        .tempdir()
        .map_err(|e| KisekiError::other(format!("failed to create staging dir: {e}")))?;

    let worker_count = concurrency.min(queue.lock().unwrap().len().max(1));
    let done_flag = Arc::new(AtomicBool::new(false));

    let newly_installed_fonts: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));

    std::thread::scope(|scope| {
        // Spawn live multi-line renderer thread on interactive TTYs.
        let renderer_handle = if is_tty && !jobs.is_empty() {
            let progress_ref = Arc::clone(&progress);
            let done_ref = Arc::clone(&done_flag);
            Some(scope.spawn(move || {
                let mut renderer = LiveRenderer::new(use_color);
                while !done_ref.load(Ordering::Relaxed) {
                    {
                        let st = progress_ref.lock().unwrap();
                        renderer.render(&st, false);
                    }
                    std::thread::sleep(Duration::from_millis(80));
                }
                let st = progress_ref.lock().unwrap();
                renderer.render(&st, true);
            }))
        } else {
            None
        };

        // Spawn bounded download/extract worker threads.
        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let queue_ref = Arc::clone(&queue);
            let progress_ref = Arc::clone(&progress);
            let outcomes_ref = Arc::clone(&outcomes);
            let fonts_ref = Arc::clone(&newly_installed_fonts);
            let jobs_ref = &jobs;
            let mirrors_ref = &mirrors;
            let vendor_bin_ref = &vendor_bin_dir;
            let font_dir_ref = font_dir.as_deref();
            let receipts_ref = &receipts_dir;
            let staging_path = staging_root.path();
            let os = platform.os();
            workers.push(scope.spawn(move || {
                let agent = timeouts.stream_agent();
                loop {
                    let Some(idx) = queue_ref.lock().unwrap().pop_front() else {
                        break;
                    };
                    let job = &jobs_ref[idx];
                    let job_dir = staging_path.join(format!("job-{idx}"));
                    let _ = fs::create_dir_all(&job_dir);
                    if show_ui && !is_tty {
                        eprintln!("  ↓ 下载 {}...", job.label);
                    }
                    let outcome = execute_job(
                        idx,
                        job,
                        &agent,
                        mirrors_ref,
                        os,
                        vendor_bin_ref,
                        font_dir_ref,
                        receipts_ref,
                        &job_dir,
                        &progress_ref,
                    );
                    let _ = fs::remove_dir_all(&job_dir);
                    if outcome.status == "installed" && job.row.kind.is_font() {
                        fonts_ref.lock().unwrap().extend(outcome.files.iter().cloned());
                    }
                    if show_ui && !is_tty {
                        match outcome.status.as_str() {
                            "installed" => {
                                eprintln!(
                                    "  ✓ {} 已安装 ({})",
                                    job.label,
                                    format_bytes(outcome.bytes)
                                );
                            }
                            "failed" => {
                                eprintln!(
                                    "  ✗ {} 安装失败: {}",
                                    job.label,
                                    outcome.error.as_deref().unwrap_or("unknown error")
                                );
                            }
                            _ => {}
                        }
                    }
                    outcomes_ref.lock().unwrap()[idx] = Some(outcome);
                }
            }));
        }

        for worker in workers {
            let _ = worker.join();
        }
        done_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = renderer_handle {
            let _ = handle.join();
        }
    });

    let items: Vec<DepItemOutcome> = outcomes
        .lock()
        .unwrap()
        .iter_mut()
        .filter_map(Option::take)
        .collect();

    // Post-parallel serialized finalization (OFL license, fc-cache, Windows registry).
    let installed_font_files = newly_installed_fonts.lock().unwrap().clone();
    let any_font_ready = wants_fonts
        && items
            .iter()
            .any(|i| (i.kind == "font" || i.kind == "font-raw") && i.status != "failed");

    if any_font_ready {
        let ofl_dest = vendor_root.join("licenses").join("OFL-JetBrainsMono.txt");
        if !ofl_dest.is_file() {
            let _ = download_ofl_license(&mirrors, &ofl_dest, timeouts);
        }
    }

    if !installed_font_files.is_empty() {
        if let Some(ref fdir) = font_dir {
            if let Some(warn) = finalize_platform_fonts(platform, fdir, &installed_font_files) {
                warnings.push(warn);
            }
        }
    }

    let installed = items.iter().filter(|i| i.status == "installed").count();
    let up_to_date = items.iter().filter(|i| i.status == "up-to-date").count();
    let failed = items.iter().filter(|i| i.status == "failed").count();
    let downloaded_bytes: u64 = items.iter().map(|i| i.bytes).sum();
    let elapsed_ms = started.elapsed().as_millis() as u64;

    if show_ui {
        if !is_tty {
            eprintln!(
                "  完成: {} 新安装 · {} 已就绪 · {} 失败 · 下载 {} · 用时 {:.1}s",
                installed,
                up_to_date,
                failed,
                format_bytes(downloaded_bytes),
                elapsed_ms as f64 / 1000.0
            );
        }
        for w in &warnings {
            eprintln!("  · {w}");
        }
    }

    Ok(DepsInstallReport {
        ok: failed == 0,
        tag,
        platform: dep_platform,
        dry_run: false,
        mirrors,
        summary: DepsInstallSummary {
            total: items.len(),
            installed,
            up_to_date,
            planned: 0,
            failed,
            downloaded_bytes,
            elapsed_ms,
        },
        items,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// Mirror & tag resolution
// ---------------------------------------------------------------------------

pub fn normalize_origin(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let stripped = trimmed.strip_suffix("/releases").unwrap_or(trimmed);
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

fn is_loopback_origin(origin: &str) -> bool {
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .unwrap_or(origin);
    rest.starts_with("127.0.0.1")
        || rest.starts_with("localhost")
        || rest.starts_with("[::1]")
}

fn is_loopback_only(dl_base: Option<&str>) -> bool {
    let Some(raw) = dl_base else {
        return false;
    };
    let parts: Vec<String> = raw
        .split(',')
        .filter_map(normalize_origin)
        .collect();
    !parts.is_empty() && parts.iter().all(|o| is_loopback_origin(o))
}

/// Orders mirror origins matching `scripts/install.sh` and `scripts/install.ps1`.
///
/// Explicit `--dl-base` / `KISEKI_DL_BASE` (which may be comma-separated) always
/// comes first. If every explicit origin is a loopback test address, public CDN
/// mirrors are omitted so tests stay 100% hermetic.
pub fn build_mirror_origins(
    dl_base: Option<&str>,
    mirror_pref: Option<&str>,
    platform: &dyn Platform,
) -> Vec<String> {
    let mut ordered = Vec::new();
    fn push_unique(ordered: &mut Vec<String>, raw: &str) {
        if let Some(clean) = normalize_origin(raw) {
            if !ordered.contains(&clean) {
                ordered.push(clean);
            }
        }
    }

    if let Some(raw) = dl_base {
        for part in raw.split(',') {
            push_unique(&mut ordered, part);
        }
        if !ordered.is_empty() && ordered.iter().all(|o| is_loopback_origin(o)) {
            return ordered;
        }
    }

    let pref = mirror_pref
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let prefer_cn = match pref.as_deref() {
        Some("cn") => true,
        Some("global") => false,
        _ => is_utc_plus_8(platform),
    };

    if prefer_cn {
        push_unique(&mut ordered, CN_ORIGIN);
        push_unique(&mut ordered, GLOBAL_ORIGIN);
    } else {
        push_unique(&mut ordered, GLOBAL_ORIGIN);
        push_unique(&mut ordered, CN_ORIGIN);
    }
    ordered
}

fn is_utc_plus_8(platform: &dyn Platform) -> bool {
    if let Some(tz) = platform.env_var("TZ") {
        let lower = tz.to_ascii_lowercase();
        if lower.contains("shanghai")
            || lower.contains("chongqing")
            || lower.contains("beijing")
            || lower.contains("hong_kong")
            || lower.contains("taipei")
            || lower.contains("singapore")
            || lower.contains("cst-8")
            || lower.contains("utc+8")
            || lower == "+0800"
        {
            return true;
        }
    }
    #[cfg(unix)]
    {
        if let Ok(target) = fs::read_link("/etc/localtime") {
            let s = target.to_string_lossy().to_ascii_lowercase();
            if s.contains("shanghai")
                || s.contains("chongqing")
                || s.contains("hong_kong")
                || s.contains("taipei")
                || s.contains("singapore")
                || s.contains("macau")
                || s.contains("harbin")
            {
                return true;
            }
        }
        if let Ok(out) = std::process::Command::new("date").arg("+%z").output() {
            if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "+0800" {
                return true;
            }
        }
    }
    #[cfg(windows)]
    {
        if windows_is_utc_plus_8() {
            return true;
        }
    }
    false
}

pub fn normalize_release_tag(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(KisekiError::other("release tag cannot be empty"));
    }
    let with_v = if trimmed.starts_with('v') {
        trimmed.to_string()
    } else {
        format!("v{trimmed}")
    };
    if !with_v
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
    {
        return Err(KisekiError::other(format!(
            "invalid release tag `{trimmed}`"
        )));
    }
    Ok(with_v)
}

fn resolve_tag_and_manifest(
    options: &DepsInstallOptions,
    mirrors: &[String],
    loopback_only: bool,
    timeouts: IoTimeouts,
) -> Result<(String, String)> {
    // Fast path: caller provided a local `deps.tsv` directly.
    if let Some(manifest_path) = options.manifest_file.as_deref() {
        let bytes = fs::read(manifest_path).map_err(|e| {
            KisekiError::other(format!(
                "failed to read manifest `{}`: {e}",
                manifest_path.display()
            ))
        })?;
        if let Some(cs_path) = options.checksums_file.as_deref() {
            let cs_text = fs::read_to_string(cs_path).map_err(|e| {
                KisekiError::other(format!(
                    "failed to read checksums `{}`: {e}",
                    cs_path.display()
                ))
            })?;
            verify_manifest_bytes(&bytes, &cs_text)?;
        }
        let text = String::from_utf8(bytes)
            .map_err(|e| KisekiError::other(format!("deps.tsv is not valid UTF-8: {e}")))?;
        let tag = match options.version.as_deref() {
            Some(v) => normalize_release_tag(v)?,
            None => format!("v{}", env!("CARGO_PKG_VERSION")),
        };
        return Ok((tag, text));
    }

    let agent = timeouts.meta_agent();

    // Determine release tag: explicit `--version` wins; otherwise try binary's
    // own tag first and fall back to latest published v2 release if unpublished.
    let explicit_tag = match options.version.as_deref() {
        Some(v) if v.eq_ignore_ascii_case("latest") => None,
        Some(v) => Some(normalize_release_tag(v)?),
        None => None,
    };

    let candidate_tags = if let Some(tag) = explicit_tag {
        vec![tag]
    } else {
        let mut tags = vec![format!("v{}", env!("CARGO_PKG_VERSION"))];
        if let Ok(latest) = resolve_latest_tag(&agent, mirrors, &options.repo, loopback_only) {
            if !tags.contains(&latest) {
                tags.push(latest);
            }
        }
        tags
    };

    let mut last_err = None;
    for tag in candidate_tags {
        match fetch_verified_manifest(
            &agent,
            &tag,
            mirrors,
            &options.repo,
            loopback_only,
            options.checksums_file.as_deref(),
        ) {
            Ok(text) => return Ok((tag, text)),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| KisekiError::other("could not resolve release manifest")))
}

fn resolve_latest_tag(
    agent: &ureq::Agent,
    mirrors: &[String],
    repo: &str,
    loopback_only: bool,
) -> Result<String> {
    for origin in mirrors {
        let url = format!("{origin}/releases/latest.json");
        if let Ok(bytes) = fetch_small_bytes(agent, &url, 64 * 1024) {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(tag) = val.get("tag").and_then(|v| v.as_str()) {
                    if tag.starts_with("v2.") {
                        if let Ok(norm) = normalize_release_tag(tag) {
                            return Ok(norm);
                        }
                    }
                }
            }
        }
    }
    if loopback_only {
        return Err(KisekiError::other(
            "could not resolve latest release tag from configured mirror",
        ));
    }
    let api_url = format!("https://api.github.com/repos/{repo}/releases");
    let bytes = fetch_small_bytes(agent, &api_url, 2 * 1024 * 1024)?;
    let releases: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
        .map_err(|e| KisekiError::other(format!("invalid GitHub releases JSON: {e}")))?;
    for rel in releases {
        let is_draft = rel.get("draft").and_then(|v| v.as_bool()).unwrap_or(false);
        if is_draft {
            continue;
        }
        if let Some(tag) = rel.get("tag_name").and_then(|v| v.as_str()) {
            if tag.starts_with("v2.") {
                return normalize_release_tag(tag);
            }
        }
    }
    Err(KisekiError::other(format!(
        "no published v2 release found for {repo}"
    )))
}

fn fetch_verified_manifest(
    agent: &ureq::Agent,
    tag: &str,
    mirrors: &[String],
    repo: &str,
    loopback_only: bool,
    local_checksums: Option<&Path>,
) -> Result<String> {
    let checksums_text = if let Some(cs_path) = local_checksums {
        fs::read_to_string(cs_path).map_err(|e| {
            KisekiError::other(format!(
                "failed to read checksums `{}`: {e}",
                cs_path.display()
            ))
        })?
    } else {
        let bytes = fetch_release_asset(agent, tag, "checksums.txt", mirrors, repo, loopback_only)?;
        String::from_utf8(bytes)
            .map_err(|e| KisekiError::other(format!("checksums.txt is not valid UTF-8: {e}")))?
    };

    let manifest_bytes =
        fetch_release_asset(agent, tag, "deps.tsv", mirrors, repo, loopback_only)?;
    verify_manifest_bytes(&manifest_bytes, &checksums_text)?;
    String::from_utf8(manifest_bytes)
        .map_err(|e| KisekiError::other(format!("deps.tsv is not valid UTF-8: {e}")))
}

fn fetch_release_asset(
    agent: &ureq::Agent,
    tag: &str,
    asset_name: &str,
    mirrors: &[String],
    repo: &str,
    loopback_only: bool,
) -> Result<Vec<u8>> {
    let mut urls: Vec<String> = mirrors
        .iter()
        .map(|origin| format!("{origin}/releases/{tag}/{asset_name}"))
        .collect();
    if !loopback_only {
        urls.push(format!(
            "https://github.com/{repo}/releases/download/{tag}/{asset_name}"
        ));
    }
    let mut errors = Vec::new();
    for url in urls {
        match fetch_small_bytes(agent, &url, MAX_MANIFEST_BYTES) {
            Ok(bytes) => return Ok(bytes),
            Err(err) => errors.push(format!("{url}: {err}")),
        }
    }
    Err(KisekiError::other(format!(
        "failed to fetch {asset_name} for {tag}: {}",
        errors.join("; ")
    )))
}

fn fetch_small_bytes(agent: &ureq::Agent, url: &str, max_bytes: u64) -> Result<Vec<u8>> {
    let resp = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| KisekiError::other(format!("HTTP error: {e}")))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(KisekiError::other(format!("HTTP {status}")));
    }
    let mut out = Vec::new();
    resp.into_body()
        .into_reader()
        .take(max_bytes + 1)
        .read_to_end(&mut out)
        .map_err(|e| KisekiError::other(format!("read error: {e}")))?;
    if out.len() as u64 > max_bytes {
        return Err(KisekiError::other(format!(
            "response exceeded {max_bytes} bytes"
        )));
    }
    Ok(out)
}

pub fn lookup_release_checksum(checksums_text: &str, asset_name: &str) -> Option<String> {
    for line in checksums_text.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        let clean = name.strip_prefix('*').unwrap_or(name);
        if clean == asset_name && is_sha256_hex(hash) {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

fn verify_manifest_bytes(manifest_bytes: &[u8], checksums_text: &str) -> Result<()> {
    let expected = lookup_release_checksum(checksums_text, "deps.tsv").ok_or_else(|| {
        KisekiError::other("checksum entry for `deps.tsv` not found in checksums.txt")
    })?;
    let actual = sha256_bytes(manifest_bytes);
    if actual != expected {
        return Err(KisekiError::other(format!(
            "SHA-256 mismatch for deps.tsv (expected {expected}, got {actual})"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Manifest parsing & platform detection
// ---------------------------------------------------------------------------

pub fn parse_deps_tsv(text: &str) -> Result<DepManifest> {
    let mut font_dirs = BTreeMap::new();
    let mut font_patterns = BTreeMap::new();
    let mut rows = Vec::new();

    for (line_no, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() >= 3 {
                match fields[0] {
                    "# font-dir" => {
                        font_dirs.insert(fields[1].to_string(), fields[2].to_string());
                    }
                    "# font" => {
                        font_patterns.insert(fields[1].to_string(), fields[2].to_string());
                    }
                    _ => {}
                }
            }
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 6 {
            return Err(KisekiError::other(format!(
                "malformed deps.tsv line {}: expected 6 columns, got {}",
                line_no + 1,
                fields.len()
            )));
        }
        let kind = match fields[0] {
            "tool" => DepKind::Tool,
            "font-raw" => DepKind::FontRaw,
            "font" => DepKind::FontZip,
            other => {
                return Err(KisekiError::other(format!(
                    "unknown dependency kind `{other}` on line {}",
                    line_no + 1
                )));
            }
        };
        let sha256 = fields[3].to_ascii_lowercase();
        if !is_sha256_hex(&sha256) {
            return Err(KisekiError::other(format!(
                "invalid SHA-256 digest on line {}: `{}`",
                line_no + 1,
                fields[3]
            )));
        }
        rows.push(DepRow {
            kind,
            platform: fields[1].to_string(),
            url: fields[2].to_string(),
            sha256,
            dest: fields[4].to_string(),
            mode: fields[5].to_string(),
        });
    }

    Ok(DepManifest {
        font_dirs,
        font_patterns,
        rows,
    })
}

pub fn detect_dep_platform(platform: &dyn Platform) -> Result<String> {
    if let Some(override_plat) = platform
        .env_var("KISEKI_DEP_PLATFORM")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return Ok(override_plat);
    }

    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => {
            return Err(KisekiError::other(format!(
                "unsupported CPU architecture `{other}`"
            )));
        }
    };

    let plat = match platform.os() {
        OsKind::MacOs => format!("darwin-{arch}"),
        OsKind::Windows => format!("windows-{arch}"),
        OsKind::Linux => {
            let musl_only = is_musl_only_linux(arch);
            if musl_only {
                format!("linux-musl-{arch}")
            } else {
                format!("linux-{arch}")
            }
        }
    };
    Ok(plat)
}

fn is_musl_only_linux(arch: &str) -> bool {
    let glibc_present = std::process::Command::new("getconf")
        .arg("GNU_LIBC_VERSION")
        .output()
        .is_ok_and(|o| o.status.success());
    if glibc_present {
        return false;
    }
    let loader = match arch {
        "arm64" => "/lib/ld-musl-aarch64.so.1",
        "amd64" => "/lib/ld-musl-x86_64.so.1",
        _ => return false,
    };
    Path::new(loader).exists()
}

fn font_os_key(dep_platform: &str) -> &'static str {
    if dep_platform.starts_with("darwin-") {
        "macos"
    } else if dep_platform.starts_with("windows-") {
        "windows"
    } else {
        "linux"
    }
}

fn resolve_font_dir(
    platform: &dyn Platform,
    manifest: &DepManifest,
    font_os: &str,
) -> Option<PathBuf> {
    if let Some(override_dir) = platform
        .env_var("KISEKI_FONT_DIR")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return Some(PathBuf::from(override_dir));
    }
    let raw = manifest.font_dirs.get(font_os)?;
    if let Some(rest) = raw.strip_prefix("~/") {
        let home = platform
            .env_var("HOME")
            .or_else(|| platform.env_var("USERPROFILE"))?;
        return Some(PathBuf::from(home).join(rest));
    }
    if raw.contains("%LOCALAPPDATA%") {
        let local = platform.env_var("LOCALAPPDATA")?;
        let expanded = raw.replace("%LOCALAPPDATA%", &local);
        return Some(PathBuf::from(expanded));
    }
    Some(PathBuf::from(raw))
}

fn vendor_root_dir(platform: &dyn Platform, paths: &KisekiPaths) -> PathBuf {
    if platform.os() == OsKind::Windows {
        if let Some(install_dir) = platform
            .env_var("KISEKI_INSTALL_DIR")
            .filter(|v| !v.is_empty())
        {
            return PathBuf::from(install_dir).join("vendor");
        }
        if let Some(local) = platform
            .env_var("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
        {
            return PathBuf::from(local).join("Programs/kiseki/vendor");
        }
    }
    paths.config_dir().join("vendor")
}

/// Maps a `deps.tsv` row to its stable tool or font catalog ID.
pub fn catalog_id_for_row(row: &DepRow) -> String {
    match row.kind {
        DepKind::Tool => Path::new(&row.dest)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&row.dest)
            .to_string(),
        DepKind::FontZip => row
            .dest
            .strip_prefix("fonts/")
            .unwrap_or(&row.dest)
            .to_string(),
        DepKind::FontRaw => {
            let file_name = row.dest.strip_prefix("fonts/").unwrap_or(&row.dest);
            for font in deps::FONTS {
                if let deps::FontSource::RawFiles { files, .. } = font.source {
                    if files.contains(&file_name) {
                        return font.id.to_string();
                    }
                }
            }
            file_name.to_string()
        }
    }
}

fn display_label_for_row(row: &DepRow) -> String {
    match row.kind {
        DepKind::Tool => catalog_id_for_row(row),
        DepKind::FontZip => row
            .dest
            .strip_prefix("fonts/")
            .unwrap_or(&row.dest)
            .to_string(),
        DepKind::FontRaw => {
            let name = row.dest.strip_prefix("fonts/").unwrap_or(&row.dest);
            let stem = name
                .strip_suffix(".ttf")
                .or_else(|| name.strip_suffix(".otf"))
                .unwrap_or(name);
            stem.replace("NerdFont", "NF")
        }
    }
}

fn select_rows(
    manifest: &DepManifest,
    dep_platform: &str,
    options: &DepsInstallOptions,
) -> Result<Vec<DepRow>> {
    let font_filter: BTreeSet<String> = options
        .fonts
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    if !font_filter.is_empty() {
        let known: BTreeSet<&str> = deps::FONTS.iter().map(|f| f.id).collect();
        for requested in &font_filter {
            if !known.contains(requested.as_str()) {
                let valid: Vec<&str> = known.into_iter().collect();
                return Err(KisekiError::other(format!(
                    "unknown font ID `{requested}`; available font IDs: {}",
                    valid.join(", ")
                )));
            }
        }
    }

    let mut selected = Vec::new();
    for row in &manifest.rows {
        if row.platform != dep_platform {
            continue;
        }
        if !font_filter.is_empty() {
            if !row.kind.is_font() {
                continue;
            }
            let id = catalog_id_for_row(row);
            if !font_filter.contains(&id) {
                continue;
            }
        } else {
            if options.only_fonts && !row.kind.is_font() {
                continue;
            }
            if (options.only_tools || options.skip_fonts) && row.kind.is_font() {
                continue;
            }
        }
        selected.push(row.clone());
    }
    Ok(selected)
}

fn plan_jobs(
    rows: &[DepRow],
    manifest: &DepManifest,
    vendor_bin_dir: &Path,
    font_dir: Option<&Path>,
    receipts_dir: &Path,
    force: bool,
) -> Result<Vec<PlannedJob>> {
    let mut jobs = Vec::with_capacity(rows.len());
    for row in rows {
        validate_row_security(row)?;
        let id = catalog_id_for_row(row);
        let label = display_label_for_row(row);
        let (font_pattern, up_to_date, existing_files) = match row.kind {
            DepKind::Tool => {
                let file_name = Path::new(&row.dest)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| KisekiError::other(format!("invalid tool dest `{}`", row.dest)))?;
                let dest = vendor_bin_dir.join(file_name);
                let is_fastfetch = file_name == "fastfetch" || file_name == "fastfetch.exe";
                let ready = if force {
                    false
                } else if is_fastfetch {
                    let receipt = fastfetch_receipt_path(&dest);
                    is_executable_file(&dest)
                        && fs::read_to_string(&receipt)
                            .map(|s| s.trim() == row.sha256)
                            .unwrap_or(false)
                } else {
                    is_executable_file(&dest)
                        && sha256_file(&dest).is_ok_and(|h| h == row.sha256)
                };
                let files = if ready { vec![dest] } else { Vec::new() };
                (None, ready, files)
            }
            DepKind::FontRaw => {
                let fdir = font_dir.ok_or_else(|| {
                    KisekiError::other("could not determine user font directory for this platform")
                })?;
                let file_name = row.dest.strip_prefix("fonts/").unwrap_or(&row.dest);
                let dest = fdir.join(file_name);
                let ready = !force
                    && dest.is_file()
                    && sha256_file(&dest).is_ok_and(|h| h == row.sha256);
                let files = if ready { vec![dest] } else { Vec::new() };
                (None, ready, files)
            }
            DepKind::FontZip => {
                let fdir = font_dir.ok_or_else(|| {
                    KisekiError::other("could not determine user font directory for this platform")
                })?;
                let pattern = manifest
                    .font_patterns
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| {
                        KisekiError::other(format!("missing `# font` pattern metadata for `{id}`"))
                    })?;
                let (ready, files) = if force {
                    (false, Vec::new())
                } else {
                    check_font_zip_receipt(receipts_dir, &id, &row.sha256, fdir)
                };
                (Some(pattern), ready, files)
            }
        };
        jobs.push(PlannedJob {
            label,
            id,
            row: row.clone(),
            font_pattern,
            up_to_date,
            existing_files,
        });
    }
    Ok(jobs)
}

fn validate_row_security(row: &DepRow) -> Result<()> {
    match row.kind {
        DepKind::Tool => {
            let allowed = matches!(
                row.dest.as_str(),
                "vendor/bin/oh-my-posh"
                    | "vendor/bin/fastfetch"
                    | "vendor/bin/oh-my-posh.exe"
                    | "vendor/bin/fastfetch.exe"
            );
            if !allowed || row.mode != "0755" {
                return Err(KisekiError::other(format!(
                    "unexpected tool destination or mode: {} {}",
                    row.dest, row.mode
                )));
            }
        }
        DepKind::FontRaw => {
            let Some(name) = row.dest.strip_prefix("fonts/") else {
                return Err(KisekiError::other(format!(
                    "unexpected font-raw destination: {}",
                    row.dest
                )));
            };
            if row.mode != "0644" || !is_safe_font_filename(name) || !name.ends_with(".ttf") {
                return Err(KisekiError::other(format!(
                    "unsafe font-raw destination or mode: {} {}",
                    row.dest, row.mode
                )));
            }
        }
        DepKind::FontZip => {
            let Some(family) = row.dest.strip_prefix("fonts/") else {
                return Err(KisekiError::other(format!(
                    "unexpected font destination: {}",
                    row.dest
                )));
            };
            let valid_id = !family.is_empty()
                && family
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            if row.mode != "0644" || !valid_id {
                return Err(KisekiError::other(format!(
                    "unsafe font destination or mode: {} {}",
                    row.dest, row.mode
                )));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-job execution: mirrored streaming download + Range resume + install
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn execute_job(
    idx: usize,
    job: &PlannedJob,
    agent: &ureq::Agent,
    mirrors: &[String],
    os: OsKind,
    vendor_bin_dir: &Path,
    font_dir: Option<&Path>,
    receipts_dir: &Path,
    job_dir: &Path,
    progress: &Arc<Mutex<ProgressState>>,
) -> DepItemOutcome {
    let asset_path = job_dir.join("asset.part");
    let mut candidates: Vec<(String, String)> = mirrors
        .iter()
        .map(|origin| {
            (
                format!("{origin}/deps/{}", job.row.sha256),
                short_host(origin),
            )
        })
        .collect();
    candidates.push((job.row.url.clone(), short_host(&job.row.url)));

    let mut download_result: Option<(u64, String)> = None;
    let mut errors = Vec::new();

    for (url, mirror_label) in &candidates {
        {
            let mut st = progress.lock().unwrap();
            st.items[idx].phase = ItemPhase::Downloading;
            st.items[idx].mirror = mirror_label.clone();
            st.items[idx].downloaded = 0;
            st.items[idx].total = None;
            st.items[idx].speed_bps = 0.0;
        }
        let _ = fs::remove_file(&asset_path);
        match download_with_resume(
            agent,
            url,
            &asset_path,
            &job.row.sha256,
            idx,
            progress,
        ) {
            Ok(bytes) => {
                download_result = Some((bytes, mirror_label.clone()));
                break;
            }
            Err(err) => {
                errors.push(format!("{mirror_label}: {err}"));
            }
        }
    }

    let Some((downloaded_bytes, used_mirror)) = download_result else {
        let msg = errors.join("; ");
        {
            let mut st = progress.lock().unwrap();
            st.items[idx].phase = ItemPhase::Failed(msg.clone());
        }
        return DepItemOutcome {
            id: job.id.clone(),
            kind: job.row.kind.as_str().to_string(),
            dest: job.row.dest.clone(),
            status: "failed".to_string(),
            bytes: 0,
            mirror: None,
            files: Vec::new(),
            error: Some(msg),
        };
    };

    // Extract & install atomically.
    {
        let mut st = progress.lock().unwrap();
        st.items[idx].phase = ItemPhase::Installing;
    }

    let install_res = match job.row.kind {
        DepKind::Tool => install_tool_asset(job, &asset_path, vendor_bin_dir, job_dir, os),
        DepKind::FontRaw => {
            let fdir = font_dir.expect("validated during planning");
            install_raw_font_asset(job, &asset_path, fdir)
        }
        DepKind::FontZip => {
            let fdir = font_dir.expect("validated during planning");
            let pattern = job.font_pattern.as_deref().expect("validated during planning");
            install_zip_font_asset(job, &asset_path, pattern, fdir, receipts_dir, job_dir)
        }
    };

    match install_res {
        Ok(files) => {
            {
                let mut st = progress.lock().unwrap();
                st.items[idx].phase = ItemPhase::Done {
                    files_count: files.len(),
                    bytes: downloaded_bytes,
                };
            }
            DepItemOutcome {
                id: job.id.clone(),
                kind: job.row.kind.as_str().to_string(),
                dest: job.row.dest.clone(),
                status: "installed".to_string(),
                bytes: downloaded_bytes,
                mirror: Some(used_mirror),
                files,
                error: None,
            }
        }
        Err(err) => {
            let msg = err.to_string();
            {
                let mut st = progress.lock().unwrap();
                st.items[idx].phase = ItemPhase::Failed(msg.clone());
            }
            DepItemOutcome {
                id: job.id.clone(),
                kind: job.row.kind.as_str().to_string(),
                dest: job.row.dest.clone(),
                status: "failed".to_string(),
                bytes: downloaded_bytes,
                mirror: Some(used_mirror),
                files: Vec::new(),
                error: Some(msg),
            }
        }
    }
}

/// Streams a single candidate URL to `dest_path`, computing SHA-256 on the fly
/// and automatically resuming via HTTP `Range` on transient mid-stream I/O errors.
fn download_with_resume(
    agent: &ureq::Agent,
    url: &str,
    dest_path: &Path,
    expected_sha256: &str,
    idx: usize,
    progress: &Arc<Mutex<ProgressState>>,
) -> Result<u64> {
    let mut offset: u64 = 0;
    let mut expected_total: Option<u64> = None;
    let mut hasher = Sha256::new();
    let mut resume_attempts: usize = 0;
    let mut buf = vec![0u8; CHUNK_SIZE];

    loop {
        let mut req = agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .header("Accept-Encoding", "identity");
        if offset > 0 {
            req = req.header("Range", &format!("bytes={offset}-"));
        }

        let resp = match req.call() {
            Ok(resp) => resp,
            Err(err) => {
                if offset > 0 && resume_attempts < MAX_RESUME_ATTEMPTS {
                    resume_attempts += 1;
                    {
                        let mut st = progress.lock().unwrap();
                        st.items[idx].phase = ItemPhase::Retrying {
                            attempt: resume_attempts,
                            max: MAX_RESUME_ATTEMPTS,
                        };
                    }
                    std::thread::sleep(Duration::from_millis(200 * resume_attempts as u64));
                    continue;
                }
                return Err(KisekiError::other(format!("request failed: {err}")));
            }
        };

        let status = resp.status().as_u16();
        let mut file = match status {
            200 => {
                // Full response: reset offset and hasher (handles servers ignoring Range).
                offset = 0;
                hasher = Sha256::new();
                expected_total = resp
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse::<u64>().ok());
                File::create(dest_path)
                    .map_err(|e| KisekiError::other(format!("create staging file: {e}")))?
            }
            206 if offset > 0 => {
                let cr = resp
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let expected_prefix = format!("bytes {offset}-");
                if !cr.starts_with(&expected_prefix) {
                    return Err(KisekiError::other(format!(
                        "unexpected Content-Range `{cr}` for offset {offset}"
                    )));
                }
                if expected_total.is_none() {
                    if let Some((_, total_str)) = cr.rsplit_once('/') {
                        expected_total = total_str.trim().parse::<u64>().ok();
                    }
                }
                OpenOptions::new()
                    .append(true)
                    .open(dest_path)
                    .map_err(|e| KisekiError::other(format!("reopen staging file: {e}")))?
            }
            416 if offset > 0 => {
                // Range not satisfiable; start cleanly from byte 0 once.
                offset = 0;
                hasher = Sha256::new();
                let _ = fs::remove_file(dest_path);
                if resume_attempts < MAX_RESUME_ATTEMPTS {
                    resume_attempts += 1;
                    continue;
                }
                return Err(KisekiError::other("HTTP 416 Range Not Satisfiable"));
            }
            other => {
                return Err(KisekiError::other(format!("HTTP {other}")));
            }
        };

        {
            let mut st = progress.lock().unwrap();
            st.items[idx].phase = ItemPhase::Downloading;
            st.items[idx].downloaded = offset;
            st.items[idx].total = expected_total;
        }

        let mut reader = resp.into_body().into_reader();
        let mut last_tick = Instant::now();
        let mut bytes_since_tick: u64 = 0;
        let mut stream_err: Option<io::Error> = None;

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    file.write_all(&buf[..n])
                        .map_err(|e| KisekiError::other(format!("write staging file: {e}")))?;
                    hasher.update(&buf[..n]);
                    offset += n as u64;
                    bytes_since_tick += n as u64;

                    let elapsed = last_tick.elapsed();
                    if elapsed >= Duration::from_millis(120) {
                        let inst_bps = bytes_since_tick as f64 / elapsed.as_secs_f64();
                        let mut st = progress.lock().unwrap();
                        let item = &mut st.items[idx];
                        item.downloaded = offset;
                        item.speed_bps = if item.speed_bps <= 0.0 {
                            inst_bps
                        } else {
                            item.speed_bps * 0.65 + inst_bps * 0.35
                        };
                        last_tick = Instant::now();
                        bytes_since_tick = 0;
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => {
                    stream_err = Some(err);
                    break;
                }
            }
        }
        let _ = file.flush();
        drop(file);

        {
            let mut st = progress.lock().unwrap();
            st.items[idx].downloaded = offset;
        }

        if let Some(err) = stream_err {
            if offset > 0 && resume_attempts < MAX_RESUME_ATTEMPTS {
                resume_attempts += 1;
                {
                    let mut st = progress.lock().unwrap();
                    st.items[idx].phase = ItemPhase::Retrying {
                        attempt: resume_attempts,
                        max: MAX_RESUME_ATTEMPTS,
                    };
                }
                std::thread::sleep(Duration::from_millis(200 * resume_attempts as u64));
                continue;
            }
            return Err(KisekiError::other(format!("stream read error: {err}")));
        }

        // Check for premature truncation when server announced exact Content-Length.
        if let Some(total) = expected_total {
            if offset < total {
                if offset > 0 && resume_attempts < MAX_RESUME_ATTEMPTS {
                    resume_attempts += 1;
                    {
                        let mut st = progress.lock().unwrap();
                        st.items[idx].phase = ItemPhase::Retrying {
                            attempt: resume_attempts,
                            max: MAX_RESUME_ATTEMPTS,
                        };
                    }
                    std::thread::sleep(Duration::from_millis(200 * resume_attempts as u64));
                    continue;
                }
                return Err(KisekiError::other(format!(
                    "truncated download: received {offset} of {total} bytes"
                )));
            }
        }

        // Verify SHA-256 strictly before leaving download step.
        {
            let mut st = progress.lock().unwrap();
            st.items[idx].phase = ItemPhase::Verifying;
            st.items[idx].total = Some(offset);
            st.items[idx].downloaded = offset;
        }
        let actual_sha = encode_hex(&hasher.finalize());
        if actual_sha != expected_sha256 {
            return Err(KisekiError::other(format!(
                "SHA-256 mismatch (expected {expected_sha256}, got {actual_sha})"
            )));
        }
        return Ok(offset);
    }
}

// ---------------------------------------------------------------------------
// Extraction & atomic file installation
// ---------------------------------------------------------------------------

fn install_tool_asset(
    job: &PlannedJob,
    asset_path: &Path,
    vendor_bin_dir: &Path,
    job_dir: &Path,
    os: OsKind,
) -> Result<Vec<PathBuf>> {
    let file_name = Path::new(&job.row.dest)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| KisekiError::other("invalid tool filename"))?;
    let dest = vendor_bin_dir.join(file_name);
    let staged_bin = job_dir.join(file_name);
    let is_fastfetch = file_name == "fastfetch" || file_name == "fastfetch.exe";

    if is_fastfetch {
        extract_fastfetch_from_zip(asset_path, &staged_bin, file_name)?;
    } else {
        fs::copy(asset_path, &staged_bin)
            .map_err(|e| KisekiError::other(format!("copy staged tool: {e}")))?;
    }

    set_executable(&staged_bin)?;

    // Health check before replacing any existing live binary.
    let status = std::process::Command::new(&staged_bin)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| KisekiError::other(format!("failed running `{file_name} --version`: {e}")))?;
    if !status.success() {
        return Err(KisekiError::other(format!(
            "`{file_name} --version` failed with status {status}"
        )));
    }

    atomic_install_file(&staged_bin, &dest, true)?;

    if is_fastfetch {
        let receipt = fastfetch_receipt_path(&dest);
        let tmp_receipt = job_dir.join("fastfetch.receipt");
        let content = format!("{}\n", job.row.sha256);
        fs::write(&tmp_receipt, content)
            .map_err(|e| KisekiError::other(format!("write fastfetch receipt: {e}")))?;
        let _ = atomic_install_file(&tmp_receipt, &receipt, false);
    }
    let _ = os;
    Ok(vec![dest])
}

fn extract_fastfetch_from_zip(zip_path: &Path, out_path: &Path, bin_name: &str) -> Result<()> {
    let file =
        File::open(zip_path).map_err(|e| KisekiError::other(format!("open fastfetch zip: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| KisekiError::other(format!("invalid fastfetch zip: {e}")))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| KisekiError::other(format!("read zip entry {i}: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if has_unsafe_zip_path(&name) {
            continue;
        }
        let matches = if bin_name == "fastfetch.exe" {
            name == "fastfetch.exe" || name.ends_with("/fastfetch.exe")
        } else {
            name.ends_with("/usr/bin/fastfetch") || name == "usr/bin/fastfetch" || name == "fastfetch"
        };
        if matches {
            copy_bounded_zip_entry(&mut entry, out_path, MAX_SINGLE_EXTRACTED_BYTES)?;
            return Ok(());
        }
    }
    Err(KisekiError::other(format!(
        "`{bin_name}` executable not found inside fastfetch archive"
    )))
}

fn install_raw_font_asset(
    job: &PlannedJob,
    asset_path: &Path,
    font_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let name = job
        .row
        .dest
        .strip_prefix("fonts/")
        .ok_or_else(|| KisekiError::other("invalid font-raw path"))?;
    if !is_safe_font_filename(name) {
        return Err(KisekiError::other(format!("unsafe font filename `{name}`")));
    }
    let dest = font_dir.join(name);
    atomic_install_file(asset_path, &dest, false)?;
    Ok(vec![dest])
}

fn install_zip_font_asset(
    job: &PlannedJob,
    asset_path: &Path,
    pattern_str: &str,
    font_dir: &Path,
    receipts_dir: &Path,
    job_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let regex = Regex::new(pattern_str).map_err(|e| {
        KisekiError::other(format!(
            "invalid font regex `{pattern_str}` for `{}`: {e}",
            job.id
        ))
    })?;
    let file =
        File::open(asset_path).map_err(|e| KisekiError::other(format!("open font zip: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| KisekiError::other(format!("invalid font zip for `{}`: {e}", job.id)))?;

    let mut installed_files = Vec::new();
    let mut installed_names = Vec::new();
    let mut seen_names = BTreeSet::new();
    let mut total_extracted: u64 = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| KisekiError::other(format!("read font zip entry {i}: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let raw_name = entry.name().replace('\\', "/");
        if has_unsafe_zip_path(&raw_name) {
            continue;
        }
        let base_name = raw_name.rsplit('/').next().unwrap_or(&raw_name).to_string();
        if !is_safe_font_filename(&base_name) || !regex.is_match(&base_name) {
            continue;
        }
        if !seen_names.insert(base_name.clone()) {
            continue;
        }

        let staged_face = job_dir.join(&base_name);
        let written = copy_bounded_zip_entry(&mut entry, &staged_face, MAX_SINGLE_EXTRACTED_BYTES)?;
        if written == 0 {
            return Err(KisekiError::other(format!(
                "extracted empty font file `{base_name}` from `{}`",
                job.id
            )));
        }
        total_extracted = total_extracted.saturating_add(written);
        if total_extracted > MAX_TOTAL_EXTRACTED_BYTES {
            return Err(KisekiError::other(format!(
                "font archive `{}` exceeded max total extracted size ({MAX_TOTAL_EXTRACTED_BYTES} bytes)",
                job.id
            )));
        }

        let dest = font_dir.join(&base_name);
        atomic_install_file(&staged_face, &dest, false)?;
        let _ = fs::remove_file(&staged_face);
        installed_names.push(base_name);
        installed_files.push(dest);
    }

    if installed_files.is_empty() {
        return Err(KisekiError::other(format!(
            "no font files matched pattern `{pattern_str}` in `{}`",
            job.id
        )));
    }

    write_font_zip_receipt(receipts_dir, &job.id, &job.row.sha256, &installed_names, job_dir)?;
    Ok(installed_files)
}

fn copy_bounded_zip_entry<R: Read>(reader: &mut R, out_path: &Path, max_bytes: u64) -> Result<u64> {
    let mut out = File::create(out_path)
        .map_err(|e| KisekiError::other(format!("create extracted file: {e}")))?;
    let mut buf = [0u8; 32 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| KisekiError::other(format!("extract zip entry: {e}")))?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n as u64);
        if total > max_bytes {
            return Err(KisekiError::other(format!(
                "zip entry exceeded max allowed uncompressed size ({max_bytes} bytes)"
            )));
        }
        out.write_all(&buf[..n])
            .map_err(|e| KisekiError::other(format!("write extracted file: {e}")))?;
    }
    out.flush()
        .map_err(|e| KisekiError::other(format!("flush extracted file: {e}")))?;
    Ok(total)
}

pub fn has_unsafe_zip_path(path: &str) -> bool {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('*')
        || path.contains('?')
        || path.contains('[')
        || path.contains(':')
        || path.contains('\0')
    {
        return true;
    }
    path.split('/').any(|seg| seg == "..")
}

pub fn is_safe_font_filename(name: &str) -> bool {
    if name.is_empty() || name.starts_with('.') || name.contains("..") {
        return false;
    }
    name.bytes().all(|b| {
        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b' ')
    })
}

fn atomic_install_file(src: &Path, dest: &Path, executable: bool) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            KisekiError::other(format!("create directory `{}`: {e}", parent.display()))
        })?;
    }
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let file_name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("asset");
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let staged = parent.join(format!(".{file_name}.tmp.{pid}.{nanos}"));
    let copy_res = fs::copy(src, &staged)
        .map_err(|e| KisekiError::other(format!("stage `{}`: {e}", dest.display())));
    if let Err(err) = copy_res {
        let _ = fs::remove_file(&staged);
        return Err(err);
    }
    if executable {
        if let Err(err) = set_executable(&staged) {
            let _ = fs::remove_file(&staged);
            return Err(err);
        }
    }
    if let Err(err) = replace_file(&staged, dest) {
        let _ = fs::remove_file(&staged);
        return Err(err);
    }
    Ok(())
}

fn replace_file(staged: &Path, dest: &Path) -> Result<()> {
    match fs::rename(staged, dest) {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(_) => {
            if dest.exists() {
                let _ = fs::remove_file(dest);
            }
            fs::rename(staged, dest).map_err(|e| {
                KisekiError::other(format!("replace `{}`: {e}", dest.display()))
            })
        }
        #[cfg(not(windows))]
        Err(e) => Err(KisekiError::other(format!(
            "replace `{}`: {e}",
            dest.display()
        ))),
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|e| KisekiError::other(format!("chmod +x `{}`: {e}", path.display())))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn fastfetch_receipt_path(dest: &Path) -> PathBuf {
    let mut os = dest.as_os_str().to_os_string();
    os.push(".asset-sha256");
    PathBuf::from(os)
}

fn font_zip_receipt_path(receipts_dir: &Path, family_id: &str) -> PathBuf {
    receipts_dir.join(format!("font-{family_id}.asset-sha256"))
}

fn check_font_zip_receipt(
    receipts_dir: &Path,
    family_id: &str,
    expected_sha256: &str,
    font_dir: &Path,
) -> (bool, Vec<PathBuf>) {
    let receipt_path = font_zip_receipt_path(receipts_dir, family_id);
    let Ok(raw) = fs::read_to_string(&receipt_path) else {
        return (false, Vec::new());
    };
    let mut matched_hash = false;
    let mut files = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(hash) = trimmed.strip_prefix("sha256:") {
            if hash.trim() == expected_sha256 {
                matched_hash = true;
            }
        } else if let Some(fname) = trimmed.strip_prefix("file:") {
            let fname = fname.trim();
            if !is_safe_font_filename(fname) {
                return (false, Vec::new());
            }
            let p = font_dir.join(fname);
            let exists_non_empty = fs::metadata(&p)
                .map(|m| m.is_file() && m.len() > 0)
                .unwrap_or(false);
            if !exists_non_empty {
                return (false, Vec::new());
            }
            files.push(p);
        }
    }
    if matched_hash && !files.is_empty() {
        (true, files)
    } else {
        (false, Vec::new())
    }
}

fn write_font_zip_receipt(
    receipts_dir: &Path,
    family_id: &str,
    sha256: &str,
    file_names: &[String],
    job_dir: &Path,
) -> Result<()> {
    let mut body = format!("sha256:{sha256}\n");
    for name in file_names {
        body.push_str("file:");
        body.push_str(name);
        body.push('\n');
    }
    let tmp = job_dir.join(format!("font-{family_id}.receipt"));
    fs::write(&tmp, body)
        .map_err(|e| KisekiError::other(format!("write font receipt: {e}")))?;
    let dest = font_zip_receipt_path(receipts_dir, family_id);
    atomic_install_file(&tmp, &dest, false)
}

fn download_ofl_license(mirrors: &[String], dest: &Path, timeouts: IoTimeouts) -> Result<()> {
    let agent = timeouts.meta_agent();
    for origin in mirrors {
        let url = format!("{origin}/deps/licenses/OFL-JetBrainsMono.txt");
        if let Ok(bytes) = fetch_small_bytes(&agent, &url, MAX_LICENSE_BYTES) {
            if !bytes.is_empty() {
                if let Some(parent) = dest.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let tmp = dest.with_extension("txt.tmp");
                if fs::write(&tmp, &bytes).is_ok() {
                    let _ = replace_file(&tmp, dest);
                }
                return Ok(());
            }
        }
    }
    Err(KisekiError::other("OFL license unavailable"))
}

fn finalize_platform_fonts(
    platform: &dyn Platform,
    font_dir: &Path,
    installed_files: &[PathBuf],
) -> Option<String> {
    match platform.os() {
        OsKind::MacOs => None,
        OsKind::Linux => {
            if platform.env_var("KISEKI_FONT_DIR").is_some() {
                // Hermetic test override: avoid polluting host fontconfig cache.
                return None;
            }
            let status = std::process::Command::new("fc-cache")
                .arg("-f")
                .arg(font_dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            match status {
                Ok(s) if s.success() => None,
                _ => Some("无法刷新字体缓存，可手动运行 `fc-cache -f`。".to_string()),
            }
        }
        OsKind::Windows => {
            if platform.env_var("KISEKI_FONT_DIR").is_some() {
                return None;
            }
            #[cfg(windows)]
            {
                if let Err(err) = register_windows_fonts(installed_files) {
                    return Some(format!("部分字体注册失败: {err}"));
                }
            }
            #[cfg(not(windows))]
            let _ = installed_files;
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Windows native font registration & timezone helper (zero extra crates)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn register_windows_fonts(files: &[PathBuf]) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const HKEY_CURRENT_USER: isize = 0x80000001u32 as isize;
    const KEY_SET_VALUE: u32 = 0x0002;
    const REG_SZ: u32 = 1;
    const HWND_BROADCAST: isize = 0xffff;
    const WM_FONTCHANGE: u32 = 0x001D;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;

    #[link(name = "gdi32")]
    extern "system" {
        fn AddFontResourceW(name: *const u16) -> i32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn SendMessageTimeoutW(
            hwnd: isize,
            msg: u32,
            wparam: usize,
            lparam: isize,
            flags: u32,
            timeout: u32,
            result: *mut usize,
        ) -> isize;
    }
    #[link(name = "advapi32")]
    extern "system" {
        fn RegCreateKeyExW(
            hkey: isize,
            subkey: *const u16,
            reserved: u32,
            class: *const u16,
            options: u32,
            sam: u32,
            sec: *const std::ffi::c_void,
            result: *mut isize,
            disposition: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            hkey: isize,
            value_name: *const u16,
            reserved: u32,
            value_type: u32,
            data: *const u8,
            cb_data: u32,
        ) -> i32;
        fn RegCloseKey(hkey: isize) -> i32;
    }

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    let subkey = wide(OsStr::new(
        r"Software\Microsoft\Windows NT\CurrentVersion\Fonts",
    ));
    let mut hkey: isize = 0;
    let mut disp: u32 = 0;
    // RegCreateKeyExW opens the key if it exists without clearing existing values.
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut hkey,
            &mut disp,
        )
    };
    if rc != 0 {
        return Err(KisekiError::other(format!(
            "RegCreateKeyExW failed with error {rc}"
        )));
    }

    let mut any_added = false;
    for path in files {
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(file_name);
        let format_label = if file_name.to_ascii_lowercase().ends_with(".otf") {
            "OpenType"
        } else {
            "TrueType"
        };
        let display_name = format!("{stem} ({format_label})");
        let w_path = wide(path.as_os_str());
        let w_name = wide(OsStr::new(&display_name));

        unsafe {
            let _ = AddFontResourceW(w_path.as_ptr());
            let byte_len = (w_path.len() * 2) as u32;
            let _ = RegSetValueExW(
                hkey,
                w_name.as_ptr(),
                0,
                REG_SZ,
                w_path.as_ptr() as *const u8,
                byte_len,
            );
        }
        any_added = true;
    }

    unsafe {
        let _ = RegCloseKey(hkey);
        if any_added {
            let mut ignored: usize = 0;
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_FONTCHANGE,
                0,
                0,
                SMTO_ABORTIFHUNG,
                1000,
                &mut ignored,
            );
        }
    }
    Ok(())
}

#[cfg(windows)]
fn windows_is_utc_plus_8() -> bool {
    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: SystemTime,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: SystemTime,
        daylight_bias: i32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetTimeZoneInformation(lp_tzi: *mut TimeZoneInformation) -> u32;
    }
    let mut tzi: TimeZoneInformation = unsafe { std::mem::zeroed() };
    let res = unsafe { GetTimeZoneInformation(&mut tzi) };
    res != 0xFFFF_FFFF && tzi.bias == -480
}

// ---------------------------------------------------------------------------
// Live multi-line progress bar UI (zero Nerd Font dependency)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum ItemPhase {
    Queued,
    Downloading,
    Retrying { attempt: usize, max: usize },
    Verifying,
    Installing,
    UpToDate,
    Done { files_count: usize, bytes: u64 },
    Failed(String),
}

#[derive(Debug, Clone)]
struct ItemProgress {
    label: String,
    kind: DepKind,
    phase: ItemPhase,
    downloaded: u64,
    total: Option<u64>,
    speed_bps: f64,
    mirror: String,
}

#[derive(Debug)]
struct ProgressState {
    started: Instant,
    items: Vec<ItemProgress>,
}

impl ProgressState {
    fn new(jobs: &[PlannedJob]) -> Self {
        let items = jobs
            .iter()
            .map(|job| ItemProgress {
                label: job.label.clone(),
                kind: job.row.kind,
                phase: ItemPhase::Queued,
                downloaded: 0,
                total: None,
                speed_bps: 0.0,
                mirror: String::new(),
            })
            .collect();
        Self {
            started: Instant::now(),
            items,
        }
    }
}

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

struct LiveRenderer {
    color: bool,
    frame: usize,
    rendered_lines: usize,
    cursor_hidden: bool,
}

impl LiveRenderer {
    fn new(color: bool) -> Self {
        Self {
            color,
            frame: 0,
            rendered_lines: 0,
            cursor_hidden: false,
        }
    }

    fn render(&mut self, state: &ProgressState, is_final: bool) {
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(100)
            .max(50);

        let lines = format_progress_frame(state, self.frame, width, self.color, is_final);
        self.frame = self.frame.wrapping_add(1);

        let mut stderr = io::stderr().lock();
        if !self.cursor_hidden {
            let _ = write!(stderr, "\x1b[?25l");
            self.cursor_hidden = true;
        }
        if self.rendered_lines > 0 {
            let _ = write!(stderr, "\x1b[{}A", self.rendered_lines);
        }
        for line in &lines {
            let _ = write!(stderr, "\r\x1b[2K{line}\n");
        }
        if is_final && self.cursor_hidden {
            let _ = write!(stderr, "\x1b[?25h");
            self.cursor_hidden = false;
        }
        let _ = stderr.flush();
        self.rendered_lines = lines.len();
    }
}

impl Drop for LiveRenderer {
    fn drop(&mut self) {
        if self.cursor_hidden {
            let mut stderr = io::stderr().lock();
            let _ = write!(stderr, "\x1b[?25h");
            let _ = stderr.flush();
            self.cursor_hidden = false;
        }
    }
}

fn format_progress_frame(
    state: &ProgressState,
    frame: usize,
    width: usize,
    color: bool,
    is_final: bool,
) -> Vec<String> {
    let spinner = SPINNER[frame % SPINNER.len()];
    let bar_width: usize = if width >= 96 { 16 } else if width >= 76 { 10 } else { 8 };
    let label_width: usize = if width >= 76 { 24 } else { 18 };
    let show_mirror = width >= 108;

    let mut lines = Vec::with_capacity(state.items.len() + 2);

    for item in &state.items {
        let label = truncate_ascii_or_chars(&item.label, label_width);
        let padded_label = format!("{label:<label_width$}");
        let line = match &item.phase {
            ItemPhase::Queued => {
                format!("  {} {}  {}", paint(color, "2", "○"), paint(color, "2", &padded_label), paint(color, "2", "等待中…"))
            }
            ItemPhase::Downloading => {
                let icon = paint(color, "1;36", spinner);
                let progress_text = match item.total {
                    Some(total) if total > 0 => {
                        let ratio = (item.downloaded as f64 / total as f64).clamp(0.0, 1.0);
                        let pct = (ratio * 100.0).round() as u32;
                        let bar = render_bar(ratio, bar_width, color);
                        format!(
                            "{bar} {:>3}% {:>8}/{:<8}",
                            pct,
                            format_bytes(item.downloaded),
                            format_bytes(total)
                        )
                    }
                    _ => {
                        let bar = render_indeterminate_bar(frame, bar_width, color);
                        format!("{bar}  --- {:>8}", format_bytes(item.downloaded))
                    }
                };
                let speed = if item.speed_bps >= 1.0 {
                    format!(" {:>9}", format_speed(item.speed_bps))
                } else {
                    String::new()
                };
                let mirror = if show_mirror && !item.mirror.is_empty() {
                    format!(" {}", paint(color, "2", &item.mirror))
                } else {
                    String::new()
                };
                format!("  {icon} {padded_label} {progress_text}{speed}{mirror}")
            }
            ItemPhase::Retrying { attempt, max } => {
                let icon = paint(color, "1;33", spinner);
                let bar = match item.total {
                    Some(total) if total > 0 => {
                        render_bar((item.downloaded as f64 / total as f64).clamp(0.0, 1.0), bar_width, color)
                    }
                    _ => render_indeterminate_bar(frame, bar_width, color),
                };
                format!(
                    "  {icon} {padded_label}  {bar}  {}",
                    paint(color, "1;33", &format!("断点续传 ({attempt}/{max})…"))
                )
            }
            ItemPhase::Verifying => {
                let icon = paint(color, "1;36", spinner);
                let bar = render_bar(1.0, bar_width, color);
                format!("  {icon} {padded_label}  {bar}  {}", paint(color, "1;36", "校验 SHA-256…"))
            }
            ItemPhase::Installing => {
                let icon = paint(color, "1;36", spinner);
                let bar = render_bar(1.0, bar_width, color);
                let verb = if item.kind == DepKind::FontZip {
                    "解压安装字体…"
                } else {
                    "安装中…"
                };
                format!("  {icon} {padded_label}  {bar}  {}", paint(color, "1;36", verb))
            }
            ItemPhase::UpToDate => {
                format!(
                    "  {} {}  {}",
                    paint(color, "1;32", "✓"),
                    padded_label,
                    paint(color, "2", "已就绪（跳过）")
                )
            }
            ItemPhase::Done { files_count, bytes } => {
                let detail = if item.kind == DepKind::FontZip && *files_count > 1 {
                    format!("已安装 ({files_count} 个字重 · {})", format_bytes(*bytes))
                } else {
                    format!("已安装 ({})", format_bytes(*bytes))
                };
                format!(
                    "  {} {}  {}",
                    paint(color, "1;32", "✓"),
                    padded_label,
                    paint(color, "32", &detail)
                )
            }
            ItemPhase::Failed(err) => {
                let short_err = truncate_ascii_or_chars(err, width.saturating_sub(label_width + 14).max(16));
                format!(
                    "  {} {}  {}",
                    paint(color, "1;31", "✗"),
                    padded_label,
                    paint(color, "1;31", &format!("失败: {short_err}"))
                )
            }
        };
        lines.push(line);
    }

    // Aggregate footer line.
    let total_items = state.items.len();
    let done_items = state
        .items
        .iter()
        .filter(|i| {
            matches!(
                i.phase,
                ItemPhase::Done { .. } | ItemPhase::UpToDate | ItemPhase::Failed(_)
            )
        })
        .count();
    let failed_items = state
        .items
        .iter()
        .filter(|i| matches!(i.phase, ItemPhase::Failed(_)))
        .count();
    let active_items = state
        .items
        .iter()
        .filter(|i| !matches!(i.phase, ItemPhase::UpToDate))
        .collect::<Vec<_>>();

    let total_downloaded: u64 = active_items
        .iter()
        .map(|i| match i.phase {
            ItemPhase::Done { bytes, .. } => bytes,
            _ => i.downloaded,
        })
        .sum();
    let known_total: u64 = active_items
        .iter()
        .filter_map(|i| match i.phase {
            ItemPhase::Done { bytes, .. } => Some(bytes),
            _ => i.total,
        })
        .sum();
    let all_totals_known = !active_items.is_empty()
        && active_items
            .iter()
            .all(|i| matches!(i.phase, ItemPhase::Done { .. } | ItemPhase::Failed(_)) || i.total.is_some());
    let agg_speed: f64 = active_items
        .iter()
        .filter(|i| matches!(i.phase, ItemPhase::Downloading))
        .map(|i| i.speed_bps)
        .sum();

    let sep_len = width.min(72).saturating_sub(2);
    lines.push(format!("  {}", paint(color, "2", &"─".repeat(sep_len))));

    let elapsed = state.started.elapsed().as_secs_f64();
    if is_final {
        let installed = state
            .items
            .iter()
            .filter(|i| matches!(i.phase, ItemPhase::Done { .. }))
            .count();
        let skipped = state
            .items
            .iter()
            .filter(|i| matches!(i.phase, ItemPhase::UpToDate))
            .count();
        let badge = if failed_items == 0 {
            paint(color, "1;32", "✓")
        } else {
            paint(color, "1;33", "!")
        };
        lines.push(format!(
            "  {badge} 完成: {installed} 新安装 · {skipped} 已就绪 · {failed_items} 失败 · 共下载 {} · 用时 {elapsed:.1}s",
            format_bytes(total_downloaded)
        ));
    } else {
        let ratio = if all_totals_known && known_total > 0 {
            (total_downloaded as f64 / known_total as f64).clamp(0.0, 1.0)
        } else if total_items > 0 {
            (done_items as f64 / total_items as f64).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let pct = (ratio * 100.0).round() as u32;
        let bar = render_bar(ratio, bar_width, color);
        let bytes_part = if known_total > 0 {
            format!(
                "{} / {}",
                format_bytes(total_downloaded),
                format_bytes(known_total)
            )
        } else {
            format_bytes(total_downloaded)
        };
        let eta_part = if all_totals_known && known_total > total_downloaded && agg_speed > 1024.0 {
            let secs = ((known_total - total_downloaded) as f64 / agg_speed).ceil() as u64;
            format!(" · ETA {}", format_eta(secs))
        } else {
            String::new()
        };
        let speed_part = if agg_speed >= 1.0 {
            format!(" · {}", format_speed(agg_speed))
        } else {
            String::new()
        };
        lines.push(format!(
            "  总进度  {bar} {:>3}%  ·  {bytes_part}{speed_part}{eta_part}  ·  {done_items}/{total_items}",
            pct
        ));
    }

    lines
}

fn render_bar(ratio: f64, width: usize, color: bool) -> String {
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    let empty = width.saturating_sub(filled);
    let filled_str = "█".repeat(filled);
    let empty_str = "░".repeat(empty);
    if color {
        format!("\x1b[38;2;255;130;184m{filled_str}\x1b[0m\x1b[2m{empty_str}\x1b[0m")
    } else {
        format!("{filled_str}{empty_str}")
    }
}

fn render_indeterminate_bar(frame: usize, width: usize, color: bool) -> String {
    let pos = frame % width.max(1);
    let mut s = String::new();
    for i in 0..width {
        if i == pos || i == (pos + 1) % width {
            s.push('▓');
        } else {
            s.push('░');
        }
    }
    if color {
        format!("\x1b[38;2;102;240;255m{s}\x1b[0m")
    } else {
        s
    }
}

fn paint(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GB", b / GIB)
    } else if b >= MIB {
        format!("{:.1} MB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_speed(bps: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    if bps >= MIB {
        format!("{:.1} MB/s", bps / MIB)
    } else if bps >= KIB {
        format!("{:.0} KB/s", bps / KIB)
    } else {
        format!("{:.0} B/s", bps)
    }
}

pub fn format_eta(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

fn truncate_ascii_or_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let mut out: String = s.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

fn strip_scheme(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
}

fn short_host(url: &str) -> String {
    let without_scheme = strip_scheme(url);
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

// ---------------------------------------------------------------------------
// Hashing helpers
// ---------------------------------------------------------------------------

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    encode_hex(&h.finalize())
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(encode_hex(&hasher.finalize()))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_platform::MockPlatform;

    #[test]
    fn parses_deps_tsv_metadata_and_rows() {
        let sha = "a".repeat(64);
        let tsv = format!(
            "# kind\tplatform\turl\tsha256\tdest\tmode\n\
             # font-dir\tmacos\t~/Library/Fonts\n\
             # font-dir\tlinux\t~/.local/share/fonts/kiseki\n\
             # font\tmaple-mono-nf-cn\t-(Regular|Medium|Bold|Italic)\\.ttf$\n\
             tool\tdarwin-arm64\thttps://example.test/omp\t{sha}\tvendor/bin/oh-my-posh\t0755\n\
             font-raw\tdarwin-arm64\thttps://example.test/jb.ttf\t{sha}\tfonts/JetBrainsMonoNerdFont-Regular.ttf\t0644\n\
             font\tdarwin-arm64\thttps://example.test/maple.zip\t{sha}\tfonts/maple-mono-nf-cn\t0644\n"
        );
        let parsed = parse_deps_tsv(&tsv).unwrap();
        assert_eq!(parsed.font_dirs.get("macos").map(String::as_str), Some("~/Library/Fonts"));
        assert_eq!(
            parsed.font_patterns.get("maple-mono-nf-cn").map(String::as_str),
            Some("-(Regular|Medium|Bold|Italic)\\.ttf$")
        );
        assert_eq!(parsed.rows.len(), 3);
        assert_eq!(catalog_id_for_row(&parsed.rows[0]), "oh-my-posh");
        assert_eq!(catalog_id_for_row(&parsed.rows[1]), "jetbrains-mono-nf");
        assert_eq!(catalog_id_for_row(&parsed.rows[2]), "maple-mono-nf-cn");
    }

    #[test]
    fn rejects_malformed_deps_tsv_rows() {
        let bad_cols = "tool\tdarwin-arm64\tonly-three\n";
        assert!(parse_deps_tsv(bad_cols).is_err());

        let bad_sha = "tool\tdarwin-arm64\thttps://x\tnot-a-sha\tvendor/bin/oh-my-posh\t0755\n";
        assert!(parse_deps_tsv(bad_sha).is_err());

        let bad_kind = format!(
            "weird\tdarwin-arm64\thttps://x\t{}\tvendor/bin/oh-my-posh\t0755\n",
            "0".repeat(64)
        );
        assert!(parse_deps_tsv(&bad_kind).is_err());
    }

    #[test]
    fn mirror_origin_ordering_and_loopback_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());

        let cn = build_mirror_origins(None, Some("cn"), &platform);
        assert_eq!(cn, vec![CN_ORIGIN, GLOBAL_ORIGIN]);

        let global = build_mirror_origins(None, Some("global"), &platform);
        assert_eq!(global, vec![GLOBAL_ORIGIN, CN_ORIGIN]);

        let custom = build_mirror_origins(
            Some("https://mirror.example.com/releases/"),
            Some("global"),
            &platform,
        );
        assert_eq!(
            custom,
            vec!["https://mirror.example.com", GLOBAL_ORIGIN, CN_ORIGIN]
        );

        let local = build_mirror_origins(
            Some("http://127.0.0.1:9001/,http://127.0.0.1:9002/releases"),
            Some("cn"),
            &platform,
        );
        assert_eq!(local, vec!["http://127.0.0.1:9001", "http://127.0.0.1:9002"]);
    }

    #[test]
    fn rejects_zip_slip_and_unsafe_font_names() {
        assert!(has_unsafe_zip_path("../evil.ttf"));
        assert!(has_unsafe_zip_path("foo/../../evil.ttf"));
        assert!(has_unsafe_zip_path("/etc/passwd"));
        assert!(has_unsafe_zip_path("font*.ttf"));
        assert!(has_unsafe_zip_path("C:evil.ttf"));
        assert!(!has_unsafe_zip_path("nested/MapleMono-NF-CN-Regular.ttf"));

        assert!(!is_safe_font_filename(".hidden.ttf"));
        assert!(!is_safe_font_filename("a..b.ttf"));
        assert!(!is_safe_font_filename("a/b.ttf"));
        assert!(is_safe_font_filename("MapleMono-NF-CN-Regular.ttf"));
    }

    #[test]
    fn validates_security_whitelist_on_rows() {
        let sha = "b".repeat(64);
        let evil_tool = DepRow {
            kind: DepKind::Tool,
            platform: "darwin-arm64".into(),
            url: "https://example.test/x".into(),
            sha256: sha.clone(),
            dest: "vendor/bin/rm".into(),
            mode: "0755".into(),
        };
        assert!(validate_row_security(&evil_tool).is_err());

        let evil_font = DepRow {
            kind: DepKind::FontRaw,
            platform: "darwin-arm64".into(),
            url: "https://example.test/x".into(),
            sha256: sha,
            dest: "fonts/../../etc/passwd.ttf".into(),
            mode: "0644".into(),
        };
        assert!(validate_row_security(&evil_font).is_err());
    }

    #[test]
    fn checksum_lookup_handles_binary_star_and_plain_format() {
        let h1 = "1".repeat(64);
        let h2 = "2".repeat(64);
        let body = format!("{h1}  kiseki-v2.0.0-beta.1-aarch64-apple-darwin.tar.gz\n{h2} *deps.tsv\n");
        assert_eq!(lookup_release_checksum(&body, "deps.tsv").as_deref(), Some(h2.as_str()));
        assert_eq!(lookup_release_checksum(&body, "missing.txt"), None);
    }

    #[test]
    fn progress_formatting_and_helpers_are_stable() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(159 * 1024 * 1024), "159.0 MB");
        assert_eq!(format_speed(2.5 * 1024.0 * 1024.0), "2.5 MB/s");
        assert_eq!(format_eta(42), "42s");
        assert_eq!(format_eta(125), "2m05s");

        let sha = "c".repeat(64);
        let jobs = vec![PlannedJob {
            label: "maple-mono-nf-cn".into(),
            id: "maple-mono-nf-cn".into(),
            row: DepRow {
                kind: DepKind::FontZip,
                platform: "darwin-arm64".into(),
                url: "https://example.test/m.zip".into(),
                sha256: sha,
                dest: "fonts/maple-mono-nf-cn".into(),
                mode: "0644".into(),
            },
            font_pattern: Some(r"\.ttf$".into()),
            up_to_date: false,
            existing_files: Vec::new(),
        }];
        let mut st = ProgressState::new(&jobs);
        st.items[0].phase = ItemPhase::Downloading;
        st.items[0].downloaded = 50 * 1024 * 1024;
        st.items[0].total = Some(100 * 1024 * 1024);
        st.items[0].speed_bps = 10.0 * 1024.0 * 1024.0;
        st.items[0].mirror = "kiseki-dl.nerv-base.com".into();

        let lines = format_progress_frame(&st, 0, 110, false, false);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("maple-mono-nf-cn"));
        assert!(lines[0].contains("50%"));
        assert!(lines[0].contains("10.0 MB/s"));
        assert!(lines[2].contains("ETA 5s"));
    }

    #[test]
    fn zip_font_receipt_requires_both_hash_and_non_empty_files() {
        let dir = tempfile::tempdir().unwrap();
        let font_dir = dir.path().join("fonts");
        let receipts_dir = dir.path().join("receipts");
        let job_dir = dir.path().join("job");
        fs::create_dir_all(&font_dir).unwrap();
        fs::create_dir_all(&job_dir).unwrap();

        let sha = "d".repeat(64);
        fs::write(font_dir.join("Face-Regular.ttf"), b"fake-ttf").unwrap();
        write_font_zip_receipt(
            &receipts_dir,
            "maple-mono-nf-cn",
            &sha,
            &["Face-Regular.ttf".to_string()],
            &job_dir,
        )
        .unwrap();

        let (ok, files) = check_font_zip_receipt(&receipts_dir, "maple-mono-nf-cn", &sha, &font_dir);
        assert!(ok);
        assert_eq!(files.len(), 1);

        // Hash change invalidates receipt.
        let (ok2, _) = check_font_zip_receipt(&receipts_dir, "maple-mono-nf-cn", &"e".repeat(64), &font_dir);
        assert!(!ok2);

        // Missing font file invalidates receipt (auto-heals on rerun!).
        fs::remove_file(font_dir.join("Face-Regular.ttf")).unwrap();
        let (ok3, _) = check_font_zip_receipt(&receipts_dir, "maple-mono-nf-cn", &sha, &font_dir);
        assert!(!ok3);
    }
}
