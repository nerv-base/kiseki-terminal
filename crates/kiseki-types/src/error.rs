//! Workspace-wide error type.
//!
//! `kiseki-types` cannot depend on the higher-level crates, so leaf crates
//! either reuse these variants directly or wrap them with `#[from]`.

use std::path::PathBuf;

use thiserror::Error;

/// Convenience alias used across the workspace.
pub type Result<T, E = KisekiError> = std::result::Result<T, E>;

/// Every recoverable failure the engine can report.
#[derive(Debug, Error)]
pub enum KisekiError {
    #[error("invalid theme id {0:?}: expected lowercase kebab-case, 2-48 chars")]
    InvalidThemeId(String),

    #[error("invalid color {0:?}: expected #RRGGBB")]
    InvalidColor(String),

    #[error("invalid rendering profile {0:?}: expected static|eco|balanced|ultra|accessibility")]
    InvalidProfile(String),

    #[error("invalid appearance {0:?}: expected dark|light")]
    InvalidAppearance(String),

    #[error("unsafe relative path {path:?}: {reason}")]
    UnsafePath { path: String, reason: String },

    #[error("theme not found: {0}")]
    ThemeNotFound(String),

    #[error("unsupported file type {path:?}: {reason}")]
    UnsupportedFile { path: String, reason: String },

    #[error("schema validation failed: {0}")]
    Schema(String),

    #[error("security violation: {0}")]
    Security(String),

    #[error("shader error: {0}")]
    Shader(String),

    #[error("compiler error: {0}")]
    Compiler(String),

    #[error("adapter {adapter} failed: {message}")]
    Adapter { adapter: String, message: String },

    #[error("transaction {tx_id} failed: {message}")]
    Transaction { tx_id: String, message: String },

    #[error("another kiseki process holds the lock at {0}")]
    Locked(PathBuf),

    #[error("state error: {0}")]
    State(String),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("json error: {0}")]
    Json(String),

    #[error("{0}")]
    Other(String),
}

impl KisekiError {
    pub fn invalid_theme_id(raw: impl Into<String>) -> Self {
        Self::InvalidThemeId(raw.into())
    }

    pub fn invalid_color(raw: impl Into<String>) -> Self {
        Self::InvalidColor(raw.into())
    }

    pub fn invalid_profile(raw: impl Into<String>) -> Self {
        Self::InvalidProfile(raw.into())
    }

    pub fn invalid_appearance(raw: impl Into<String>) -> Self {
        Self::InvalidAppearance(raw.into())
    }

    pub fn unsafe_path(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::UnsafePath {
            path: path.into(),
            reason: reason.into(),
        }
    }

    pub fn unsupported_file(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::UnsupportedFile {
            path: path.into(),
            reason: reason.into(),
        }
    }

    pub fn schema(message: impl Into<String>) -> Self {
        Self::Schema(message.into())
    }

    pub fn security(message: impl Into<String>) -> Self {
        Self::Security(message.into())
    }

    pub fn shader(message: impl Into<String>) -> Self {
        Self::Shader(message.into())
    }

    pub fn compiler(message: impl Into<String>) -> Self {
        Self::Compiler(message.into())
    }

    pub fn adapter(adapter: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Adapter {
            adapter: adapter.into(),
            message: message.into(),
        }
    }

    pub fn transaction(tx_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Transaction {
            tx_id: tx_id.into(),
            message: message.into(),
        }
    }

    pub fn state(message: impl Into<String>) -> Self {
        Self::State(message.into())
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }

    /// Wraps an `io::Error` with the path that produced it.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

impl From<serde_json::Error> for KisekiError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.to_string())
    }
}

impl From<std::io::Error> for KisekiError {
    fn from(error: std::io::Error) -> Self {
        Self::Other(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_actionable() {
        let error = KisekiError::invalid_color("#FFF");
        assert_eq!(error.to_string(), "invalid color \"#FFF\": expected #RRGGBB");

        let error = KisekiError::unsafe_path("../etc/passwd", "contains '..'");
        assert!(error.to_string().contains("unsafe relative path"));
    }

    #[test]
    fn io_errors_keep_the_path() {
        let error = KisekiError::io("/tmp/x", std::io::Error::other("boom"));
        assert!(error.to_string().contains("/tmp/x"));
        assert!(error.to_string().contains("boom"));
    }
}
