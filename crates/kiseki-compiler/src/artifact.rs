//! In-memory artefact map produced by the compiler.
//!
//! Keys are slash-separated paths relative to `<kiseki_config>/generated/`.
//! The compiler never touches the filesystem: it returns this map and the
//! transaction engine decides what to write, journal and roll back.

use std::collections::BTreeMap;

use kiseki_types::{KisekiError, Result};

/// A validated relative artefact path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactPath(String);

impl ArtifactPath {
    /// Validates a relative path (no absolute roots, no `..`, no backslashes).
    pub fn new(raw: impl Into<String>) -> Result<Self> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(KisekiError::unsafe_path("", "path must not be empty"));
        }
        if raw.starts_with('/') {
            return Err(KisekiError::unsafe_path(&raw, "path must be relative"));
        }
        if raw.contains('\\') {
            return Err(KisekiError::unsafe_path(&raw, "path must use '/' separators"));
        }
        if raw.split('/').any(|segment| segment == ".." || segment == "." || segment.is_empty()) {
            return Err(KisekiError::unsafe_path(
                &raw,
                "path must not contain '.', '..' or empty segments",
            ));
        }
        if !raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/'))
        {
            return Err(KisekiError::unsafe_path(
                &raw,
                "path contains characters outside [A-Za-z0-9._-]",
            ));
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for ArtifactPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ArtifactPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Every text artefact one compiled theme produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactMap {
    entries: BTreeMap<ArtifactPath, String>,
}

impl ArtifactMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<String>, content: impl Into<String>) -> Result<()> {
        self.entries.insert(ArtifactPath::new(path)?, content.into());
        Ok(())
    }

    pub fn get(&self, path: &str) -> Option<&str> {
        self.entries
            .get(&ArtifactPath::new(path).ok()?)
            .map(String::as_str)
    }

    pub fn contains(&self, path: &str) -> bool {
        self.get(path).is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Deterministic iteration order (sorted by path).
    pub fn iter(&self) -> impl Iterator<Item = (&ArtifactPath, &String)> {
        self.entries.iter()
    }

    pub fn paths(&self) -> Vec<&str> {
        self.entries.keys().map(ArtifactPath::as_str).collect()
    }

    /// Renders a compact JSON description, used by `--json` output and tests.
    pub fn to_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (path, content) in &self.entries {
            map.insert(
                path.as_str().to_string(),
                serde_json::Value::String(content.clone()),
            );
        }
        serde_json::Value::Object(map)
    }
}

/// Canonical artefact names.
pub mod names {
    pub const GHOSTTY_CONF: &str = "ghostty.conf";
    pub const KITTY_CONF: &str = "kitty.conf";
    pub const WEZTERM_LUA: &str = "wezterm.lua";
    pub const ITERM2_DYNAMIC_PROFILE: &str = "iterm2-dynamic-profile.json";
    pub const ALACRITTY_TOML: &str = "alacritty.toml";
    pub const APPLE_TERMINAL_SCRIPT: &str = "apple-terminal.applescript";
    pub const ENV_ZSH: &str = "env.zsh";
    pub const ENV_PS1: &str = "env.ps1";
    pub const ENV_BASH: &str = "env.bash";
    pub const ENV_FISH: &str = "env.fish";
    pub const PROMPT_OMP: &str = "prompt.omp.json";
    pub const PROMPT_OMP_PALETTE: &str = "prompt.omp.palette.json";
    pub const STARSHIP_PALETTE: &str = "starship-palette.toml";
    pub const FASTFETCH: &str = "fastfetch.jsonc";
    pub const WT_VISUAL: &str = "wt-visual.json";
    pub const SHADER_GLSL: &str = "shaders/active.glsl";
    pub const SHADER_HLSL: &str = "shaders/active.hlsl";
    // Tier 2 ecosystem artefacts (RFC-0003 §4)
    pub const NVIM_LUA: &str = "nvim/kiseki.lua";
    pub const VIM_VIM: &str = "vim/kiseki.vim";
    pub const HELIX_TOML: &str = "helix/kiseki.toml";
    pub const TMUX_CONF: &str = "tmux.conf";
    pub const ZELLIJ_KDL: &str = "zellij.kdl";
    pub const OPENCODE_JSON: &str = "opencode.json";
    pub const BAT_TMTHEME: &str = "bat/Kiseki.tmTheme";
    pub const DELTA_GITCONFIG: &str = "delta.gitconfig";
    pub const LAZYGIT_YML: &str = "lazygit.yml";
    pub const BTOP_THEME: &str = "btop/kiseki.theme";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_lookup_are_deterministic() {
        let mut map = ArtifactMap::new();
        map.insert("b.txt", "b").unwrap();
        map.insert("a.txt", "a").unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.paths(), vec!["a.txt", "b.txt"]);
        assert_eq!(map.get("a.txt"), Some("a"));
        assert!(map.contains("b.txt"));
        assert!(!map.contains("c.txt"));
        assert_eq!(map.to_json()["a.txt"], "a");
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        for bad in [
            "",
            "/abs",
            "../escape",
            "a/../b",
            "a//b",
            "a\\b",
            "a b",
            "a;b",
            "a\nb",
        ] {
            assert!(ArtifactPath::new(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn nested_paths_are_accepted() {
        let path = ArtifactPath::new("shaders/active.glsl").unwrap();
        assert_eq!(path.as_str(), "shaders/active.glsl");
        assert_eq!(path.to_string(), "shaders/active.glsl");
    }
}
