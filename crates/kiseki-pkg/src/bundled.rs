//! Themes embedded in the binary so first-run is fully offline (ADR-013).
//!
//! Milestone 1 ships `nerv-eva`; the other three official themes migrate in
//! Milestone 2. Files are materialised into `<kiseki_data>/themes/official/`
//! on first use so absolute asset paths and `kiseki theme validate` work
//! exactly like a community install.

/// One embedded file.
#[derive(Debug, Clone, Copy)]
pub struct BundledFile {
    /// Slash-separated path relative to the theme root.
    pub path: &'static str,
    pub bytes: &'static [u8],
}

impl BundledFile {
    pub const fn new(path: &'static str, bytes: &'static [u8]) -> Self {
        Self { path, bytes }
    }

    pub fn text(&self) -> Option<&'static str> {
        std::str::from_utf8(self.bytes).ok()
    }
}

/// An official theme compiled into the binary.
#[derive(Debug, Clone, Copy)]
pub struct BundledTheme {
    pub id: &'static str,
    pub version: &'static str,
    pub name: &'static str,
    pub tagline: &'static str,
    pub files: &'static [BundledFile],
}

impl BundledTheme {
    pub fn file(&self, path: &str) -> Option<&'static BundledFile> {
        self.files.iter().find(|file| file.path == path)
    }

    pub fn manifest_json(&self) -> &'static str {
        self.file("manifest.json")
            .and_then(BundledFile::text)
            .expect("bundled manifest.json must be UTF-8")
    }

    pub fn tokens_json(&self) -> &'static str {
        self.file("tokens.json")
            .and_then(BundledFile::text)
            .expect("bundled tokens.json must be UTF-8")
    }

    pub fn glsl(&self) -> Option<&'static str> {
        self.file("shaders/live.glsl").and_then(BundledFile::text)
    }

    pub fn hlsl(&self) -> Option<&'static str> {
        self.file("shaders/live.hlsl").and_then(BundledFile::text)
    }

    pub fn logo(&self) -> Option<&'static str> {
        self.file("assets/logo.txt").and_then(BundledFile::text)
    }

    /// Everything needed to compute a pack listing.
    pub fn paths(&self) -> Vec<&'static str> {
        self.files.iter().map(|file| file.path).collect()
    }
}

const NERV_EVA_FILES: &[BundledFile] = &[
    BundledFile::new("manifest.json", include_bytes!("../../../themes/nerv-eva/manifest.json")),
    BundledFile::new("tokens.json", include_bytes!("../../../themes/nerv-eva/tokens.json")),
    BundledFile::new("LICENSE", include_bytes!("../../../themes/nerv-eva/LICENSE")),
    BundledFile::new("CREDITS.md", include_bytes!("../../../themes/nerv-eva/CREDITS.md")),
    BundledFile::new("assets/logo.txt", include_bytes!("../../../themes/nerv-eva/assets/logo.txt")),
    BundledFile::new(
        "assets/wallpaper.png",
        include_bytes!("../../../themes/nerv-eva/assets/wallpaper.png"),
    ),
    BundledFile::new(
        "shaders/live.glsl",
        include_bytes!("../../../themes/nerv-eva/shaders/live.glsl"),
    ),
    BundledFile::new(
        "shaders/live.hlsl",
        include_bytes!("../../../themes/nerv-eva/shaders/live.hlsl"),
    ),
    BundledFile::new(
        "previews/preview.png",
        include_bytes!("../../../themes/nerv-eva/previews/preview.png"),
    ),
];

const NERV_EVA: BundledTheme = BundledTheme {
    id: "nerv-eva",
    version: "2.0.0",
    name: "NERV UNIT-02",
    tagline: "特務機関 NERV 弐号機 · 战术蓝图暗红橙",
    files: NERV_EVA_FILES,
};

const BUNDLED: &[BundledTheme] = &[NERV_EVA];

/// All themes embedded in this build.
pub fn bundled_themes() -> &'static [BundledTheme] {
    BUNDLED
}

/// Looks up an embedded theme by ID.
pub fn bundled_theme(id: &str) -> Option<&'static BundledTheme> {
    BUNDLED.iter().find(|theme| theme.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_theme_has_the_required_files() {
        for theme in bundled_themes() {
            for required in [
                "manifest.json",
                "tokens.json",
                "LICENSE",
                "CREDITS.md",
                "assets/logo.txt",
                "assets/wallpaper.png",
                "shaders/live.glsl",
                "shaders/live.hlsl",
            ] {
                assert!(theme.file(required).is_some(), "{} missing {required}", theme.id);
            }
            let manifest: kiseki_schema::ThemeManifest =
                serde_json::from_str(theme.manifest_json()).unwrap();
            assert_eq!(manifest.id.as_str(), theme.id);
            assert_eq!(manifest.version, theme.version);
            let _tokens: kiseki_schema::ThemeTokens =
                serde_json::from_str(theme.tokens_json()).unwrap();
            assert!(theme.glsl().unwrap().contains("mainImage"));
            assert!(theme.hlsl().unwrap().contains("cbuffer PixelShaderSettings"));
        }
    }

    #[test]
    fn lookup_is_stable() {
        assert!(bundled_theme("nerv-eva").is_some());
        assert!(bundled_theme("does-not-exist").is_none());
        assert_eq!(bundled_themes().len(), 1);
    }

    #[test]
    fn paths_cover_the_whole_pack() {
        let paths = bundled_theme("nerv-eva").unwrap().paths();
        assert!(paths.contains(&"LICENSE"));
        assert!(paths.contains(&"assets/wallpaper.png"));
        assert!(!paths.iter().any(|path| path.ends_with(".lua")));
    }
}
