//! Theme packs, strict validation, the built-in official theme and v1 migration.
//!
//! This crate owns every read of untrusted theme bytes:
//! * embedded official packs (`bundled`),
//! * installed/community discovery (`library`),
//! * the security validator (`validate`),
//! * the reversible v1 → v2 migration planner (`migration`).

pub mod bundled;
pub mod library;
pub mod migration;
pub mod scaffold;
pub mod validate;

pub use bundled::{bundled_theme, bundled_themes, BundledFile, BundledTheme};
pub use library::{
    default_theme_id, file_count, is_theme_dir, official_root, LoadedTheme, ThemeEntry, ThemeLibrary, ThemeSource,
};
pub use migration::{
    detect_v1, has_read_only_targets, is_v1_shim, plan_migration, MigrationPlan, MigrationReport, MigrationStep,
    V1ConfigHook, V1Detection, V1Preferences, V1_BEGIN, V1_END, V1_LUA_BEGIN, V1_LUA_END,
    V1_SHIM_MARKER, V1_SHIM_NAMES,
};
pub use scaffold::{scaffold_theme_pack, title_case_id, ScaffoldOptions, ScaffoldResult};
pub use validate::{
    allowed_file, collect_files, install_from_dir, remove_installed, validate_theme_dir,
    validate_theme_dir_cached, InstalledTheme, ThemeFile, ThemeValidation, ValidationScope,
    MAX_BYTES, MAX_FILES, MAX_LOGO_BYTES,
};
