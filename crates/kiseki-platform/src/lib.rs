//! Cross-platform services: paths, symlink-safe IO, locking, environment
//! detection, font discovery and Ghostty helpers.
//!
//! This crate is the only place in the workspace allowed to touch the host.
//! Everything else receives a [`Platform`] so tests can substitute a
//! [`MockPlatform`] and assert on the exact side effects a command would cause.

pub mod deps;
pub mod env;
pub mod fonts;
pub mod fs;
pub mod ghostty;
pub mod lock;
pub mod paths;
pub mod platform;

pub use env::{capture_env, detect, detect_current, EnvSnapshot};
pub use fonts::{
    catalog, config_families, default_font_dirs, font_status, referenced_catalog_ids,
    resolve_font_id, FontCatalog, FontCatalogEntry, FontFaceInfo, FontFamilies, FontInventory,
    FontScanner, FontStatus,
};
pub use fs::{inspect_path, is_protected_prefix, PathKind, SyncMode, WriteOutcome};
pub use ghostty::{
    include_line, reload as reload_ghostty, validate_config as validate_ghostty_config,
    ReloadOutcome,
};
pub use lock::{Lock, DEFAULT_TIMEOUT};
pub use paths::{windows_documents_dirs, windows_known_documents_dir, KisekiPaths};
pub use platform::{
    CommandOutput, MockPlatform, Platform, RecordedCommand, SyncStats, SystemPlatform, LOCK_TIMEOUT,
};
