//! Pure-Rust font discovery and Font Catalog ID resolution (RFC-0001 §6).
//!
//! `ttf-parser` reads the OpenType `name` table directly, so no `osascript`,
//! `fc-list`, `system_profiler` or `fc-cache` subprocess is ever needed. Scans
//! are cached per directory and invalidated by that directory's mtime.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use kiseki_types::OsKind;
use serde::{Deserialize, Serialize};

/// Embedded catalog of officially recommended fonts.
const CATALOG_JSON: &str = include_str!("../font-catalog.json");

/// Family names per platform, because vendors ship different spellings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontFamilies {
    #[serde(default)]
    pub macos: Vec<String>,
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub linux: Vec<String>,
}

impl FontFamilies {
    pub fn for_os(&self, os: OsKind) -> &[String] {
        match os {
            OsKind::MacOs => &self.macos,
            OsKind::Windows => &self.windows,
            OsKind::Linux => &self.linux,
        }
    }
}

/// A downloadable upstream release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FontDownload {
    pub url: String,
    pub file_name: String,
    pub sha256: String,
}

/// One entry in `font-catalog.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FontCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub license: String,
    pub version: String,
    pub families: FontFamilies,
    #[serde(default)]
    pub post_script_names: Vec<String>,
    #[serde(default)]
    pub download: Option<FontDownload>,
}

/// The whole catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FontCatalog {
    pub version: u32,
    #[serde(default)]
    pub generated_at: Option<String>,
    pub fonts: Vec<FontCatalogEntry>,
}

/// Returns the embedded catalog, parsed once.
pub fn catalog() -> &'static FontCatalog {
    static CATALOG: OnceLock<FontCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(CATALOG_JSON).expect("embedded font-catalog.json must be valid")
    })
}

/// Looks a Font Catalog ID up by its stable identifier.
pub fn resolve_font_id(id: &str) -> Option<&'static FontCatalogEntry> {
    catalog().fonts.iter().find(|entry| entry.id == id)
}

/// One installed face discovered on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontFaceInfo {
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_script_name: Option<String>,
    pub path: PathBuf,
}

/// Every face found in the scanned directories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontInventory {
    pub faces: Vec<FontFaceInfo>,
    pub families: BTreeSet<String>,
    pub post_script_names: BTreeSet<String>,
    pub scanned_dirs: usize,
    pub unreadable_files: usize,
}

impl FontInventory {
    fn absorb(&mut self, faces: Vec<FontFaceInfo>) {
        for face in faces {
            if !self.families.contains(&face.family) {
                self.families.insert(face.family.clone());
            }
            if let Some(post_script) = &face.post_script_name {
                self.post_script_names.insert(post_script.clone());
            }
            self.faces.push(face);
        }
    }

    /// Case-insensitive family lookup.
    pub fn has_family(&self, family: &str) -> bool {
        self.families
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(family))
    }

    /// Finds the concrete installed spelling of a family name.
    pub fn canonical_family(&self, family: &str) -> Option<&str> {
        self.families
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(family))
            .map(String::as_str)
    }

    /// Case-insensitive PostScript name lookup.
    pub fn has_post_script_name(&self, name: &str) -> bool {
        self.post_script_names
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone)]
struct CacheEntry {
    modified: Option<std::time::SystemTime>,
    faces: Vec<FontFaceInfo>,
}

/// Caches parsed faces per directory.
///
/// A cache hit costs one `metadata()` call per directory, which is what keeps
/// `kiseki use` inside its 120 ms hot-path budget on machines with thousands of
/// installed fonts.
#[derive(Debug, Default)]
pub struct FontScanner {
    cache: Mutex<BTreeMap<PathBuf, CacheEntry>>,
    parses: std::sync::atomic::AtomicUsize,
    hits: std::sync::atomic::AtomicUsize,
}

impl FontScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of font files actually parsed (test/diagnostic hook).
    pub fn parses(&self) -> usize {
        self.parses.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Number of directory cache hits.
    pub fn cache_hits(&self) -> usize {
        self.hits.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn cache_entries(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    /// Scans every directory, reusing cached parses where the mtime matches.
    pub fn scan(&self, dirs: &[PathBuf]) -> FontInventory {
        let mut inventory = FontInventory::default();
        for dir in dirs {
            inventory.scanned_dirs += 1;
            let faces = self.scan_dir(dir, &mut inventory);
            inventory.absorb(faces);
        }
        inventory
    }

    fn scan_dir(&self, dir: &Path, inventory: &mut FontInventory) -> Vec<FontFaceInfo> {
        let Ok(metadata) = std::fs::metadata(dir) else {
            return Vec::new();
        };
        if !metadata.is_dir() {
            return Vec::new();
        }
        let modified = metadata.modified().ok();
        {
            let cache = self.cache.lock().unwrap();
            if let Some(entry) = cache.get(dir) {
                if entry.modified == modified {
                    self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return entry.faces.clone();
                }
            }
        }

        let mut faces = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return faces;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                faces.extend(self.scan_dir(&path, inventory));
                continue;
            }
            if !file_type.is_file() || !is_font_file(&path) {
                continue;
            }
            match parse_face(&path) {
                Some(face) => {
                    self.parses.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    faces.push(face);
                }
                None => inventory.unreadable_files += 1,
            }
        }

        self.cache.lock().unwrap().insert(
            dir.to_path_buf(),
            CacheEntry {
                modified,
                faces: faces.clone(),
            },
        );
        faces
    }
}

fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc"
            )
        })
        .unwrap_or(false)
}

/// Reads the family and PostScript names out of a font file.
fn parse_face(path: &Path) -> Option<FontFaceInfo> {
    let data = std::fs::read(path).ok()?;
    // Collections contain several faces; index 0 is the regular weight in every
    // font we ship, and it keeps the scan cost flat.
    let face = ttf_parser::Face::parse(&data, 0).ok()?;
    let mut family = None;
    let mut post_script = None;
    for name in face.names() {
        if !name.is_unicode() {
            continue;
        }
        if name.name_id == ttf_parser::name_id::FAMILY && family.is_none() {
            family = name.to_string();
        }
        if name.name_id == ttf_parser::name_id::POST_SCRIPT_NAME && post_script.is_none() {
            post_script = name.to_string();
        }
    }
    let family = family?;
    Some(FontFaceInfo {
        family,
        post_script_name: post_script,
        path: path.to_path_buf(),
    })
}

/// Conventional font directories for the host.
pub fn default_font_dirs(os: OsKind, home: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match os {
        OsKind::MacOs => {
            dirs.push(PathBuf::from("/System/Library/Fonts"));
            dirs.push(PathBuf::from("/Library/Fonts"));
            if let Some(home) = home {
                dirs.push(home.join("Library").join("Fonts"));
            }
        }
        OsKind::Linux => {
            if let Some(home) = home {
                dirs.push(home.join(".local").join("share").join("fonts"));
                dirs.push(home.join(".fonts"));
            }
            if let Some(data_home) = env("XDG_DATA_HOME") {
                dirs.push(PathBuf::from(data_home).join("fonts"));
            }
            dirs.push(PathBuf::from("/usr/share/fonts"));
            dirs.push(PathBuf::from("/usr/local/share/fonts"));
        }
        OsKind::Windows => {
            if let Some(windir) = env("WINDIR") {
                dirs.push(PathBuf::from(windir).join("Fonts"));
            } else {
                dirs.push(PathBuf::from("C:\\Windows\\Fonts"));
            }
            if let Some(local) = env("LOCALAPPDATA") {
                dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
            }
        }
    }
    dirs
}

/// Resolution result for one `typography.families` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontStatus {
    /// The string the theme declared (catalog ID or literal family).
    pub requested: String,
    /// The catalog ID, when the request matched one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<String>,
    /// The concrete family name to write into terminal configs.
    pub resolved_family: String,
    /// True when the family (or a catalog alias) exists on disk.
    pub installed: bool,
    /// Where the family was found, when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl FontStatus {
    /// The value terminal configs should use.
    pub fn config_family(&self) -> &str {
        &self.resolved_family
    }
}

/// Resolves each declared family against the catalog and the installed fonts.
pub fn font_status(families: &[String], inventory: &FontInventory, os: OsKind) -> Vec<FontStatus> {
    families
        .iter()
        .map(|requested| resolve_one(requested, inventory, os))
        .collect()
}

fn resolve_one(requested: &str, inventory: &FontInventory, os: OsKind) -> FontStatus {
    if let Some(entry) = resolve_font_id(requested) {
        for candidate in entry.families.for_os(os) {
            if let Some(canonical) = inventory.canonical_family(candidate) {
                return FontStatus {
                    requested: requested.to_string(),
                    catalog_id: Some(entry.id.clone()),
                    resolved_family: canonical.to_string(),
                    installed: true,
                    path: inventory
                        .faces
                        .iter()
                        .find(|face| face.family.eq_ignore_ascii_case(canonical))
                        .map(|face| face.path.clone()),
                };
            }
        }
        for post_script in &entry.post_script_names {
            if inventory.has_post_script_name(post_script) {
                return FontStatus {
                    requested: requested.to_string(),
                    catalog_id: Some(entry.id.clone()),
                    resolved_family: entry
                        .families
                        .for_os(os)
                        .first()
                        .cloned()
                        .unwrap_or_else(|| entry.display_name.clone()),
                    installed: true,
                    path: None,
                };
            }
        }
        return FontStatus {
            requested: requested.to_string(),
            catalog_id: Some(entry.id.clone()),
            resolved_family: entry
                .families
                .for_os(os)
                .first()
                .cloned()
                .unwrap_or_else(|| entry.display_name.clone()),
            installed: false,
            path: None,
        };
    }

    let installed = inventory.canonical_family(requested);
    FontStatus {
        requested: requested.to_string(),
        catalog_id: None,
        resolved_family: installed.unwrap_or(requested).to_string(),
        installed: installed.is_some(),
        path: installed.and_then(|canonical| {
            inventory
                .faces
                .iter()
                .find(|face| face.family.eq_ignore_ascii_case(canonical))
                .map(|face| face.path.clone())
        }),
    }
}

/// Config-ready family list: every entry resolved to a concrete name.
pub fn config_families(statuses: &[FontStatus]) -> Vec<String> {
    statuses
        .iter()
        .map(|status| status.resolved_family.clone())
        .collect()
}

/// The catalog entry IDs referenced by a family list.
pub fn referenced_catalog_ids(families: &[String]) -> Vec<&'static str> {
    let mut ids = BTreeSet::new();
    for family in families {
        if let Some(entry) = resolve_font_id(family) {
            ids.insert(entry.id.as_str());
        }
    }
    ids.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Builds a minimal but valid TrueType file with one `name` record.
    fn minimal_ttf(family: &str, post_script: Option<&str>) -> Vec<u8> {
        fn be16(value: u16) -> [u8; 2] {
            value.to_be_bytes()
        }
        fn be32(value: u32) -> [u8; 4] {
            value.to_be_bytes()
        }

        let mut head = vec![0u8; 54];
        head[0..4].copy_from_slice(&be32(0x0001_0000));
        head[12..16].copy_from_slice(&be32(0x5F0F_3CF5));
        head[18..20].copy_from_slice(&be16(1000));

        let mut hhea = vec![0u8; 36];
        hhea[0..4].copy_from_slice(&be32(0x0001_0000));
        hhea[34..36].copy_from_slice(&be16(1));

        let mut maxp = vec![0u8; 6];
        maxp[0..4].copy_from_slice(&be32(0x0000_5000));
        maxp[4..6].copy_from_slice(&be16(1));

        let hmtx = vec![0u8; 4];

        let mut cmap_sub = vec![0u8; 32];
        cmap_sub[0..2].copy_from_slice(&be16(4));
        cmap_sub[2..4].copy_from_slice(&be16(32));
        cmap_sub[6..8].copy_from_slice(&be16(4));
        cmap_sub[14..16].copy_from_slice(&be16(0x20));
        cmap_sub[16..18].copy_from_slice(&be16(0xFFFF));
        cmap_sub[20..22].copy_from_slice(&be16(0x20));
        cmap_sub[22..24].copy_from_slice(&be16(0xFFFF));
        cmap_sub[26..28].copy_from_slice(&be16(1));
        let mut cmap = vec![0u8; 12];
        cmap[2..4].copy_from_slice(&be16(1));
        cmap[4..8].copy_from_slice(&be32(3));
        cmap[8..12].copy_from_slice(&be32(12));
        cmap.extend_from_slice(&cmap_sub);

        let mut records = Vec::new();
        let mut strings = Vec::new();
        for (name_id, text) in [(1u16, family), (6u16, post_script.unwrap_or(""))] {
            if text.is_empty() {
                continue;
            }
            let mut utf16 = Vec::new();
            for unit in text.encode_utf16() {
                utf16.extend_from_slice(&be16(unit));
            }
            records.extend_from_slice(&be16(3));
            records.extend_from_slice(&be16(1));
            records.extend_from_slice(&be16(0x0409));
            records.extend_from_slice(&be16(name_id));
            records.extend_from_slice(&be16(utf16.len() as u16));
            records.extend_from_slice(&be16(strings.len() as u16));
            strings.extend_from_slice(&utf16);
        }
        let mut name = vec![0u8; 6];
        name[2..4].copy_from_slice(&be16((records.len() / 12) as u16));
        name[4..6].copy_from_slice(&be16((6 + records.len()) as u16));
        name.extend_from_slice(&records);
        name.extend_from_slice(&strings);

        let tables: Vec<(&str, Vec<u8>)> = vec![
            ("cmap", cmap),
            ("head", head),
            ("hhea", hhea),
            ("hmtx", hmtx),
            ("maxp", maxp),
            ("name", name),
        ];
        let count = tables.len() as u16;
        let mut out = Vec::new();
        out.extend_from_slice(&be32(0x0001_0000));
        out.extend_from_slice(&be16(count));
        out.extend_from_slice(&be16(64));
        out.extend_from_slice(&be16(3));
        out.extend_from_slice(&be16(0));
        let mut offset = 12 + 16 * u32::from(count);
        let mut records_out = Vec::new();
        for (tag, data) in &tables {
            records_out.extend_from_slice(tag.as_bytes());
            records_out.extend_from_slice(&be32(0));
            records_out.extend_from_slice(&be32(offset));
            records_out.extend_from_slice(&be32(data.len() as u32));
            offset += data.len() as u32;
            offset += (4 - (data.len() % 4)) as u32 % 4;
        }
        out.extend_from_slice(&records_out);
        for (_, data) in &tables {
            out.extend_from_slice(data);
            let padding = (4 - (data.len() % 4)) % 4;
            out.resize(out.len() + padding, 0);
        }
        out
    }

    fn write_font(dir: &Path, file: &str, family: &str, post_script: Option<&str>) {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut handle = std::fs::File::create(&path).unwrap();
        handle.write_all(&minimal_ttf(family, post_script)).unwrap();
    }

    #[test]
    fn catalog_is_valid_and_ids_are_stable() {
        let catalog = catalog();
        assert_eq!(catalog.version, 1);
        assert_eq!(catalog.fonts.len(), 4);
        for id in [
            "jetbrains-mono-nf",
            "maple-mono-nf-cn",
            "departure-mono-nf",
            "fusion-pixel-12-zh",
        ] {
            let entry = resolve_font_id(id).unwrap();
            assert!(!entry.families.for_os(OsKind::MacOs).is_empty());
            assert!(!entry.families.for_os(OsKind::Windows).is_empty());
            assert!(!entry.families.for_os(OsKind::Linux).is_empty());
        }
        assert!(resolve_font_id("nope").is_none());
    }

    #[test]
    fn scanner_reads_family_and_postscript_names() {
        let dir = tempfile::tempdir().unwrap();
        write_font(dir.path(), "TestMono.ttf", "Test Mono", Some("TestMono-Regular"));
        let scanner = FontScanner::new();
        let inventory = scanner.scan(&[dir.path().to_path_buf()]);
        assert_eq!(inventory.faces.len(), 1);
        assert!(inventory.has_family("Test Mono"));
        assert!(inventory.has_family("test mono"));
        assert!(inventory.has_post_script_name("TestMono-Regular"));
        assert_eq!(inventory.canonical_family("test mono"), Some("Test Mono"));
    }

    #[test]
    fn scanner_cache_is_invalidated_by_mtime() {
        let dir = tempfile::tempdir().unwrap();
        write_font(dir.path(), "A.ttf", "Alpha", None);
        let scanner = FontScanner::new();
        let first = scanner.scan(&[dir.path().to_path_buf()]);
        assert_eq!(first.faces.len(), 1);
        assert_eq!(scanner.parses(), 1);

        let second = scanner.scan(&[dir.path().to_path_buf()]);
        assert_eq!(second.faces.len(), 1);
        assert_eq!(scanner.parses(), 1, "second scan must hit the cache");
        assert!(scanner.cache_hits() >= 1);

        // Change the directory mtime by adding a file.
        write_font(dir.path(), "B.ttf", "Beta", None);
        let third = scanner.scan(&[dir.path().to_path_buf()]);
        assert_eq!(third.faces.len(), 2);
        assert!(scanner.parses() >= 2);
    }

    #[test]
    fn nested_directories_are_scanned() {
        let dir = tempfile::tempdir().unwrap();
        write_font(&dir.path().join("nested"), "C.ttf", "Gamma", None);
        let scanner = FontScanner::new();
        let inventory = scanner.scan(&[dir.path().to_path_buf()]);
        assert!(inventory.has_family("Gamma"));
    }

    #[test]
    fn unreadable_and_non_font_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.ttf"), b"not a font").unwrap();
        std::fs::write(dir.path().join("readme.txt"), b"hi").unwrap();
        let scanner = FontScanner::new();
        let inventory = scanner.scan(&[dir.path().to_path_buf()]);
        assert!(inventory.faces.is_empty());
        assert_eq!(inventory.unreadable_files, 1);
    }

    #[test]
    fn catalog_ids_resolve_to_installed_families() {
        let dir = tempfile::tempdir().unwrap();
        write_font(dir.path(), "JetBrains.ttf", "JetBrainsMono Nerd Font", Some("JetBrainsMonoNF-Regular"));
        let scanner = FontScanner::new();
        let inventory = scanner.scan(&[dir.path().to_path_buf()]);

        let statuses = font_status(
            &["jetbrains-mono-nf".to_string(), "Missing Mono".to_string()],
            &inventory,
            OsKind::MacOs,
        );
        assert_eq!(statuses[0].catalog_id.as_deref(), Some("jetbrains-mono-nf"));
        assert!(statuses[0].installed);
        assert_eq!(statuses[0].resolved_family, "JetBrainsMono Nerd Font");
        assert_eq!(statuses[0].path.as_ref().unwrap().file_name().unwrap(), "JetBrains.ttf");

        assert!(!statuses[1].installed);
        assert_eq!(statuses[1].resolved_family, "Missing Mono");
        assert_eq!(
            config_families(&statuses),
            vec!["JetBrainsMono Nerd Font", "Missing Mono"]
        );
    }

    #[test]
    fn catalog_fallback_uses_display_family_when_missing() {
        let inventory = FontInventory::default();
        let statuses = font_status(&["maple-mono-nf-cn".to_string()], &inventory, OsKind::Linux);
        assert!(!statuses[0].installed);
        assert_eq!(statuses[0].resolved_family, "Maple Mono NF CN");
    }

    #[test]
    fn referenced_ids_are_deduplicated_and_sorted() {
        let families = vec![
            "jetbrains-mono-nf".to_string(),
            "literal".to_string(),
            "jetbrains-mono-nf".to_string(),
            "departure-mono-nf".to_string(),
        ];
        assert_eq!(
            referenced_catalog_ids(&families),
            vec!["departure-mono-nf", "jetbrains-mono-nf"]
        );
    }

    #[test]
    fn default_dirs_match_the_platform_conventions() {
        let home = Path::new("/home/zz");
        let env = |key: &str| match key {
            "XDG_DATA_HOME" => Some("/home/zz/.data".to_string()),
            _ => None,
        };
        let macos = default_font_dirs(OsKind::MacOs, Some(home), &env);
        assert!(macos.contains(&PathBuf::from("/System/Library/Fonts")));
        assert!(macos.contains(&home.join("Library").join("Fonts")));

        let linux = default_font_dirs(OsKind::Linux, Some(home), &env);
        assert!(linux.contains(&home.join(".local").join("share").join("fonts")));
        assert!(linux.contains(&PathBuf::from("/usr/share/fonts")));
        assert!(linux.contains(&PathBuf::from("/home/zz/.data/fonts")));

        let windows = default_font_dirs(OsKind::Windows, Some(home), &env);
        assert!(!windows.is_empty());
    }
}
