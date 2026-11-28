//! Synchronous HTTP/file gallery client, ETag/TTL cache, safe install, and
//! theme update orchestration (RFC-0006 §6).
//!
//! Uses pure synchronous `ureq` + `rustls` (zero `tokio`) and is ONLY invoked
//! by explicit gallery / remote-install / update commands or the TUI Gallery
//! tab. Routine theme switching remains 100% offline.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kiseki_pkg::{install_from_dir, validate_theme_dir, InstalledTheme, MAX_BYTES};
use kiseki_platform::Platform;
use kiseki_state::GalleryConfig;
use kiseki_types::{KisekiError, Result};
use serde::{Deserialize, Serialize};

use crate::archive::{extract_tar_gz_safely, sha256_hex};
use crate::model::{
    normalize_subpath, parse_install_target, GalleryIndex, GalleryTheme, GalleryUpdates,
    InstallTarget, OriginRegistry, ThemeOrigin, DEFAULT_GALLERY_SOURCES,
};

/// Maximum allowed size of a single `gallery/index.json` or `updates.json` (8 MiB).
pub const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;
/// Default HTTP timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 20;

/// Metadata stored alongside a cached gallery index.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedIndexMeta {
    source: String,
    fetched_at_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    etag: Option<String>,
}

/// Result of loading the gallery index.
#[derive(Debug, Clone)]
pub struct LoadedIndex {
    pub index: GalleryIndex,
    pub source: String,
    pub from_cache: bool,
    pub stale: bool,
    pub warnings: Vec<String>,
}

/// Result of installing a theme from any target (local dir/archive, gallery ID, or GitHub).
#[derive(Debug, Clone)]
pub struct GalleryInstallReport {
    pub dry_run: bool,
    pub valid: bool,
    pub id: String,
    pub source_kind: String,
    pub path: Option<PathBuf>,
    pub file_count: usize,
    pub total_bytes: u64,
    pub origin: Option<ThemeOrigin>,
}

/// Outcome for one community theme checked by `kiseki theme update`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOutcome {
    pub id: String,
    /// `"updated"` | `"available"` | `"up-to-date"` | `"skipped"`
    pub status: String,
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub note: Option<String>,
}

/// Synchronous Kiseki Gallery client.
#[derive(Debug, Clone)]
pub struct GalleryClient {
    sources: Vec<String>,
    github_proxy: Option<String>,
    cache_dir: PathBuf,
    cache_ttl_secs: u64,
    timeout_secs: u64,
}

impl GalleryClient {
    /// Builds a client from `config.toml` `[gallery]` settings and Kiseki cache dir.
    pub fn from_config(config: &GalleryConfig, cache_dir: PathBuf) -> Self {
        let sources = if config.sources.is_empty() {
            DEFAULT_GALLERY_SOURCES
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        } else {
            config.sources.clone()
        };
        Self {
            sources,
            github_proxy: config
                .github_proxy
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            cache_dir,
            cache_ttl_secs: config.cache_ttl_secs,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs.max(1);
        self
    }

    /// Loads the gallery index from cache or configured sources.
    ///
    /// When all configured remote sources fail and a stale disk cache exists,
    /// returns the stale cache with `stale = true` and explanatory warnings so
    /// offline browsing keeps working cleanly.
    pub fn load_index(&self, force_refresh: bool) -> Result<LoadedIndex> {
        let mut warnings = Vec::new();
        let mut combined: Option<GalleryIndex> = None;
        let mut primary_source = String::new();
        let mut any_from_cache = false;
        let mut any_stale = false;

        // If the user configured custom sources, merge all reachable sources;
        // if using the built-in mirror list, stop at the first healthy mirror.
        let is_default_mirrors = self.sources.len() == DEFAULT_GALLERY_SOURCES.len()
            && self
                .sources
                .iter()
                .zip(DEFAULT_GALLERY_SOURCES.iter())
                .all(|(a, b)| a == b);

        let mut errors = Vec::new();
        for source in &self.sources {
            match self.load_single_source(source, force_refresh) {
                Ok(loaded) => {
                    if primary_source.is_empty() {
                        primary_source = loaded.source.clone();
                    }
                    any_from_cache |= loaded.from_cache;
                    any_stale |= loaded.stale;
                    warnings.extend(loaded.warnings);
                    match combined.as_mut() {
                        Some(existing) => existing.merge(loaded.index),
                        None => combined = Some(loaded.index),
                    }
                    if is_default_mirrors && !loaded.stale {
                        break;
                    }
                }
                Err(error) => {
                    errors.push(format!("{source}: {error}"));
                }
            }
        }

        if let Some(index) = combined {
            if !errors.is_empty() && !is_default_mirrors {
                warnings.push(format!(
                    "some gallery sources were unreachable: {}",
                    errors.join("; ")
                ));
            }
            return Ok(LoadedIndex {
                index,
                source: primary_source,
                from_cache: any_from_cache,
                stale: any_stale,
                warnings,
            });
        }

        Err(KisekiError::other(format!(
            "failed to load gallery index from all {} source(s): {}",
            self.sources.len(),
            errors.join("; ")
        )))
    }

    /// Lazily fetches `updates.json` next to the primary index source (best-effort).
    pub fn load_updates(&self) -> Result<Option<GalleryUpdates>> {
        for source in &self.sources {
            let updates_url = derive_sibling_url(source, "updates.json");
            if let Ok(FetchOutcome::Body { bytes, .. }) =
                self.fetch_bounded(&updates_url, MAX_INDEX_BYTES, None)
            {
                if let Ok(parsed) = serde_json::from_slice::<GalleryUpdates>(&bytes) {
                    return Ok(Some(parsed));
                }
            }
        }
        Ok(None)
    }

    fn load_single_source(&self, source: &str, force_refresh: bool) -> Result<LoadedIndex> {
        let key = &sha256_hex(source.as_bytes())[..16];
        let body_path = self.cache_dir.join(format!("index-{key}.json"));
        let meta_path = self.cache_dir.join(format!("index-{key}.meta.json"));

        let cached_meta = std::fs::read_to_string(&meta_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<CachedIndexMeta>(&raw).ok());
        let cached_index = std::fs::read(&body_path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<GalleryIndex>(&raw).ok());

        let now = unix_now();
        if !force_refresh {
            if let (Some(meta), Some(index)) = (&cached_meta, &cached_index) {
                if now.saturating_sub(meta.fetched_at_unix) <= self.cache_ttl_secs {
                    return Ok(LoadedIndex {
                        index: index.clone(),
                        source: source.to_string(),
                        from_cache: true,
                        stale: false,
                        warnings: Vec::new(),
                    });
                }
            }
        }

        let etag = cached_meta.as_ref().and_then(|m| m.etag.as_deref());
        match self.fetch_bounded(source, MAX_INDEX_BYTES, etag) {
            Ok(FetchOutcome::NotModified) => {
                if let Some(index) = cached_index {
                    let _ = self.write_cache(
                        &body_path,
                        &meta_path,
                        None,
                        &CachedIndexMeta {
                            source: source.to_string(),
                            fetched_at_unix: now,
                            etag: etag.map(str::to_string),
                        },
                    );
                    return Ok(LoadedIndex {
                        index,
                        source: source.to_string(),
                        from_cache: true,
                        stale: false,
                        warnings: Vec::new(),
                    });
                }
                // Cache body disappeared; re-fetch without ETag.
                let FetchOutcome::Body { bytes, etag } =
                    self.fetch_bounded(source, MAX_INDEX_BYTES, None)?
                else {
                    return Err(KisekiError::other("unexpected 304 without If-None-Match"));
                };
                let index: GalleryIndex = serde_json::from_slice(&bytes)
                    .map_err(|e| KisekiError::Json(format!("invalid gallery index: {e}")))?;
                let _ = self.write_cache(
                    &body_path,
                    &meta_path,
                    Some(&bytes),
                    &CachedIndexMeta {
                        source: source.to_string(),
                        fetched_at_unix: now,
                        etag,
                    },
                );
                Ok(LoadedIndex {
                    index,
                    source: source.to_string(),
                    from_cache: false,
                    stale: false,
                    warnings: Vec::new(),
                })
            }
            Ok(FetchOutcome::Body { bytes, etag }) => {
                let index: GalleryIndex = serde_json::from_slice(&bytes)
                    .map_err(|e| KisekiError::Json(format!("invalid gallery index: {e}")))?;
                let _ = self.write_cache(
                    &body_path,
                    &meta_path,
                    Some(&bytes),
                    &CachedIndexMeta {
                        source: source.to_string(),
                        fetched_at_unix: now,
                        etag,
                    },
                );
                Ok(LoadedIndex {
                    index,
                    source: source.to_string(),
                    from_cache: false,
                    stale: false,
                    warnings: Vec::new(),
                })
            }
            Err(error) => {
                if let Some(index) = cached_index {
                    return Ok(LoadedIndex {
                        index,
                        source: source.to_string(),
                        from_cache: true,
                        stale: true,
                        warnings: vec![format!(
                            "could not refresh gallery from {source} ({error}); using cached index"
                        )],
                    });
                }
                Err(error)
            }
        }
    }

    fn write_cache(
        &self,
        body_path: &Path,
        meta_path: &Path,
        body: Option<&[u8]>,
        meta: &CachedIndexMeta,
    ) -> Result<()> {
        std::fs::create_dir_all(&self.cache_dir)
            .map_err(|e| KisekiError::io(&self.cache_dir, e))?;
        if let Some(bytes) = body {
            std::fs::write(body_path, bytes).map_err(|e| KisekiError::io(body_path, e))?;
        }
        let encoded = serde_json::to_string_pretty(meta)
            .map_err(|e| KisekiError::Json(e.to_string()))?;
        std::fs::write(meta_path, encoded.as_bytes()).map_err(|e| KisekiError::io(meta_path, e))?;
        Ok(())
    }

    /// Unified install handler: local directory, local `.tar.gz`, Gallery short ID,
    /// or `github:owner/repo[@ref][#subpath]`.
    pub fn install_target(
        &self,
        platform: &dyn Platform,
        raw: &str,
        force: bool,
        force_local: bool,
        force_gallery: bool,
        dry_run: bool,
    ) -> Result<GalleryInstallReport> {
        match parse_install_target(raw, force_local, force_gallery)? {
            InstallTarget::LocalDir(dir) => {
                if dry_run {
                    let validation = validate_theme_dir(&dir)?;
                    let id = validation
                        .pack
                        .as_ref()
                        .map(|p| p.manifest.id.as_str().to_string())
                        .unwrap_or_else(|| dir.display().to_string());
                    return Ok(GalleryInstallReport {
                        dry_run: true,
                        valid: validation.is_valid(),
                        id,
                        source_kind: "local".to_string(),
                        path: Some(dir),
                        file_count: validation.file_count,
                        total_bytes: validation.total_bytes,
                        origin: None,
                    });
                }
                let installed = install_from_dir(platform, &dir, force)?;
                let origin = ThemeOrigin {
                    id: installed.id.clone(),
                    source: "local".to_string(),
                    repo: None,
                    subpath: None,
                    git_ref: None,
                    version: None,
                    commit: None,
                    sha256: None,
                    installed_at: platform.now_rfc3339(),
                };
                OriginRegistry::record(platform, platform.paths(), origin.clone())?;
                Ok(report_from_installed(installed, "local", Some(origin)))
            }
            InstallTarget::LocalArchive(archive_path) => {
                let bytes = read_file_bounded(&archive_path, MAX_BYTES)?;
                let temp = tempfile::tempdir()
                    .map_err(|e| KisekiError::io(std::env::temp_dir(), e))?;
                let stats = extract_tar_gz_safely(&bytes, None, None, None, temp.path())?;
                self.finish_extracted_install(
                    platform,
                    temp.path(),
                    "archive",
                    force,
                    dry_run,
                    |id, version| ThemeOrigin {
                        id,
                        source: "archive".to_string(),
                        repo: None,
                        subpath: None,
                        git_ref: None,
                        version,
                        commit: None,
                        sha256: Some(stats.sha256.clone()),
                        installed_at: platform.now_rfc3339(),
                    },
                )
            }
            InstallTarget::GalleryId(id) => {
                let loaded = self.load_index(false)?;
                let entry = loaded.index.find(&id).ok_or_else(|| {
                    KisekiError::ThemeNotFound(format!(
                        "{id} (not found in gallery index; run `kiseki gallery refresh` or pass a local path)"
                    ))
                })?;
                self.install_gallery_entry(platform, entry, force, dry_run)
            }
            InstallTarget::GitHub {
                owner,
                repo,
                git_ref,
                subpath,
            } => {
                let ref_seg = git_ref.as_deref().unwrap_or("HEAD");
                let canonical_url =
                    format!("https://codeload.github.com/{owner}/{repo}/tar.gz/{ref_seg}");
                let url = self.apply_github_proxy(&canonical_url);
                let FetchOutcome::Body { bytes, .. } =
                    self.fetch_bounded(&url, MAX_BYTES, None)?
                else {
                    return Err(KisekiError::other("unexpected empty archive response"));
                };
                let temp = tempfile::tempdir()
                    .map_err(|e| KisekiError::io(std::env::temp_dir(), e))?;
                let stats = extract_tar_gz_safely(
                    &bytes,
                    None,
                    None,
                    subpath.as_deref(),
                    temp.path(),
                )?;
                let repo_slug = format!("{owner}/{repo}");
                self.finish_extracted_install(
                    platform,
                    temp.path(),
                    "github",
                    force,
                    dry_run,
                    |id, version| ThemeOrigin {
                        id,
                        source: "github".to_string(),
                        repo: Some(repo_slug.clone()),
                        subpath: subpath.clone(),
                        git_ref: git_ref.clone(),
                        version,
                        commit: None,
                        sha256: Some(stats.sha256.clone()),
                        installed_at: platform.now_rfc3339(),
                    },
                )
            }
        }
    }

    /// Downloads, verifies, validates, and installs a resolved `GalleryTheme`.
    pub fn install_gallery_entry(
        &self,
        platform: &dyn Platform,
        entry: &GalleryTheme,
        force: bool,
        dry_run: bool,
    ) -> Result<GalleryInstallReport> {
        let bytes = self.download_gallery_archive(entry)?;
        let subpath = normalize_subpath(&entry.subpath)?;
        let temp =
            tempfile::tempdir().map_err(|e| KisekiError::io(std::env::temp_dir(), e))?;
        let stats = extract_tar_gz_safely(
            &bytes,
            Some(&entry.download.sha256),
            entry.download.strip_prefix.as_deref(),
            subpath.as_deref(),
            temp.path(),
        )?;

        self.finish_extracted_install(
            platform,
            temp.path(),
            "gallery",
            force,
            dry_run,
            |id, version| ThemeOrigin {
                id,
                source: "gallery".to_string(),
                repo: Some(entry.repo.clone()),
                subpath: subpath.clone(),
                git_ref: None,
                version: if entry.version.is_empty() {
                    version
                } else {
                    Some(entry.version.clone())
                },
                commit: (!entry.commit.is_empty()).then(|| entry.commit.clone()),
                sha256: Some(stats.sha256.clone()),
                installed_at: platform.now_rfc3339(),
            },
        )
    }

    /// Checks installed community themes against the fresh gallery index and
    /// upgrades outdated ones.
    pub fn update_installed(
        &self,
        platform: &dyn Platform,
        target_id: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<UpdateOutcome>> {
        let origins = OriginRegistry::load(platform, platform.paths())?;
        let candidates: Vec<&ThemeOrigin> = match target_id {
            Some(id) => match origins.themes.get(id) {
                Some(origin) => vec![origin],
                None => {
                    return Err(KisekiError::ThemeNotFound(format!(
                        "{id} is not tracked as an installed gallery/github theme"
                    )))
                }
            },
            None => origins.themes.values().collect(),
        };

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let loaded = self.load_index(true)?;
        let mut outcomes = Vec::with_capacity(candidates.len());

        for origin in candidates {
            if origin.source != "gallery" {
                outcomes.push(UpdateOutcome {
                    id: origin.id.clone(),
                    status: "skipped".to_string(),
                    from_version: origin.version.clone(),
                    to_version: origin.version.clone(),
                    note: Some(format!(
                        "installed from `{}` (not managed by gallery index)",
                        origin.source
                    )),
                });
                continue;
            }
            let Some(entry) = loaded.index.find(&origin.id) else {
                outcomes.push(UpdateOutcome {
                    id: origin.id.clone(),
                    status: "skipped".to_string(),
                    from_version: origin.version.clone(),
                    to_version: None,
                    note: Some("not present in current gallery index".to_string()),
                });
                continue;
            };

            let sha_matches = origin
                .sha256
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(&entry.download.sha256));
            let commit_matches = !entry.commit.is_empty()
                && origin.commit.as_deref() == Some(entry.commit.as_str());
            if sha_matches || commit_matches {
                outcomes.push(UpdateOutcome {
                    id: origin.id.clone(),
                    status: "up-to-date".to_string(),
                    from_version: origin.version.clone(),
                    to_version: Some(entry.version.clone()),
                    note: None,
                });
                continue;
            }

            if dry_run {
                outcomes.push(UpdateOutcome {
                    id: origin.id.clone(),
                    status: "available".to_string(),
                    from_version: origin.version.clone(),
                    to_version: Some(entry.version.clone()),
                    note: Some(format!(
                        "commit {}",
                        entry.commit.chars().take(8).collect::<String>()
                    )),
                });
                continue;
            }

            self.install_gallery_entry(platform, entry, true, false)?;
            outcomes.push(UpdateOutcome {
                id: origin.id.clone(),
                status: "updated".to_string(),
                from_version: origin.version.clone(),
                to_version: Some(entry.version.clone()),
                note: None,
            });
        }

        Ok(outcomes)
    }

    fn finish_extracted_install<F>(
        &self,
        platform: &dyn Platform,
        extracted_dir: &Path,
        source_kind: &str,
        force: bool,
        dry_run: bool,
        make_origin: F,
    ) -> Result<GalleryInstallReport>
    where
        F: FnOnce(String, Option<String>) -> ThemeOrigin,
    {
        // Always run local full validation (defense in depth — RFC-0006 §6.3).
        let validation = validate_theme_dir(extracted_dir)?;
        let id = validation
            .pack
            .as_ref()
            .map(|p| p.manifest.id.as_str().to_string())
            .unwrap_or_else(|| extracted_dir.display().to_string());
        let version = validation
            .pack
            .as_ref()
            .map(|p| p.manifest.version.clone());

        if dry_run {
            return Ok(GalleryInstallReport {
                dry_run: true,
                valid: validation.is_valid(),
                id,
                source_kind: source_kind.to_string(),
                path: None,
                file_count: validation.file_count,
                total_bytes: validation.total_bytes,
                origin: None,
            });
        }

        if !validation.is_valid() {
            let first = validation
                .diagnostics
                .iter()
                .find(|d| d.is_fatal())
                .map(|d| format!("{}: {}", d.code, d.message))
                .unwrap_or_else(|| "theme pack has blocking validation errors".to_string());
            return Err(KisekiError::schema(first));
        }

        let installed = install_from_dir(platform, extracted_dir, force)?;
        let origin = make_origin(installed.id.clone(), version);
        OriginRegistry::record(platform, platform.paths(), origin.clone())?;
        Ok(report_from_installed(installed, source_kind, Some(origin)))
    }

    fn download_gallery_archive(&self, entry: &GalleryTheme) -> Result<Vec<u8>> {
        let mut urls = Vec::new();
        if let Some(track) = entry
            .download
            .track_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            urls.push(track.to_string());
        }
        let direct = self.apply_github_proxy(&entry.download.url);
        if !urls.contains(&direct) {
            urls.push(direct);
        }
        if !urls.contains(&entry.download.url) {
            urls.push(entry.download.url.clone());
        }

        let mut errors = Vec::new();
        for url in urls {
            match self.fetch_bounded(&url, MAX_BYTES, None) {
                Ok(FetchOutcome::Body { bytes, .. }) => return Ok(bytes),
                Ok(FetchOutcome::NotModified) => {
                    errors.push(format!("{url}: unexpected 304"));
                }
                Err(error) => {
                    errors.push(format!("{url}: {error}"));
                }
            }
        }
        Err(KisekiError::other(format!(
            "failed to download theme `{}`: {}",
            entry.id,
            errors.join("; ")
        )))
    }

    fn apply_github_proxy(&self, url: &str) -> String {
        let Some(proxy) = self.github_proxy.as_deref() else {
            return url.to_string();
        };
        if url.starts_with("https://github.com/") || url.starts_with("https://codeload.github.com/")
        {
            format!("{}/{}", proxy.trim_end_matches('/'), url)
        } else {
            url.to_string()
        }
    }

    fn fetch_bounded(
        &self,
        location: &str,
        max_bytes: u64,
        etag: Option<&str>,
    ) -> Result<FetchOutcome> {
        if let Some(path) = location.strip_prefix("file://") {
            let bytes = read_file_bounded(Path::new(path), max_bytes)?;
            return Ok(FetchOutcome::Body { bytes, etag: None });
        }
        if !location.starts_with("http://") && !location.starts_with("https://") {
            let bytes = read_file_bounded(Path::new(location), max_bytes)?;
            return Ok(FetchOutcome::Body { bytes, etag: None });
        }

        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(self.timeout_secs)))
            .http_status_as_error(false)
            .build();
        let agent: ureq::Agent = config.into();

        let mut req = agent
            .get(location)
            .header("User-Agent", concat!("kiseki-cli/", env!("CARGO_PKG_VERSION")));
        if let Some(tag) = etag {
            req = req.header("If-None-Match", tag);
        }

        let response = req
            .call()
            .map_err(|error| KisekiError::other(format!("HTTP request failed: {error}")))?;
        let status = response.status().as_u16();
        if status == 304 {
            return Ok(FetchOutcome::NotModified);
        }
        if !(200..300).contains(&status) {
            return Err(KisekiError::other(format!("HTTP {status} for {location}")));
        }

        let resp_etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let mut reader = response.into_body().into_reader();
        let mut bytes = Vec::new();
        reader
            .by_ref()
            .take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| KisekiError::other(format!("failed reading response body: {error}")))?;
        if bytes.len() as u64 > max_bytes {
            return Err(KisekiError::other(format!(
                "response from {location} exceeded maximum allowed size ({max_bytes} bytes)"
            )));
        }

        Ok(FetchOutcome::Body {
            bytes,
            etag: resp_etag,
        })
    }
}

enum FetchOutcome {
    NotModified,
    Body { bytes: Vec<u8>, etag: Option<String> },
}

fn read_file_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|e| KisekiError::io(path, e))?;
    let mut buf = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut buf)
        .map_err(|e| KisekiError::io(path, e))?;
    if buf.len() as u64 > max_bytes {
        return Err(KisekiError::other(format!(
            "{} exceeds maximum allowed size ({max_bytes} bytes)",
            path.display()
        )));
    }
    Ok(buf)
}

fn derive_sibling_url(index_url: &str, sibling: &str) -> String {
    match index_url.rsplit_once('/') {
        Some((prefix, _)) => format!("{prefix}/{sibling}"),
        None => sibling.to_string(),
    }
}

fn report_from_installed(
    installed: InstalledTheme,
    source_kind: &str,
    origin: Option<ThemeOrigin>,
) -> GalleryInstallReport {
    GalleryInstallReport {
        dry_run: false,
        valid: true,
        id: installed.id,
        source_kind: source_kind.to_string(),
        path: Some(installed.path),
        file_count: installed.file_count,
        total_bytes: installed.total_bytes,
        origin,
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
