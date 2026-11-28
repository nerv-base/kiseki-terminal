//! Theme library: embedded official themes, community packs and the loader the
//! compiler consumes.
//!
//! Precedence when two roots contain the same ID:
//! user-configured directories > community > official > bundled. A higher
//! precedence theme shadows a lower one without deleting it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kiseki_platform::{Platform, SyncMode};
use kiseki_schema::{ThemeManifest, ThemePack, ThemeTokens};
use kiseki_types::{Appearance, KisekiError, OsKind, RenderingProfile, Result, ThemeId};

use crate::bundled::{bundled_theme, bundled_themes, BundledFile, BundledTheme};
use crate::validate::{
    collect_files, read_text_bounded, validate_theme_dir, validate_theme_dir_with_assets,
    ValidationScope, MAX_JSON_BYTES,
};

/// Where a loaded theme came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThemeSource {
    Bundled,
    Official,
    Community,
    UserDirectory,
}

impl ThemeSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::Official => "official",
            Self::Community => "community",
            Self::UserDirectory => "user",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Bundled => "built-in",
            Self::Official => "official",
            Self::Community => "community",
            Self::UserDirectory => "user",
        }
    }
}

/// A theme as listed by `kiseki theme list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeEntry {
    pub id: ThemeId,
    pub name: String,
    pub version: String,
    pub appearance: Appearance,
    pub source: ThemeSource,
    pub root: Option<PathBuf>,
    pub tagline: Option<String>,
    pub layers: Vec<String>,
}

impl ThemeEntry {
    fn from_manifest(manifest: &ThemeManifest, source: ThemeSource, root: Option<PathBuf>) -> Self {
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            appearance: manifest.appearance,
            source,
            root,
            tagline: manifest.subtitle.clone(),
            layers: manifest.layers.iter().map(|layer| layer.as_str().to_string()).collect(),
        }
    }
}

/// A theme ready for compilation, with its external assets loaded.
#[derive(Debug, Clone)]
pub struct LoadedTheme {
    pub pack: ThemePack,
    pub source: ThemeSource,
    pub root: PathBuf,
    pub glsl: Option<String>,
    pub hlsl: Option<String>,
    pub logo: Option<String>,
}

impl LoadedTheme {
    pub fn exported(&self) -> bool {
        !matches!(self.source, ThemeSource::Bundled)
    }
}

/// Discovers and loads themes.
#[derive(Debug, Clone)]
pub struct ThemeLibrary {
    roots: Vec<(ThemeSource, PathBuf)>,
}

impl ThemeLibrary {
    /// Builds a library from the platform layout plus optional user directories.
    pub fn open(platform: &dyn Platform, extra_dirs: &[PathBuf]) -> Self {
        let paths = platform.paths();
        let mut roots = Vec::new();
        for directory in extra_dirs {
            roots.push((ThemeSource::UserDirectory, directory.clone()));
        }
        roots.push((ThemeSource::Community, paths.community_themes_dir()));
        roots.push((ThemeSource::Official, paths.official_themes_dir()));
        Self { roots }
    }

    pub fn roots(&self) -> &[(ThemeSource, PathBuf)] {
        &self.roots
    }

    /// Materialises every embedded theme into the official store.
    pub fn materialize_official(&self, platform: &dyn Platform) -> Result<usize> {
        let mut written = 0;
        for theme in bundled_themes() {
            if self.materialize_one(platform, theme)? {
                written += 1;
            }
        }
        Ok(written)
    }

    fn materialize_one(&self, platform: &dyn Platform, theme: &BundledTheme) -> Result<bool> {
        let official = platform.paths().official_themes_dir();
        let target = official.join(theme.id);
        let versions_path = official.join(".versions.json");
        let mut versions: std::collections::BTreeMap<String, String> = platform
            .read_to_string(&versions_path)?
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        if versions.get(theme.id).is_some_and(|version| version == theme.version)
            && target.join("manifest.json").exists()
        {
            return Ok(false);
        }
        let expected: BTreeSet<&str> = theme.files.iter().map(|file| file.path).collect();
        let mut dirs_to_sync = BTreeSet::new();
        for file in theme.files {
            let dest = target.join(file.path);
            platform.write_preserving_symlinks_with_sync(&dest, file.bytes, SyncMode::AtomicNoSync)?;
            if let Some(parent) = dest.parent() {
                dirs_to_sync.insert(parent.to_path_buf());
            }
        }
        // Clean up stale files left behind when an official pack upgrades
        // (e.g. a legacy `assets/wallpaper.png` superseded by `wallpaper.jpg`,
        // or the pre-release `.kiseki-bundled-version` marker).
        if target.is_dir() {
            for entry in collect_files(&target)? {
                if !entry.is_dir && !expected.contains(entry.relative.as_str()) {
                    let stale = target.join(&entry.relative);
                    platform.remove_file(&stale)?;
                    if let Some(parent) = stale.parent() {
                        dirs_to_sync.insert(parent.to_path_buf());
                    }
                }
            }
        }
        platform.remove_file(&target.join(".kiseki-bundled-version"))?;
        for dir in &dirs_to_sync {
            platform.sync_dir(dir)?;
        }
        versions.insert(theme.id.to_string(), theme.version.to_string());
        let encoded = serde_json::to_string_pretty(&versions)
            .map_err(|error| KisekiError::Json(error.to_string()))?;
        platform.write_preserving_symlinks_with_sync(
            &versions_path,
            format!("{encoded}\n").as_bytes(),
            SyncMode::FileOnly,
        )?;
        platform.sync_dir(&official)?;
        Ok(true)
    }

    /// Lists every theme, highest precedence last (so the map is deduped).
    pub fn list(&self, platform: &dyn Platform) -> Result<Vec<ThemeEntry>> {
        let mut entries: BTreeMap<String, ThemeEntry> = BTreeMap::new();
        for theme in bundled_themes() {
            entries.insert(
                theme.id.to_string(),
                ThemeEntry {
                    id: ThemeId::new(theme.id).expect("bundled theme id is valid"),
                    name: theme.name.to_string(),
                    version: theme.version.to_string(),
                    appearance: bundled_appearance(theme),
                    source: ThemeSource::Bundled,
                    root: None,
                    tagline: Some(theme.tagline.to_string()),
                    layers: bundled_layers(theme),
                },
            );
        }

        // Lowest precedence first so later inserts shadow earlier ones.
        for (source, root) in self.roots.iter().rev() {
            let dirs = platform.list_dir(root)?;
            for dir in dirs {
                if !dir.is_dir() {
                    continue;
                }
                let Some(manifest) = read_manifest(&dir)? else {
                    continue;
                };
                // When an official pack on disk lags behind the version compiled
                // into this binary, `load()` will upgrade it transparently; keep
                // the newer bundled entry in `list()` so users see the version
                // that will actually activate.
                if *source == ThemeSource::Official {
                    if let Some(bundled) = bundled_theme(manifest.id.as_str()) {
                        if manifest.version != bundled.version {
                            continue;
                        }
                    }
                }
                let entry = ThemeEntry::from_manifest(&manifest, *source, Some(dir.clone()));
                entries.insert(entry.id.as_str().to_string(), entry);
            }
        }

        Ok(entries.into_values().collect())
    }

    /// Loads a theme by ID, materialising or upgrading a bundled official theme
    /// when needed.
    ///
    /// Precedence stays `UserDirectory > Community > Official`: if a user or
    /// community pack shadows an official ID it is returned untouched; when
    /// resolving from `Official`, `materialize_one` checks `.versions.json` so
    /// a newer bundled version refreshes the on-disk official pack instead of
    /// serving stale files forever.
    pub fn load(&self, platform: &dyn Platform, id: &ThemeId) -> Result<LoadedTheme> {
        self.load_inner(platform, id, ValidationScope::AllProfiles)
    }

    /// Loads a theme by ID, validating shaders only for `profile` and reusing
    /// the content-hashed validation cache under `cache_dir`.
    pub fn load_for_profile(
        &self,
        platform: &dyn Platform,
        id: &ThemeId,
        profile: RenderingProfile,
    ) -> Result<LoadedTheme> {
        self.load_inner(platform, id, ValidationScope::Profile(profile))
    }

    fn load_inner(
        &self,
        platform: &dyn Platform,
        id: &ThemeId,
        scope: ValidationScope,
    ) -> Result<LoadedTheme> {
        self.load_with_validation_inner(platform, id, scope, true)
            .map(|(loaded, _)| loaded)
    }

    /// Loads a theme by ID and returns both [`LoadedTheme`] and its
    /// [`ThemeValidation`] in a single pass.
    pub fn load_with_validation(
        &self,
        platform: &dyn Platform,
        id: &ThemeId,
        scope: ValidationScope,
        write_cache: bool,
    ) -> Result<(LoadedTheme, crate::validate::ThemeValidation)> {
        if write_cache {
            self.load_with_validation_inner(platform, id, scope, true)
        } else {
            self.load_readonly_with_validation_inner(platform, id, scope)
        }
    }

    fn load_with_validation_inner(
        &self,
        platform: &dyn Platform,
        id: &ThemeId,
        scope: ValidationScope,
        write_cache: bool,
    ) -> Result<(LoadedTheme, crate::validate::ThemeValidation)> {
        let cache_dir = Some(platform.paths().cache_dir());
        for (source, root) in &self.roots {
            let dir = root.join(id.as_str());
            if *source == ThemeSource::Official {
                if let Some(theme) = bundled_theme(id.as_str()) {
                    self.materialize_one(platform, theme)?;
                }
            }
            if dir.join("manifest.json").exists() {
                return load_from_dir_with_validation(&dir, *source, cache_dir, scope, write_cache);
            }
        }
        if let Some(theme) = bundled_theme(id.as_str()) {
            self.materialize_one(platform, theme)?;
            let dir = platform.paths().official_themes_dir().join(theme.id);
            return load_from_dir_with_validation(
                &dir,
                ThemeSource::Official,
                cache_dir,
                scope,
                write_cache,
            );
        }
        Err(KisekiError::ThemeNotFound(id.to_string()))
    }

    /// Loads a theme from an explicit directory (for `theme validate` and SDK).
    pub fn load_path(&self, path: &Path) -> Result<LoadedTheme> {
        load_from_dir(path, ThemeSource::UserDirectory)
    }

    /// Loads a theme for rendering **without writing anything**.
    ///
    /// Installed packs are read from disk; a bundled pack is parsed straight
    /// out of the binary and pointed at the absolute path it *would* occupy
    /// after materialisation. `--dry-run` uses this so a preview never creates
    /// `official/<id>` or `.versions.json`.
    pub fn load_readonly(&self, platform: &dyn Platform, id: &ThemeId) -> Result<LoadedTheme> {
        self.load_readonly_inner(platform, id, ValidationScope::AllProfiles)
    }

    /// Read-only load scoped to `profile` (never writes cache or theme store).
    pub fn load_readonly_for_profile(
        &self,
        platform: &dyn Platform,
        id: &ThemeId,
        profile: RenderingProfile,
    ) -> Result<LoadedTheme> {
        self.load_readonly_inner(platform, id, ValidationScope::Profile(profile))
    }

    fn load_readonly_inner(
        &self,
        platform: &dyn Platform,
        id: &ThemeId,
        scope: ValidationScope,
    ) -> Result<LoadedTheme> {
        self.load_readonly_with_validation_inner(platform, id, scope)
            .map(|(loaded, _)| loaded)
    }

    fn load_readonly_with_validation_inner(
        &self,
        platform: &dyn Platform,
        id: &ThemeId,
        scope: ValidationScope,
    ) -> Result<(LoadedTheme, crate::validate::ThemeValidation)> {
        let cache_dir = Some(platform.paths().cache_dir());
        for (source, root) in &self.roots {
            let dir = root.join(id.as_str());
            if dir.join("manifest.json").exists() {
                return load_from_dir_with_validation(&dir, *source, cache_dir, scope, false);
            }
        }
        let Some(theme) = bundled_theme(id.as_str()) else {
            return Err(KisekiError::ThemeNotFound(id.to_string()));
        };
        let pack = pack_from_bundled(theme)?;
        let shader = pack
            .tokens
            .atmosphere
            .as_ref()
            .and_then(|atmosphere| atmosphere.shader.as_ref());
        let glsl = shader
            .and_then(|shader| shader.glsl.as_deref())
            .and_then(|path| theme.file(path))
            .and_then(BundledFile::text)
            .map(str::to_string);
        let hlsl = shader
            .and_then(|shader| shader.hlsl.as_deref())
            .and_then(|path| theme.file(path))
            .and_then(BundledFile::text)
            .map(str::to_string);
        let logo_relative = pack
            .tokens
            .overrides
            .as_ref()
            .and_then(|overrides| overrides.fastfetch.as_ref())
            .and_then(|fastfetch| fastfetch.logo_path.as_deref())
            .unwrap_or("assets/logo.txt");
        let logo = theme
            .file(logo_relative)
            .and_then(BundledFile::text)
            .map(str::to_string);
        let balanced = kiseki_schema::ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced);
        let contrast =
            kiseki_schema::lint_contrast(&balanced.palette, Some(&balanced.syntax));
        let validation = crate::validate::ThemeValidation {
            pack: Some(pack.clone()),
            diagnostics: contrast.to_diagnostics(),
            shader_reports: Vec::new(),
            contrast: Some(contrast),
            file_count: theme.files.len(),
            total_bytes: theme.files.iter().map(|file| file.bytes.len() as u64).sum(),
        };
        let loaded = LoadedTheme {
            pack,
            source: ThemeSource::Official,
            root: platform.paths().official_themes_dir().join(theme.id),
            glsl,
            hlsl,
            logo,
        };
        Ok((loaded, validation))
    }

    /// Re-materialises the embedded copy of `id`, repairing a damaged install.
    ///
    /// `recover --safe` uses this when the active pack can no longer be parsed:
    /// the embedded bytes are the last known-good source of truth.
    pub fn repair_bundled(&self, platform: &dyn Platform, id: &str) -> Result<LoadedTheme> {
        let Some(theme) = bundled_theme(id) else {
            return Err(KisekiError::ThemeNotFound(id.to_string()));
        };
        let target = platform.paths().official_themes_dir().join(theme.id);
        match std::fs::remove_dir_all(&target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(KisekiError::io(&target, error)),
        }
        self.materialize_one(platform, theme)?;
        load_from_dir(&target, ThemeSource::Official)
    }

    /// Reads a theme pack **without touching the filesystem**.
    ///
    /// Reads an installed pack from disk or parses the embedded copy in memory;
    /// it never calls `materialize_one`. The picker uses this so `kiseki init`
    /// followed by `Esc` writes nothing at all.
    pub fn load_pack_readonly(&self, id: &ThemeId) -> Result<ThemePack> {
        for (_, root) in &self.roots {
            let dir = root.join(id.as_str());
            if dir.join("manifest.json").exists() {
                return read_pack(&dir);
            }
        }
        if let Some(theme) = bundled_theme(id.as_str()) {
            return pack_from_bundled(theme);
        }
        Err(KisekiError::ThemeNotFound(id.to_string()))
    }
}

/// Parses `manifest.json` + `tokens.json` without validating or writing.
fn read_pack(dir: &Path) -> Result<ThemePack> {
    let manifest_path = dir.join("manifest.json");
    let tokens_path = dir.join("tokens.json");
    let manifest_raw = read_text_bounded(dir, "manifest.json", MAX_JSON_BYTES)?.ok_or_else(|| {
        KisekiError::io(
            &manifest_path,
            std::io::Error::from(std::io::ErrorKind::NotFound),
        )
    })?;
    let tokens_raw = read_text_bounded(dir, "tokens.json", MAX_JSON_BYTES)?.ok_or_else(|| {
        KisekiError::io(
            &tokens_path,
            std::io::Error::from(std::io::ErrorKind::NotFound),
        )
    })?;
    let manifest: ThemeManifest = serde_json::from_str(&manifest_raw)
        .map_err(|error| KisekiError::Json(format!("{}: {error}", manifest_path.display())))?;
    let tokens: ThemeTokens = serde_json::from_str(&tokens_raw)
        .map_err(|error| KisekiError::Json(format!("{}: {error}", tokens_path.display())))?;
    Ok(ThemePack { manifest, tokens })
}

/// Parses an embedded theme pack straight from the binary.
fn pack_from_bundled(theme: &BundledTheme) -> Result<ThemePack> {
    let manifest: ThemeManifest = serde_json::from_str(theme.manifest_json())
        .map_err(|error| KisekiError::Json(format!("bundled {}: {error}", theme.id)))?;
    let tokens: ThemeTokens = serde_json::from_str(theme.tokens_json())
        .map_err(|error| KisekiError::Json(format!("bundled {}: {error}", theme.id)))?;
    Ok(ThemePack { manifest, tokens })
}

fn read_manifest(dir: &Path) -> Result<Option<ThemeManifest>> {
    let path = dir.join("manifest.json");
    let Some(raw) = read_text_bounded(dir, "manifest.json", MAX_JSON_BYTES)? else {
        return Ok(None);
    };
    match serde_json::from_str(&raw) {
        Ok(manifest) => Ok(Some(manifest)),
        Err(error) => Err(KisekiError::Json(format!("{}: {error}", path.display()))),
    }
}

fn load_from_dir(dir: &Path, source: ThemeSource) -> Result<LoadedTheme> {
    load_from_dir_with_validation(dir, source, None, ValidationScope::AllProfiles, false)
        .map(|(loaded, _)| loaded)
}

fn load_from_dir_with_validation(
    dir: &Path,
    source: ThemeSource,
    cache_dir: Option<&Path>,
    scope: ValidationScope,
    write_cache: bool,
) -> Result<(LoadedTheme, crate::validate::ThemeValidation)> {
    // Validation is the security gate *and* the only place pack files are
    // read: structural failures abort before a manifest is opened and every
    // content read is bounded, so a hostile pack cannot make `use` allocate
    // unbounded memory.
    let (mut validation, assets) =
        validate_theme_dir_with_assets(dir, cache_dir, scope, write_cache)?;
    if !validation.is_valid() {
        let failures: Vec<String> = validation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_fatal())
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
            .collect();
        return Err(KisekiError::security(format!(
            "theme pack at {} failed validation: {}",
            dir.display(),
            failures.join("; ")
        )));
    }
    let Some(pack) = validation.pack.take() else {
        return Err(KisekiError::security(format!(
            "theme pack at {} has no readable manifest",
            dir.display()
        )));
    };
    validation.pack = Some(pack.clone());

    let glsl = assets.glsl;
    let hlsl = assets.hlsl;
    let logo_relative = pack
        .tokens
        .overrides
        .as_ref()
        .and_then(|overrides| overrides.fastfetch.as_ref())
        .and_then(|fastfetch| fastfetch.logo_path.as_deref())
        .unwrap_or("assets/logo.txt");
    let logo = if logo_relative == "assets/logo.txt" {
        assets.logo
    } else {
        read_optional(dir, Some(logo_relative))?
    };

    Ok((
        LoadedTheme {
            pack,
            source,
            root: dir.to_path_buf(),
            glsl,
            hlsl,
            logo,
        },
        validation,
    ))
}

fn read_optional(dir: &Path, relative: Option<&str>) -> Result<Option<String>> {
    let Some(relative) = relative else {
        return Ok(None);
    };
    let path = dir.join(relative);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(KisekiError::io(path, error)),
    }
}

fn bundled_appearance(theme: &BundledTheme) -> Appearance {
    serde_json::from_str::<ThemeManifest>(theme.manifest_json())
        .map(|manifest| manifest.appearance)
        .unwrap_or(Appearance::Dark)
}

fn bundled_layers(theme: &BundledTheme) -> Vec<String> {
    serde_json::from_str::<ThemeManifest>(theme.manifest_json())
        .map(|manifest| manifest.layers.iter().map(|layer| layer.as_str().to_string()).collect())
        .unwrap_or_default()
}

/// The default theme used for first-run state and `recover --safe`.
pub fn default_theme_id() -> ThemeId {
    ThemeId::new("nerv-eva").expect("default theme id is valid")
}

/// Returns the expected official theme root without performing IO.
pub fn official_root(paths: &kiseki_platform::KisekiPaths, id: &str) -> PathBuf {
    paths.official_themes_dir().join(id)
}

/// Validates an installed directory (thin wrapper for the CLI).
pub fn validate_installed(dir: &Path) -> Result<crate::validate::ThemeValidation> {
    validate_theme_dir(dir)
}

/// True when a directory contains a v2 theme (manifest + tokens).
pub fn is_theme_dir(dir: &Path) -> bool {
    dir.join("manifest.json").is_file() && dir.join("tokens.json").is_file()
}

/// Counts files in a pack without loading token data (for status output).
pub fn file_count(dir: &Path) -> usize {
    collect_files(dir)
        .map(|files| files.iter().filter(|file| !file.is_dir).count())
        .unwrap_or(0)
}

/// Platform-specific note used by doctor when the official store is missing.
pub fn official_store_hint(platform: &dyn Platform) -> String {
    match platform.os() {
        OsKind::Windows => format!(
            "run `kiseki init` to unpack into {}",
            platform.paths().official_themes_dir().display()
        ),
        _ => format!(
            "run `kiseki init` to unpack into {}",
            platform.paths().official_themes_dir().display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_platform::MockPlatform;

    fn platform() -> (tempfile::TempDir, MockPlatform) {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        (dir, platform)
    }

    #[test]
    fn bundled_theme_listing_works_before_materialisation() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        let entries = library.list(&platform).unwrap();
        assert_eq!(entries.len(), 4);
        let nerv = entries
            .iter()
            .find(|entry| entry.id.as_str() == "nerv-eva")
            .expect("nerv-eva is listed");
        assert_eq!(nerv.source, ThemeSource::Bundled);
        assert!(nerv.root.is_none());
        assert!(nerv.layers.contains(&"atmosphere".to_string()));
    }

    #[test]
    fn loading_materialises_the_official_pack() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        let loaded = library
            .load(&platform, &ThemeId::new("nerv-eva").unwrap())
            .unwrap();
        assert_eq!(loaded.source, ThemeSource::Official);
        assert!(loaded.root.starts_with(platform.paths().official_themes_dir()));
        assert!(loaded.root.join("assets/wallpaper.png").exists());
        assert!(loaded.glsl.as_deref().unwrap().contains("mainImage"));
        assert!(loaded.hlsl.as_deref().unwrap().contains("cbuffer PixelShaderSettings"));
        assert!(loaded.logo.as_deref().unwrap().contains("\u{1b}["));
        assert_eq!(loaded.pack.manifest.id.as_str(), "nerv-eva");

        // Materialising official packs writes only the remaining 3 themes,
        // and a subsequent pass writes nothing.
        let written = library.materialize_official(&platform).unwrap();
        assert_eq!(written, 3);
        let written_again = library.materialize_official(&platform).unwrap();
        assert_eq!(written_again, 0);
    }

    #[test]
    fn loading_refreshes_an_outdated_official_pack_version() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        let id = ThemeId::new("starlight-8bit").unwrap();
        let first = library.load(&platform, &id).unwrap();

        // Simulate an older 2.0.0 materialized copy on disk.
        let official = platform.paths().official_themes_dir();
        platform
            .write_preserving_symlinks(
                &official.join(".versions.json"),
                br#"{"starlight-8bit":"2.0.0"}"#,
            )
            .unwrap();
        let glsl_path = first.root.join("shaders/live.glsl");
        platform
            .write_preserving_symlinks(&glsl_path, b"// stale 2.0.0 shader\n")
            .unwrap();

        let refreshed = library.load(&platform, &id).unwrap();
        assert!(
            refreshed.glsl.as_deref().unwrap().contains("mainImage"),
            "newer bundled version must overwrite stale official pack files"
        );
        let versions = std::fs::read_to_string(official.join(".versions.json")).unwrap();
        assert!(versions.contains(bundled_theme("starlight-8bit").unwrap().version));
    }

    #[test]
    fn upgrading_bundled_pack_removes_stale_wallpaper_png() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        let id = ThemeId::new("shinkai-rain").unwrap();
        let official = platform.paths().official_themes_dir();
        let root = official.join("shinkai-rain");

        // Simulate an older 2.0.2 install that had `assets/wallpaper.png`.
        platform
            .write_preserving_symlinks(
                &official.join(".versions.json"),
                br#"{"shinkai-rain":"2.0.2"}"#,
            )
            .unwrap();
        let legacy_png = root.join("assets/wallpaper.png");
        platform
            .write_preserving_symlinks(&legacy_png, b"legacy png bytes")
            .unwrap();
        assert!(legacy_png.exists());

        let loaded = library.load(&platform, &id).unwrap();
        assert!(
            !legacy_png.exists(),
            "stale assets/wallpaper.png must be deleted on bundled upgrade"
        );
        assert!(loaded.root.join("assets/wallpaper.jpg").exists());
    }

    #[test]
    fn unknown_theme_is_a_clear_error() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        let error = library
            .load(&platform, &ThemeId::new("does-not-exist").unwrap())
            .unwrap_err();
        assert!(matches!(error, KisekiError::ThemeNotFound(_)));
    }

    #[test]
    fn community_theme_shadows_the_official_one() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        library
            .load(&platform, &ThemeId::new("nerv-eva").unwrap())
            .unwrap();
        // Install a community copy with a different version.
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("themes")
            .join("nerv-eva");
        crate::validate::install_from_dir(&platform, &source, true).unwrap();
        let community_manifest = platform
            .paths()
            .community_themes_dir()
            .join("nerv-eva")
            .join("manifest.json");
        let mut manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&community_manifest).unwrap(),
        )
        .unwrap();
        manifest["name"] = serde_json::json!("Community NERV");
        platform
            .write_preserving_symlinks(
                &community_manifest,
                serde_json::to_string_pretty(&manifest).unwrap().as_bytes(),
            )
            .unwrap();

        let loaded = library
            .load(&platform, &ThemeId::new("nerv-eva").unwrap())
            .unwrap();
        assert_eq!(loaded.source, ThemeSource::Community);
        assert_eq!(loaded.pack.manifest.name, "Community NERV");
        let entries = library.list(&platform).unwrap();
        assert_eq!(entries.len(), 4);
        let nerv = entries
            .iter()
            .find(|entry| entry.id.as_str() == "nerv-eva")
            .expect("nerv-eva is listed");
        assert_eq!(nerv.source, ThemeSource::Community);
    }

    #[test]
    fn default_theme_and_helpers() {
        assert_eq!(default_theme_id().as_str(), "nerv-eva");
        assert!(is_theme_dir(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../themes/nerv-eva")));
        assert!(file_count(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../themes/nerv-eva")) >= 9);
    }

    #[test]
    fn readonly_pack_loading_writes_nothing() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        let pack = library
            .load_pack_readonly(&ThemeId::new("nerv-eva").unwrap())
            .unwrap();
        assert_eq!(pack.manifest.id.as_str(), "nerv-eva");
        assert!(pack.tokens.atmosphere.is_some());
        // No `official/nerv-eva`, no `.versions.json`, not even the directory.
        let official = platform.paths().official_themes_dir();
        assert!(!official.join("nerv-eva").exists());
        assert!(!official.join(".versions.json").exists());
        assert!(!official.exists(), "the official store must not be created");

        let error = library
            .load_pack_readonly(&ThemeId::new("does-not-exist").unwrap())
            .unwrap_err();
        assert!(matches!(error, KisekiError::ThemeNotFound(_)));
    }

    #[test]
    fn readonly_theme_loading_never_materialises() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        let loaded = library
            .load_readonly(&platform, &ThemeId::new("nerv-eva").unwrap())
            .unwrap();
        assert_eq!(loaded.pack.manifest.id.as_str(), "nerv-eva");
        assert!(loaded.glsl.as_deref().unwrap().contains("mainImage"));
        assert!(loaded.hlsl.as_deref().unwrap().contains("cbuffer PixelShaderSettings"));
        assert!(loaded.logo.as_deref().unwrap().contains("\u{1b}["));
        // The root is the path the pack *would* occupy, but nothing was written.
        assert_eq!(
            loaded.root,
            platform.paths().official_themes_dir().join("nerv-eva")
        );
        assert!(!loaded.root.exists());
        assert!(!platform.paths().official_themes_dir().exists());
    }

    #[test]
    fn oversized_pack_json_is_rejected_without_a_full_read() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        library
            .load(&platform, &ThemeId::new("nerv-eva").unwrap())
            .unwrap();
        let tokens = platform
            .paths()
            .official_themes_dir()
            .join("nerv-eva/tokens.json");
        std::fs::write(&tokens, vec![b' '; crate::validate::MAX_JSON_BYTES + 1]).unwrap();

        let error = library
            .load_pack_readonly(&ThemeId::new("nerv-eva").unwrap())
            .unwrap_err();
        assert!(error.to_string().contains("exceeds"), "{error}");

        // `use` goes through the validator, which also refuses the oversize
        // read before parsing anything.
        let error = library
            .load(&platform, &ThemeId::new("nerv-eva").unwrap())
            .unwrap_err();
        assert!(error.to_string().contains("failed validation"), "{error}");
    }

    #[test]
    fn invalid_installed_pack_is_rejected_by_validation_before_parsing() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        library
            .load(&platform, &ThemeId::new("nerv-eva").unwrap())
            .unwrap();
        let root = platform.paths().official_themes_dir().join("nerv-eva");
        // A disallowed script plus an unparseable manifest: the structural
        // failure must abort before the manifest is read.
        std::fs::write(root.join("evil.lua"), "os.execute('rm -rf /')").unwrap();
        std::fs::write(root.join("manifest.json"), b"\xFF\xFE not json").unwrap();
        let error = library
            .load(&platform, &ThemeId::new("nerv-eva").unwrap())
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("security.file-not-allowed"), "{message}");
        assert!(!message.contains("validation.manifest"), "{message}");
    }

    #[test]
    fn readonly_pack_loading_prefers_installed_packs() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        // Materialise, rename, then confirm the read-only path reads the
        // installed copy instead of the embedded one.
        library
            .load(&platform, &ThemeId::new("nerv-eva").unwrap())
            .unwrap();
        let manifest = platform.paths().official_themes_dir().join("nerv-eva/manifest.json");
        let mut value: serde_json::Value =
            serde_json::from_str(&platform.read_to_string(&manifest).unwrap().unwrap()).unwrap();
        value["name"] = serde_json::json!("Installed Copy");
        platform
            .write_preserving_symlinks(
                &manifest,
                serde_json::to_string_pretty(&value).unwrap().as_bytes(),
            )
            .unwrap();
        let pack = library
            .load_pack_readonly(&ThemeId::new("nerv-eva").unwrap())
            .unwrap();
        assert_eq!(pack.manifest.name, "Installed Copy");
    }

    #[test]
    fn load_readonly_does_not_write_validation_cache() {
        let (_dir, platform) = platform();
        let library = ThemeLibrary::open(&platform, &[]);
        library
            .load_for_profile(
                &platform,
                &ThemeId::new("nerv-eva").unwrap(),
                RenderingProfile::Balanced,
            )
            .unwrap();
        assert!(platform.paths().cache_dir().join("validation").exists());

        // Clear cache, then ensure load_readonly_for_profile never creates it.
        std::fs::remove_dir_all(platform.paths().cache_dir().join("validation")).unwrap();
        library
            .load_readonly_for_profile(
                &platform,
                &ThemeId::new("nerv-eva").unwrap(),
                RenderingProfile::Balanced,
            )
            .unwrap();
        assert!(!platform.paths().cache_dir().join("validation").exists());
    }
}
