//! Symlink-preserving filesystem primitives (RFC-0003 §6.1, ADR-008).
//!
//! The single most important invariant in this module: **a writable symlink is
//! never replaced by a regular file**. Dotfile managers (Stow, chezmoi, yadm,
//! Home Manager) rely on the link staying a link, so we resolve the target and
//! perform the atomic write there.
//!
//! Read-only targets (Nix store, system directories, `chmod -w` files) are not
//! an error: they return [`WriteOutcome::SkippedReadOnly`] so the transaction
//! engine can degrade that adapter to `render-only` and keep the rest healthy.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use kiseki_types::{KisekiError, Result};

/// How a host path is currently backed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathKind {
    /// Nothing exists at the path yet.
    Missing,
    /// A regular, writable file.
    Regular,
    /// A symlink pointing at a writable file. The payload is the resolved
    /// target that must receive the write.
    WritableSymlink(PathBuf),
    /// A symlink pointing at a read-only target.
    ReadOnlySymlink(PathBuf),
    /// A regular file without write permission.
    ReadOnlyFile,
}

impl PathKind {
    /// True when the path is a symlink in any form.
    pub fn is_symlink(&self) -> bool {
        matches!(self, Self::WritableSymlink(_) | Self::ReadOnlySymlink(_))
    }

    /// True when the engine may write through the path.
    pub fn is_writable(&self) -> bool {
        matches!(self, Self::Missing | Self::Regular | Self::WritableSymlink(_))
    }

    /// The symlink target, when the path is a link.
    pub fn target(&self) -> Option<&Path> {
        match self {
            Self::WritableSymlink(target) | Self::ReadOnlySymlink(target) => Some(target),
            _ => None,
        }
    }

    /// Stable label used by `status`/`doctor` and JSON output.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Regular => "regular",
            Self::WritableSymlink(_) => "writable-symlink",
            Self::ReadOnlySymlink(_) => "readonly-symlink",
            Self::ReadOnlyFile => "readonly-file",
        }
    }
}

/// Result of a symlink-aware write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The payload reached disk.
    Written {
        /// The file that actually received the bytes (the resolved target for
        /// symlinks, the original path otherwise).
        path: PathBuf,
        /// True when the file did not exist before.
        created: bool,
    },
    /// The target is read-only; the caller should fall back to render-only.
    SkippedReadOnly {
        path: PathBuf,
        reason: String,
    },
}

impl WriteOutcome {
    pub fn is_written(&self) -> bool {
        matches!(self, Self::Written { .. })
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Written { path, .. } | Self::SkippedReadOnly { path, .. } => path,
        }
    }
}

/// Inspects a path without following symlinks for the *classification* while
/// still resolving the target for the write.
pub fn inspect_path(path: &Path) -> PathKind {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return PathKind::Missing;
    };

    if metadata.file_type().is_symlink() {
        let target = resolve_symlink(path);
        if target_is_writable(&target) && !is_protected_prefix(path) {
            PathKind::WritableSymlink(target)
        } else {
            PathKind::ReadOnlySymlink(target)
        }
    } else if metadata.permissions().readonly() || is_protected_prefix(path) || !target_is_writable(path) {
        PathKind::ReadOnlyFile
    } else {
        PathKind::Regular
    }
}

/// Resolves a symlink, following chains, and falls back to the raw link text.
pub fn resolve_symlink(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }
    if let Ok(raw) = fs::read_link(path) {
        if raw.is_absolute() {
            return raw;
        }
        return path.parent().map(|parent| parent.join(&raw)).unwrap_or(raw);
    }
    path.to_path_buf()
}

/// Directories whose contents must never be rewritten.
///
/// Nix store paths are read-only by construction; the system prefixes are
/// included so a hostile theme cannot trick the engine into writing into
/// `/System` or `/usr`.
pub fn is_protected_prefix(path: &Path) -> bool {
    const PREFIXES: &[&str] = &["/nix/store", "/System", "/usr", "/bin", "/sbin", "/etc", "/Library"];
    let text = path.to_string_lossy();
    PREFIXES
        .iter()
        .any(|prefix| text == *prefix || text.starts_with(&format!("{prefix}/")))
}

fn target_is_writable(path: &Path) -> bool {
    if is_protected_prefix(path) {
        return false;
    }
    match fs::metadata(path) {
        Ok(metadata) => {
            if metadata.permissions().readonly() {
                return false;
            }
            if metadata.is_dir() {
                // A directory is writable when we can create a probe file in it.
                return tempfile::NamedTempFile::new_in(path).is_ok();
            }
            // Opening for write without truncation proves the effective UID may
            // write the file; it has no visible side effect.
            fs::OpenOptions::new().write(true).open(path).is_ok()
        }
        Err(_) => {
            // Missing file: writability is a property of the parent directory.
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            let Ok(metadata) = fs::metadata(parent) else {
                return false;
            };
            if metadata.permissions().readonly() {
                return false;
            }
            if is_protected_prefix(parent) {
                return false;
            }
            // Creating and immediately removing a probe file is the only
            // portable way to test directory write permission.
            tempfile::NamedTempFile::new_in(parent).is_ok()
        }
    }
}

/// Reads a UTF-8 file, returning `None` when it does not exist.
pub fn read_to_string(path: &Path) -> Result<Option<String>> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|error| KisekiError::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, error))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(KisekiError::io(path, error)),
    }
}

/// Reads raw bytes, returning `None` when the file does not exist.
pub fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(KisekiError::io(path, error)),
    }
}

/// Atomically replaces `path` with `contents`.
///
/// Writes a sibling temp file, fsyncs it, then renames over the destination so
/// a crash can never leave a half-written config behind.
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| KisekiError::io(parent, error))?;

    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| KisekiError::io(parent, error))?;
    temp.write_all(contents)
        .map_err(|error| KisekiError::io(temp.path(), error))?;
    temp.as_file()
        .sync_all()
        .map_err(|error| KisekiError::io(temp.path(), error))?;

    match temp.persist(path) {
        Ok(_) => {}
        Err(error) => {
            // Windows refuses to rename over an existing file; removing the
            // destination first is still crash-safe because the temp file is
            // fully written and fsynced.
            let io_error = error.error;
            if path.exists() {
                fs::remove_file(path).map_err(|remove_error| KisekiError::io(path, remove_error))?;
                error.file.persist(path).map_err(|persist| {
                    KisekiError::io(path, std::io::Error::other(persist.error.to_string()))
                })?;
            } else {
                return Err(KisekiError::io(path, io_error));
            }
        }
    }

    #[cfg(unix)]
    {
        // Persist the directory entry too, otherwise a power loss can forget the
        // rename even though the data blocks are on disk.
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }

    Ok(())
}

/// Writes through symlinks instead of replacing them.
pub fn write_preserving_symlinks(path: &Path, contents: &[u8]) -> Result<WriteOutcome> {
    let kind = inspect_path(path);
    match kind {
        PathKind::Missing | PathKind::Regular => {
            let created = !path.exists();
            atomic_write(path, contents)?;
            Ok(WriteOutcome::Written {
                path: path.to_path_buf(),
                created,
            })
        }
        PathKind::WritableSymlink(target) => {
            let created = !target.exists();
            atomic_write(&target, contents)?;
            // The link itself is untouched: that is the whole point.
            Ok(WriteOutcome::Written {
                path: target,
                created,
            })
        }
        PathKind::ReadOnlySymlink(target) => Ok(WriteOutcome::SkippedReadOnly {
            path: target.clone(),
            reason: format!(
                "{} is a read-only symlink (target {}); adapter degraded to render-only",
                path.display(),
                target.display()
            ),
        }),
        PathKind::ReadOnlyFile => Ok(WriteOutcome::SkippedReadOnly {
            path: path.to_path_buf(),
            reason: format!(
                "{} is read-only; adapter degraded to render-only",
                path.display()
            ),
        }),
    }
}

/// Deletes a file if it exists.
pub fn remove_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(KisekiError::io(path, error)),
    }
}

/// Lists regular files directly under `dir`, sorted for deterministic output.
pub fn list_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(KisekiError::io(dir, error)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| KisekiError::io(dir, error))?;
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symlink(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, link).unwrap();
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn missing_path_is_writable() {
        let dir = tempdir();
        let path = dir.path().join("new.conf");
        assert_eq!(inspect_path(&path), PathKind::Missing);
        let outcome = write_preserving_symlinks(&path, b"hello\n").unwrap();
        assert!(outcome.is_written());
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");
    }

    #[test]
    fn regular_file_is_replaced_atomically() {
        let dir = tempdir();
        let path = dir.path().join("config");
        fs::write(&path, "old").unwrap();
        assert_eq!(inspect_path(&path), PathKind::Regular);
        write_preserving_symlinks(&path, b"new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn writable_symlink_is_preserved_and_target_written() {
        let dir = tempdir();
        let target = dir.path().join("dotfiles").join("zshrc");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "original").unwrap();
        let link = dir.path().join("zshrc");
        symlink(&target, &link);

        let kind = inspect_path(&link);
        assert!(matches!(kind, PathKind::WritableSymlink(_)));
        assert!(kind.is_symlink());

        let outcome = write_preserving_symlinks(&link, b"themed").unwrap();
        assert!(outcome.is_written());
        assert_eq!(outcome.path(), fs::canonicalize(&target).unwrap().as_path());

        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "themed");
        assert_eq!(fs::read_to_string(&link).unwrap(), "themed");
    }

    #[test]
    fn read_only_file_is_skipped_not_an_error() {
        let dir = tempdir();
        let path = dir.path().join("locked");
        fs::write(&path, "locked").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();

        assert_eq!(inspect_path(&path), PathKind::ReadOnlyFile);
        let outcome = write_preserving_symlinks(&path, b"nope").unwrap();
        assert!(!outcome.is_written());
        assert!(matches!(outcome, WriteOutcome::SkippedReadOnly { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), "locked");
    }

    #[test]
    fn symlink_to_read_only_file_is_skipped() {
        let dir = tempdir();
        let target = dir.path().join("nix-store-config");
        fs::write(&target, "readonly").unwrap();
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&target, permissions).unwrap();
        let link = dir.path().join("link");
        symlink(&target, &link);

        let kind = inspect_path(&link);
        assert!(matches!(kind, PathKind::ReadOnlySymlink(_)), "{kind:?}");
        let outcome = write_preserving_symlinks(&link, b"nope").unwrap();
        assert!(matches!(outcome, WriteOutcome::SkippedReadOnly { .. }));
        assert_eq!(fs::read_to_string(&target).unwrap(), "readonly");
    }

    #[test]
    fn protected_prefixes_are_never_writable() {
        assert!(is_protected_prefix(Path::new("/nix/store/abc-config")));
        assert!(is_protected_prefix(Path::new("/System/Library/x")));
        assert!(is_protected_prefix(Path::new("/usr/local/etc/x")));
        assert!(!is_protected_prefix(Path::new("/home/zz/.config/kiseki")));
        assert!(!is_protected_prefix(Path::new("/Users/zz/usr-thing")));
    }

    #[test]
    fn missing_symlink_target_stays_a_symlink_and_gets_created() {
        let dir = tempdir();
        let target = dir.path().join("missing-target");
        let link = dir.path().join("link");
        symlink(&target, &link);
        let outcome = write_preserving_symlinks(&link, b"created").unwrap();
        assert!(outcome.is_written());
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "created");
    }

    #[test]
    fn read_and_list_helpers_are_quiet_on_missing_paths() {
        let dir = tempdir();
        assert_eq!(read_to_string(&dir.path().join("nope")).unwrap(), None);
        assert_eq!(read_bytes(&dir.path().join("nope")).unwrap(), None);
        assert!(list_dir(&dir.path().join("nope")).unwrap().is_empty());
        remove_file(&dir.path().join("nope")).unwrap();
    }
}
