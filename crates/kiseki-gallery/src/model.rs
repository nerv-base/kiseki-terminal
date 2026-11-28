//! Data models, sorting/filtering, target parsing, and origin tracking for
//! Kiseki Gallery (RFC-0006).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use kiseki_platform::{KisekiPaths, Platform};
use kiseki_types::{Appearance, KisekiError, Result, ThemeId};
use serde::{Deserialize, Serialize};

/// Schema version of `gallery/index.json` and `gallery-origins.json`.
pub const GALLERY_SCHEMA_VERSION: u32 = 1;

/// Built-in official gallery index sources (tried in order).
pub const DEFAULT_GALLERY_SOURCES: &[&str] = &[
    "https://dl.kisekiterm.dev/gallery/index.json",
    "https://kiseki-dl.nerv-base.com/gallery/index.json",
    "https://nerv-base.github.io/kiseki-themes/index.json",
];

/// Published static gallery index (`gallery/index.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryIndex {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub themes: Vec<GalleryTheme>,
}

fn default_schema_version() -> u32 {
    GALLERY_SCHEMA_VERSION
}

impl GalleryIndex {
    /// Finds a theme entry by its flat short ID.
    pub fn find(&self, id: &str) -> Option<&GalleryTheme> {
        self.themes.iter().find(|entry| entry.id == id)
    }

    /// Merges another index into `self` (first source wins on duplicate IDs).
    pub fn merge(&mut self, other: GalleryIndex) {
        for theme in other.themes {
            if !self.themes.iter().any(|existing| existing.id == theme.id) {
                self.themes.push(theme);
            }
        }
        if self.generated_at.is_empty() {
            self.generated_at = other.generated_at;
        }
    }
}

/// One community theme listed in `gallery/index.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryTheme {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub authors: Vec<GalleryAuthor>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default = "default_subpath")]
    pub subpath: String,
    #[serde(default)]
    pub commit: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub capabilities: GalleryCapabilities,
    #[serde(default)]
    pub metrics: GalleryMetrics,
    pub download: GalleryDownload,
    #[serde(default)]
    pub preview: GalleryPreview,
}

fn default_subpath() -> String {
    ".".to_string()
}

/// Theme author metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryAuthor {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Objective capabilities auto-disclosed by `kiseki theme validate --json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryCapabilities {
    #[serde(default)]
    pub has_wallpaper: bool,
    #[serde(default)]
    pub has_shader: bool,
    #[serde(default)]
    pub shader_profiles: Vec<String>,
    #[serde(default)]
    pub wcag_grade: String,
}

/// Objective ranking metrics (no manual `verified` badge).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryMetrics {
    #[serde(default)]
    pub stars: u64,
    #[serde(default)]
    pub stars7d: u64,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub downloads30d: u64,
    #[serde(default)]
    pub trending_score: f64,
}

/// Immutable archive download coordinates pinned by SHA-256.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryDownload {
    /// Optional anonymous-counting gateway URL (Cloudflare Worker 302 or R2 mirror).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_url: Option<String>,
    /// Canonical immutable archive URL (e.g. `codeload.github.com/.../tar.gz/<sha>`).
    pub url: String,
    /// Expected lowercase hex SHA-256 digest of the `.tar.gz` archive.
    pub sha256: String,
    /// Archive size in bytes.
    #[serde(default)]
    pub bytes: u64,
    /// Optional archive root directory prefix to strip before `subpath`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_prefix: Option<String>,
}

/// Storefront visual preview metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryPreview {
    /// R2-hosted WebP screenshot thumbnail URL (<= 960px), when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_url: Option<String>,
    /// Compact token palette used for live fallback terminal rendering & TUI swatches.
    #[serde(default)]
    pub palette: GalleryPalettePreview,
}

/// Compact color swatch embedded directly in `index.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryPalettePreview {
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub surface0: String,
    #[serde(default)]
    pub text_primary: String,
    #[serde(default)]
    pub accent_primary: String,
    #[serde(default)]
    pub accent_secondary: String,
    #[serde(default)]
    pub keyword: String,
    #[serde(default)]
    pub string: String,
    #[serde(default)]
    pub function: String,
}

/// Optional lazy-loaded release notes (`gallery/updates.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryUpdates {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub themes: BTreeMap<String, GalleryThemeUpdateNotes>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryThemeUpdateNotes {
    #[serde(default)]
    pub latest_tag: Option<String>,
    #[serde(default)]
    pub release_notes: Option<String>,
    #[serde(default)]
    pub recent_commits: Vec<String>,
}

/// Gallery sort order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GallerySort {
    #[default]
    Trending,
    Downloads,
    Stars,
    Updated,
    Newest,
}

impl GallerySort {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trending => "trending",
            Self::Downloads => "downloads",
            Self::Stars => "stars",
            Self::Updated => "updated",
            Self::Newest => "newest",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Trending => Self::Downloads,
            Self::Downloads => Self::Stars,
            Self::Stars => Self::Updated,
            Self::Updated => Self::Newest,
            Self::Newest => Self::Trending,
        }
    }
}

impl FromStr for GallerySort {
    type Err = KisekiError;

    fn from_str(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "trending" | "hot" => Ok(Self::Trending),
            "downloads" | "download" | "installs" => Ok(Self::Downloads),
            "stars" | "star" => Ok(Self::Stars),
            "updated" | "recent" => Ok(Self::Updated),
            "newest" | "new" => Ok(Self::Newest),
            other => Err(KisekiError::other(format!(
                "invalid gallery sort {other:?}; expected trending|downloads|stars|updated|newest"
            ))),
        }
    }
}

/// Filter + sort criteria for searching the gallery index.
#[derive(Debug, Clone, Default)]
pub struct GalleryFilter {
    pub query: Option<String>,
    pub appearance: Option<Appearance>,
    pub has_shader: bool,
    pub has_wallpaper: bool,
    pub tag: Option<String>,
    pub sort: GallerySort,
}

/// Computes the RFC-0006 multi-dimensional trending score.
pub fn compute_trending_score(
    downloads_30d: u64,
    stars_7d: u64,
    stars: u64,
    recency_0_to_1: f64,
) -> f64 {
    let recency = recency_0_to_1.clamp(0.0, 1.0);
    0.50 * ((1 + downloads_30d) as f64).ln()
        + 0.30 * ((1 + stars_7d.saturating_mul(4)) as f64).ln()
        + 0.15 * ((1 + stars) as f64).ln()
        + 0.05 * recency
}

/// Filters and sorts gallery entries deterministically.
pub fn filter_and_sort<'a>(
    themes: &'a [GalleryTheme],
    filter: &GalleryFilter,
) -> Vec<&'a GalleryTheme> {
    let needle = filter
        .query
        .as_deref()
        .map(|q| q.trim().to_ascii_lowercase())
        .filter(|q| !q.is_empty());
    let tag_needle = filter
        .tag
        .as_deref()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty());

    let mut matched: Vec<&'a GalleryTheme> = themes
        .iter()
        .filter(|theme| {
            if let Some(app) = filter.appearance {
                if theme.appearance != app {
                    return false;
                }
            }
            if filter.has_shader && !theme.capabilities.has_shader {
                return false;
            }
            if filter.has_wallpaper && !theme.capabilities.has_wallpaper {
                return false;
            }
            if let Some(tag) = tag_needle.as_deref() {
                if !theme.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                    return false;
                }
            }
            if let Some(q) = needle.as_deref() {
                let in_id = theme.id.to_ascii_lowercase().contains(q);
                let in_name = theme.name.to_ascii_lowercase().contains(q);
                let in_sub = theme.subtitle.to_ascii_lowercase().contains(q);
                let in_repo = theme.repo.to_ascii_lowercase().contains(q);
                let in_tags = theme
                    .tags
                    .iter()
                    .any(|t| t.to_ascii_lowercase().contains(q));
                let in_authors = theme
                    .authors
                    .iter()
                    .any(|a| a.name.to_ascii_lowercase().contains(q));
                if !(in_id || in_name || in_sub || in_repo || in_tags || in_authors) {
                    return false;
                }
            }
            true
        })
        .collect();

    matched.sort_by(|a, b| {
        let ord = match filter.sort {
            GallerySort::Trending => b
                .metrics
                .trending_score
                .partial_cmp(&a.metrics.trending_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.metrics.downloads30d.cmp(&a.metrics.downloads30d))
                .then_with(|| b.metrics.stars.cmp(&a.metrics.stars)),
            GallerySort::Downloads => b
                .metrics
                .downloads
                .cmp(&a.metrics.downloads)
                .then_with(|| b.metrics.downloads30d.cmp(&a.metrics.downloads30d)),
            GallerySort::Stars => b
                .metrics
                .stars
                .cmp(&a.metrics.stars)
                .then_with(|| b.metrics.stars7d.cmp(&a.metrics.stars7d)),
            GallerySort::Updated => b.updated_at.cmp(&a.updated_at),
            GallerySort::Newest => b.created_at.cmp(&a.created_at),
        };
        ord.then_with(|| a.id.cmp(&b.id))
    });

    matched
}

// ---------------------------------------------------------------------------
// Install target resolution
// ---------------------------------------------------------------------------

/// Resolved destination of `kiseki theme install <target>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallTarget {
    LocalDir(PathBuf),
    LocalArchive(PathBuf),
    GitHub {
        owner: String,
        repo: String,
        git_ref: Option<String>,
        subpath: Option<String>,
    },
    GalleryId(String),
}

/// Classifies a CLI `theme install <target>` argument.
pub fn parse_install_target(
    raw: &str,
    force_local: bool,
    force_gallery: bool,
) -> Result<InstallTarget> {
    if force_local && force_gallery {
        return Err(KisekiError::other(
            "--local and --gallery are mutually exclusive",
        ));
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(KisekiError::other("install target must not be empty"));
    }

    if force_gallery {
        let id = ThemeId::new(trimmed)?;
        return Ok(InstallTarget::GalleryId(id.into_string()));
    }

    if !force_local {
        if let Some(rest) = trimmed.strip_prefix("github:") {
            return parse_github_spec(rest);
        }
        if let Some(rest) = trimmed
            .strip_prefix("https://github.com/")
            .or_else(|| trimmed.strip_prefix("http://github.com/"))
        {
            return parse_github_url(rest);
        }
    }

    let path = PathBuf::from(trimmed);
    let looks_like_archive = is_archive_path(&path);
    let looks_like_path = force_local
        || path.exists()
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || looks_like_archive;

    if looks_like_path {
        if looks_like_archive || path.is_file() {
            return Ok(InstallTarget::LocalArchive(path));
        }
        return Ok(InstallTarget::LocalDir(path));
    }

    if let Ok(id) = ThemeId::new(trimmed) {
        return Ok(InstallTarget::GalleryId(id.into_string()));
    }

    Ok(InstallTarget::LocalDir(path))
}

fn is_archive_path(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    lower.ends_with(".tar.gz") || lower.ends_with(".tgz")
}

fn parse_github_spec(rest: &str) -> Result<InstallTarget> {
    let (before_hash, subpath) = match rest.split_once('#') {
        Some((head, sub)) => (head, normalize_subpath(sub)?),
        None => (rest, None),
    };
    let (repo_part, git_ref) = match before_hash.split_once('@') {
        Some((head, r)) => (head, Some(validate_git_ref(r)?)),
        None => (before_hash, None),
    };
    let (owner, repo) = split_owner_repo(repo_part)?;
    Ok(InstallTarget::GitHub {
        owner,
        repo,
        git_ref,
        subpath,
    })
}

fn parse_github_url(rest: &str) -> Result<InstallTarget> {
    let rest = rest.trim_end_matches('/');
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 2 {
        return Err(KisekiError::other(format!(
            "invalid GitHub URL `https://github.com/{rest}`: expected owner/repo"
        )));
    }
    let owner = validate_github_ident(parts[0], "owner")?;
    let repo = validate_github_ident(parts[1].trim_end_matches(".git"), "repo")?;
    let (git_ref, subpath) = if parts.len() >= 4 && parts[2] == "tree" {
        let git_ref = Some(validate_git_ref(parts[3])?);
        let subpath = if parts.len() > 4 {
            normalize_subpath(&parts[4..].join("/"))?
        } else {
            None
        };
        (git_ref, subpath)
    } else {
        (None, None)
    };
    Ok(InstallTarget::GitHub {
        owner,
        repo,
        git_ref,
        subpath,
    })
}

fn split_owner_repo(spec: &str) -> Result<(String, String)> {
    let mut parts = spec.split('/');
    let owner = parts
        .next()
        .ok_or_else(|| KisekiError::other("expected github:owner/repo"))?;
    let repo = parts
        .next()
        .ok_or_else(|| KisekiError::other("expected github:owner/repo"))?;
    if parts.next().is_some() {
        return Err(KisekiError::other(format!(
            "invalid GitHub repo {spec:?}; use github:owner/repo[@ref][#subpath]"
        )));
    }
    Ok((
        validate_github_ident(owner, "owner")?,
        validate_github_ident(repo.trim_end_matches(".git"), "repo")?,
    ))
}

fn validate_github_ident(raw: &str, label: &str) -> Result<String> {
    if raw.is_empty()
        || raw.len() > 100
        || raw == "."
        || raw == ".."
        || !raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(KisekiError::unsafe_path(
            raw,
            format!("invalid GitHub {label}"),
        ));
    }
    Ok(raw.to_string())
}

fn validate_git_ref(raw: &str) -> Result<String> {
    if raw.is_empty()
        || raw.len() > 128
        || raw.contains("..")
        || !raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'))
    {
        return Err(KisekiError::unsafe_path(raw, "invalid Git ref"));
    }
    Ok(raw.to_string())
}

pub fn normalize_subpath(raw: &str) -> Result<Option<String>> {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok(None);
    }
    for segment in trimmed.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains('\\')
            || segment.chars().any(char::is_control)
        {
            return Err(KisekiError::unsafe_path(
                raw,
                "subpath must be a clean relative path without '..'",
            ));
        }
    }
    Ok(Some(trimmed.to_string()))
}

// ---------------------------------------------------------------------------
// Installed theme origin tracking (stored OUTSIDE community theme dirs so the
// strict whitelist validator never sees engine metadata inside a theme pack)
// ---------------------------------------------------------------------------

/// How an installed community theme was acquired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeOrigin {
    pub id: String,
    /// `"gallery"` | `"github"` | `"archive"` | `"local"`
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub installed_at: String,
}

/// Persistent map of installed community theme origins (`gallery-origins.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRegistry {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub themes: BTreeMap<String, ThemeOrigin>,
}

impl Default for OriginRegistry {
    fn default() -> Self {
        Self {
            schema_version: GALLERY_SCHEMA_VERSION,
            themes: BTreeMap::new(),
        }
    }
}

impl OriginRegistry {
    pub fn load(platform: &dyn Platform, paths: &KisekiPaths) -> Result<Self> {
        let file = paths.gallery_origins_file();
        let Some(raw) = platform.read_to_string(&file)? else {
            return Ok(Self::default());
        };
        serde_json::from_str(&raw)
            .map_err(|error| KisekiError::State(format!("{}: {error}", file.display())))
    }

    pub fn save(&self, platform: &dyn Platform, paths: &KisekiPaths) -> Result<()> {
        let file = paths.gallery_origins_file();
        if let Some(parent) = file.parent() {
            platform.create_dir_all(parent)?;
        }
        let encoded = serde_json::to_string_pretty(self)
            .map_err(|error| KisekiError::Json(error.to_string()))?;
        platform.write_preserving_symlinks(&file, format!("{encoded}\n").as_bytes())?;
        Ok(())
    }

    pub fn record(
        platform: &dyn Platform,
        paths: &KisekiPaths,
        origin: ThemeOrigin,
    ) -> Result<()> {
        let mut reg = Self::load(platform, paths)?;
        reg.themes.insert(origin.id.clone(), origin);
        reg.save(platform, paths)
    }

    pub fn remove(platform: &dyn Platform, paths: &KisekiPaths, id: &str) -> Result<()> {
        let mut reg = Self::load(platform, paths)?;
        if reg.themes.remove(id).is_some() {
            reg.save(platform, paths)?;
        }
        Ok(())
    }
}
