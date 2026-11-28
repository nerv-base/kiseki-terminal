//! Themes embedded in the binary so first-run is fully offline (ADR-013).
//!
//! All four official themes (`nerv-eva`, `kyoto-sakura`, `shinkai-rain`, and
//! `starlight-8bit`) are compiled into the binary and materialised into
//! `<kiseki_data>/themes/official/` on first use so absolute asset paths and
//! `kiseki theme validate` work identically to a community install.

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
    version: "2.0.3",
    name: "NERV UNIT-02",
    tagline: "特務機関 NERV 弐号機 · 战术蓝图暗红橙",
    files: NERV_EVA_FILES,
};

const KYOTO_SAKURA_FILES: &[BundledFile] = &[
    BundledFile::new("manifest.json", include_bytes!("../../../themes/kyoto-sakura/manifest.json")),
    BundledFile::new("tokens.json", include_bytes!("../../../themes/kyoto-sakura/tokens.json")),
    BundledFile::new("LICENSE", include_bytes!("../../../themes/kyoto-sakura/LICENSE")),
    BundledFile::new("CREDITS.md", include_bytes!("../../../themes/kyoto-sakura/CREDITS.md")),
    BundledFile::new("assets/logo.txt", include_bytes!("../../../themes/kyoto-sakura/assets/logo.txt")),
    BundledFile::new(
        "assets/wallpaper.jpg",
        include_bytes!("../../../themes/kyoto-sakura/assets/wallpaper.jpg"),
    ),
    BundledFile::new(
        "shaders/live.glsl",
        include_bytes!("../../../themes/kyoto-sakura/shaders/live.glsl"),
    ),
    BundledFile::new(
        "shaders/live.hlsl",
        include_bytes!("../../../themes/kyoto-sakura/shaders/live.hlsl"),
    ),
    BundledFile::new(
        "previews/preview.png",
        include_bytes!("../../../themes/kyoto-sakura/previews/preview.png"),
    ),
];

const KYOTO_SAKURA: BundledTheme = BundledTheme {
    id: "kyoto-sakura",
    version: "2.0.3",
    name: "Kyoto Sakura",
    tagline: "夜樱绘卷 · 和风月夜神社与垂枝樱",
    files: KYOTO_SAKURA_FILES,
};

const SHINKAI_RAIN_FILES: &[BundledFile] = &[
    BundledFile::new("manifest.json", include_bytes!("../../../themes/shinkai-rain/manifest.json")),
    BundledFile::new("tokens.json", include_bytes!("../../../themes/shinkai-rain/tokens.json")),
    BundledFile::new("LICENSE", include_bytes!("../../../themes/shinkai-rain/LICENSE")),
    BundledFile::new("CREDITS.md", include_bytes!("../../../themes/shinkai-rain/CREDITS.md")),
    BundledFile::new("assets/logo.txt", include_bytes!("../../../themes/shinkai-rain/assets/logo.txt")),
    BundledFile::new(
        "assets/wallpaper.jpg",
        include_bytes!("../../../themes/shinkai-rain/assets/wallpaper.jpg"),
    ),
    BundledFile::new(
        "shaders/live.glsl",
        include_bytes!("../../../themes/shinkai-rain/shaders/live.glsl"),
    ),
    BundledFile::new(
        "shaders/live.hlsl",
        include_bytes!("../../../themes/shinkai-rain/shaders/live.hlsl"),
    ),
    BundledFile::new(
        "previews/preview.png",
        include_bytes!("../../../themes/shinkai-rain/previews/preview.png"),
    ),
];

const SHINKAI_RAIN: BundledTheme = BundledTheme {
    id: "shinkai-rain",
    version: "2.0.3",
    name: "Shinkai Rain",
    tagline: "言叶之雨 · 新海诚东京雨夜电影感",
    files: SHINKAI_RAIN_FILES,
};

const STARLIGHT_8BIT_FILES: &[BundledFile] = &[
    BundledFile::new("manifest.json", include_bytes!("../../../themes/starlight-8bit/manifest.json")),
    BundledFile::new("tokens.json", include_bytes!("../../../themes/starlight-8bit/tokens.json")),
    BundledFile::new("LICENSE", include_bytes!("../../../themes/starlight-8bit/LICENSE")),
    BundledFile::new("CREDITS.md", include_bytes!("../../../themes/starlight-8bit/CREDITS.md")),
    BundledFile::new(
        "assets/logo.txt",
        include_bytes!("../../../themes/starlight-8bit/assets/logo.txt"),
    ),
    BundledFile::new(
        "assets/wallpaper.png",
        include_bytes!("../../../themes/starlight-8bit/assets/wallpaper.png"),
    ),
    BundledFile::new(
        "shaders/live.glsl",
        include_bytes!("../../../themes/starlight-8bit/shaders/live.glsl"),
    ),
    BundledFile::new(
        "shaders/live.hlsl",
        include_bytes!("../../../themes/starlight-8bit/shaders/live.hlsl"),
    ),
    BundledFile::new(
        "previews/preview.png",
        include_bytes!("../../../themes/starlight-8bit/previews/preview.png"),
    ),
];

const STARLIGHT_8BIT: BundledTheme = BundledTheme {
    id: "starlight-8bit",
    version: "2.0.3",
    name: "Starlight 8-Bit",
    tagline: "星屑 8-Bit · 复古二次元像素夜空",
    files: STARLIGHT_8BIT_FILES,
};

const BUNDLED: &[BundledTheme] = &[NERV_EVA, KYOTO_SAKURA, SHINKAI_RAIN, STARLIGHT_8BIT];

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
                "shaders/live.glsl",
                "shaders/live.hlsl",
            ] {
                assert!(theme.file(required).is_some(), "{} missing {required}", theme.id);
            }
            let manifest: kiseki_schema::ThemeManifest =
                serde_json::from_str(theme.manifest_json()).unwrap();
            assert_eq!(manifest.id.as_str(), theme.id);
            assert_eq!(manifest.version, theme.version);
            let tokens: kiseki_schema::ThemeTokens =
                serde_json::from_str(theme.tokens_json()).unwrap();
            let wallpaper_path = tokens
                .atmosphere
                .as_ref()
                .and_then(|atmosphere| atmosphere.wallpaper.as_ref())
                .map(|wallpaper| wallpaper.path.as_str())
                .expect("bundled theme declares wallpaper path");
            assert!(
                theme.file(wallpaper_path).is_some(),
                "{} missing declared wallpaper {wallpaper_path}",
                theme.id
            );
            assert!(theme.glsl().unwrap().contains("mainImage"));
            assert!(theme.hlsl().unwrap().contains("cbuffer PixelShaderSettings"));
        }
    }

    #[test]
    fn lookup_is_stable() {
        for id in ["nerv-eva", "kyoto-sakura", "shinkai-rain", "starlight-8bit"] {
            assert!(bundled_theme(id).is_some(), "missing bundled theme {id}");
        }
        assert!(bundled_theme("does-not-exist").is_none());
        assert_eq!(bundled_themes().len(), 4);
    }

    #[test]
    fn every_bundled_theme_passes_full_pack_validation() {
        use kiseki_platform::Platform;

        let dir = tempfile::tempdir().unwrap();
        let platform = kiseki_platform::MockPlatform::new(dir.path());
        let library = crate::ThemeLibrary::open(&platform, &[]);
        library.materialize_official(&platform).unwrap();
        for theme in bundled_themes() {
            let root = platform.paths().official_themes_dir().join(theme.id);
            let validation = crate::validate_theme_dir(&root).unwrap();
            let fatals: Vec<String> = validation
                .diagnostics
                .iter()
                .filter(|d| d.is_fatal())
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect();
            assert!(
                validation.is_valid(),
                "bundled theme {} failed validation: {fatals:#?}",
                theme.id
            );
            let non_advisory_warnings: Vec<String> = validation
                .diagnostics
                .iter()
                .filter(|d| !d.is_fatal() && d.code != "wcag.contrast.recommended")
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect();
            assert!(
                non_advisory_warnings.is_empty(),
                "bundled theme {} emitted unexpected warnings: {non_advisory_warnings:#?}",
                theme.id
            );
        }
    }

    #[test]
    fn paths_cover_the_whole_pack() {
        let paths = bundled_theme("nerv-eva").unwrap().paths();
        assert!(paths.contains(&"LICENSE"));
        assert!(paths.contains(&"assets/wallpaper.png"));
        assert!(!paths.iter().any(|path| path.ends_with(".lua")));
    }
}
