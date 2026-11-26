//! Host directory layout (RFC-0001 §4).
//!
//! Every path the engine touches is derived here so tests can point the whole
//! system at a throwaway directory with a single environment variable.

use std::path::{Path, PathBuf};

use kiseki_types::OsKind;

/// The three roots Kiseki owns.
///
/// * `config` — user preferences, `state.json`, `state.gen`, `generated/`.
/// * `cache` — font scan caches and downloads.
/// * `data` — installed themes and the transaction journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KisekiPaths {
    config: PathBuf,
    cache: PathBuf,
    data: PathBuf,
}

impl KisekiPaths {
    /// Builds the layout from explicit roots.
    pub fn new(config: impl Into<PathBuf>, cache: impl Into<PathBuf>, data: impl Into<PathBuf>) -> Self {
        Self {
            config: config.into(),
            cache: cache.into(),
            data: data.into(),
        }
    }

    /// Resolves the platform layout from environment variables.
    ///
    /// `KISEKI_HOME` wins over everything and nests `cache/` and `data/` inside
    /// it, which is what makes hermetic tests and portable installs possible.
    pub fn from_env(os: OsKind, env: &dyn Fn(&str) -> Option<String>) -> Self {
        if let Some(home) = env("KISEKI_HOME").filter(|value| !value.trim().is_empty()) {
            let home = PathBuf::from(home);
            return Self::new(home.clone(), home.join("cache"), home.join("data"));
        }

        let home_dir = env("HOME")
            .or_else(|| env("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        match os {
            OsKind::Windows => {
                let config = env("USERPROFILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home_dir.clone())
                    .join(".config")
                    .join("kiseki");
                let local = env("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home_dir.join("AppData").join("Local"))
                    .join("kiseki");
                Self::new(config, local.join("cache"), local.join("data"))
            }
            OsKind::MacOs | OsKind::Linux => {
                let config_home = env("XDG_CONFIG_HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home_dir.join(".config"));
                let cache_home = env("XDG_CACHE_HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home_dir.join(".cache"));
                let data_home = env("XDG_DATA_HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home_dir.join(".local").join("share"));
                Self::new(
                    config_home.join("kiseki"),
                    cache_home.join("kiseki"),
                    data_home.join("kiseki"),
                )
            }
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn state_file(&self) -> PathBuf {
        self.config.join("state.json")
    }

    /// The single-line integer every shell hook reads (RFC-0001 §5).
    pub fn gen_file(&self) -> PathBuf {
        self.config.join("state.gen")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.config.join(".lock")
    }

    pub fn generated_dir(&self) -> PathBuf {
        self.config.join("generated")
    }

    pub fn shaders_dir(&self) -> PathBuf {
        self.generated_dir().join("shaders")
    }

    pub fn preview_dir(&self) -> PathBuf {
        self.config.join("preview")
    }

    pub fn preview_recovery_file(&self) -> PathBuf {
        self.preview_dir().join("recovery.json")
    }

    pub fn preview_lock_file(&self) -> PathBuf {
        self.preview_dir().join("active-preview.lock")
    }

    pub fn journal_dir(&self) -> PathBuf {
        self.data.join("journal")
    }

    pub fn official_themes_dir(&self) -> PathBuf {
        self.data.join("themes").join("official")
    }

    pub fn community_themes_dir(&self) -> PathBuf {
        self.data.join("themes").join("community")
    }

    pub fn font_cache_file(&self) -> PathBuf {
        self.cache.join("fonts.json")
    }

    /// Path of a generated artefact by its slash-separated relative name.
    pub fn generated(&self, relative: &str) -> PathBuf {
        let mut path = self.generated_dir();
        for segment in relative.split('/').filter(|segment| !segment.is_empty()) {
            path.push(segment);
        }
        path
    }

    /// True when `candidate` lives under one of the Kiseki-owned roots.
    pub fn owns(&self, candidate: &Path) -> bool {
        [&self.config, &self.cache, &self.data]
            .into_iter()
            .any(|root| candidate.starts_with(root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env_map(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: BTreeMap<String, String> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn kiseki_home_wins_and_nests_every_root() {
        let env = env_map(&[
            ("KISEKI_HOME", "/tmp/kiseki-home"),
            ("HOME", "/home/ignored"),
            ("XDG_CONFIG_HOME", "/home/ignored/config"),
        ]);
        let paths = KisekiPaths::from_env(OsKind::Linux, &env);
        assert_eq!(paths.config_dir(), Path::new("/tmp/kiseki-home"));
        assert_eq!(paths.cache_dir(), Path::new("/tmp/kiseki-home/cache"));
        assert_eq!(paths.data_dir(), Path::new("/tmp/kiseki-home/data"));
        assert_eq!(
            paths.state_file(),
            Path::new("/tmp/kiseki-home/state.json")
        );
        assert_eq!(paths.generated_dir(), Path::new("/tmp/kiseki-home/generated"));
        assert!(paths.owns(Path::new("/tmp/kiseki-home/generated/env.zsh")));
        assert!(!paths.owns(Path::new("/tmp/other/env.zsh")));
    }

    #[test]
    fn xdg_variables_are_honoured() {
        let env = env_map(&[
            ("HOME", "/home/zz"),
            ("XDG_CONFIG_HOME", "/home/zz/.config-custom"),
            ("XDG_CACHE_HOME", "/home/zz/.cache-custom"),
            ("XDG_DATA_HOME", "/home/zz/.data-custom"),
        ]);
        let paths = KisekiPaths::from_env(OsKind::Linux, &env);
        assert_eq!(paths.config_dir(), Path::new("/home/zz/.config-custom/kiseki"));
        assert_eq!(paths.cache_dir(), Path::new("/home/zz/.cache-custom/kiseki"));
        assert_eq!(paths.data_dir(), Path::new("/home/zz/.data-custom/kiseki"));
    }

    #[test]
    fn xdg_defaults_apply_when_unset() {
        let env = env_map(&[("HOME", "/home/zz")]);
        let paths = KisekiPaths::from_env(OsKind::MacOs, &env);
        assert_eq!(paths.config_dir(), Path::new("/home/zz/.config/kiseki"));
        assert_eq!(paths.cache_dir(), Path::new("/home/zz/.cache/kiseki"));
        assert_eq!(
            paths.data_dir(),
            Path::new("/home/zz/.local/share/kiseki")
        );
    }

    #[test]
    fn windows_uses_userprofile_and_localappdata() {
        let env = env_map(&[
            ("USERPROFILE", "C:\\Users\\zz"),
            ("LOCALAPPDATA", "C:\\Users\\zz\\AppData\\Local"),
        ]);
        let paths = KisekiPaths::from_env(OsKind::Windows, &env);
        assert_eq!(paths.config_dir().file_name().unwrap(), "kiseki");
        assert_eq!(paths.cache_dir().file_name().unwrap(), "cache");
        assert_eq!(paths.cache_dir().parent().unwrap().file_name().unwrap(), "kiseki");
        assert_eq!(paths.data_dir().file_name().unwrap(), "data");
        assert_eq!(paths.data_dir().parent().unwrap().file_name().unwrap(), "kiseki");
    }

    #[test]
    fn generated_paths_are_relative_and_composable() {
        let paths = KisekiPaths::new("/c", "/k", "/d");
        assert_eq!(
            paths.generated("shaders/active.glsl"),
            Path::new("/c/generated/shaders/active.glsl")
        );
        assert_eq!(paths.shaders_dir(), Path::new("/c/generated/shaders"));
        assert_eq!(paths.journal_dir(), Path::new("/d/journal"));
        assert_eq!(
            paths.official_themes_dir(),
            Path::new("/d/themes/official")
        );
    }
}
