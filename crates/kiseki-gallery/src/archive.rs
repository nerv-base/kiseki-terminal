//! Safe `.tar.gz` archive extraction & packing with Zip-Slip, symlink,
//! file-count, and compression-bomb defenses (RFC-0006 §6.3).

use std::collections::BTreeSet;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use kiseki_pkg::{MAX_BYTES, MAX_FILES};
use kiseki_types::{KisekiError, Result};
use sha2::{Digest, Sha256};

/// Summary of a safely extracted theme archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractStats {
    pub sha256: String,
    pub compressed_bytes: u64,
    pub extracted_files: usize,
    pub extracted_bytes: u64,
}

/// Computes lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Safely extracts a `.tar.gz` theme archive into `dest_dir`.
///
/// Enforces:
/// 1. Compressed archive size <= `MAX_BYTES` (30 MiB)
/// 2. Optional SHA-256 digest match
/// 3. Rejection of symlinks, hardlinks, FIFOs, device nodes, absolute paths,
///    and `..` traversal
/// 4. Optional `strip_prefix` (or auto-stripping a single wrapper folder when
///    no root `manifest.json` exists) + optional `subpath`
/// 5. Extracted file count <= `MAX_FILES` (64) and total unpacked size <=
///    `MAX_BYTES` (30 MiB)
pub fn extract_tar_gz_safely(
    bytes: &[u8],
    expected_sha256: Option<&str>,
    strip_prefix: Option<&str>,
    subpath: Option<&str>,
    dest_dir: &Path,
) -> Result<ExtractStats> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(KisekiError::other(format!(
            "theme archive exceeds maximum allowed size ({} > {} bytes)",
            bytes.len(),
            MAX_BYTES
        )));
    }

    let actual_sha = sha256_hex(bytes);
    if let Some(expected) = expected_sha256 {
        let expected = expected.trim().to_ascii_lowercase();
        if actual_sha != expected {
            return Err(KisekiError::other(format!(
                "theme archive SHA-256 mismatch: expected {expected}, got {actual_sha}"
            )));
        }
    }

    let normalized_strip = strip_prefix
        .map(|s| s.trim().trim_matches('/').to_string())
        .filter(|s| !s.is_empty());
    let normalized_subpath = subpath
        .map(|s| s.trim().trim_matches('/').to_string())
        .filter(|s| !s.is_empty() && s != ".");

    // Pass 1: inspect paths to auto-detect a single wrapper directory when
    // `strip_prefix` was not supplied (standard GitHub & release tarballs).
    let effective_strip = match normalized_strip {
        Some(prefix) => Some(prefix),
        None => detect_single_wrapper_prefix(bytes)?,
    };

    std::fs::create_dir_all(dest_dir).map_err(|error| KisekiError::io(dest_dir, error))?;

    // Pass 2: validate every entry and extract regular files within caps.
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut extracted_files = 0usize;
    let mut extracted_bytes = 0u64;

    let entries = archive
        .entries()
        .map_err(|error| KisekiError::other(format!("invalid tar.gz archive: {error}")))?;

    for entry_result in entries {
        let mut entry = entry_result
            .map_err(|error| KisekiError::other(format!("corrupt tar entry: {error}")))?;
        let entry_type = entry.header().entry_type();

        // Ignore benign PAX extended headers that tar handles internally.
        if entry_type.is_pax_global_extensions()
            || entry_type.is_pax_local_extensions()
            || entry_type.is_gnu_longname()
            || entry_type.is_gnu_longlink()
        {
            continue;
        }

        let raw_path = entry
            .path()
            .map_err(|error| KisekiError::other(format!("unreadable tar path: {error}")))?
            .into_owned();
        let clean_segments = sanitize_tar_path(&raw_path)?;

        // Apply wrapper prefix and subpath filters before checking special types
        // outside the requested subpath; inside the target subtree, reject ALL
        // non-regular / non-directory entries unconditionally.
        let Some(rel_segments) =
            strip_prefix_and_subpath(&clean_segments, effective_strip.as_deref(), normalized_subpath.as_deref())
        else {
            // Even outside the subpath, refuse symlinks/hardlinks when no
            // subpath isolation was requested.
            if normalized_subpath.is_none()
                && !entry_type.is_dir()
                && !entry_type.is_file()
            {
                return Err(KisekiError::unsafe_path(
                    raw_path.to_string_lossy(),
                    "archive contains a disallowed symlink, hardlink, or special file",
                ));
            }
            continue;
        };

        if entry_type.is_dir() {
            if !rel_segments.is_empty() {
                let dir_path = join_segments(dest_dir, &rel_segments);
                std::fs::create_dir_all(&dir_path)
                    .map_err(|error| KisekiError::io(&dir_path, error))?;
            }
            continue;
        }

        if !entry_type.is_file() {
            return Err(KisekiError::unsafe_path(
                raw_path.to_string_lossy(),
                "archive contains a disallowed symlink, hardlink, or special file",
            ));
        }

        if rel_segments.is_empty() {
            continue;
        }

        extracted_files += 1;
        if extracted_files > MAX_FILES {
            return Err(KisekiError::other(format!(
                "theme archive exceeds maximum file count ({MAX_FILES})"
            )));
        }

        let remaining = MAX_BYTES.saturating_sub(extracted_bytes);
        let mut buf = Vec::new();
        let mut limited = entry.by_ref().take(remaining + 1);
        limited
            .read_to_end(&mut buf)
            .map_err(|error| KisekiError::other(format!("failed reading tar entry: {error}")))?;
        if buf.len() as u64 > remaining {
            return Err(KisekiError::other(format!(
                "theme archive exceeds maximum unpacked size ({MAX_BYTES} bytes)"
            )));
        }
        extracted_bytes += buf.len() as u64;

        let out_path = join_segments(dest_dir, &rel_segments);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| KisekiError::io(parent, error))?;
        }
        std::fs::write(&out_path, &buf).map_err(|error| KisekiError::io(&out_path, error))?;
    }

    if extracted_files == 0 {
        return Err(KisekiError::other(
            "theme archive contained no extractable files for the requested path",
        ));
    }

    Ok(ExtractStats {
        sha256: actual_sha,
        compressed_bytes: bytes.len() as u64,
        extracted_files,
        extracted_bytes,
    })
}

fn detect_single_wrapper_prefix(bytes: &[u8]) -> Result<Option<String>> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut top_dirs = BTreeSet::new();
    let mut has_root_file = false;

    let entries = archive
        .entries()
        .map_err(|error| KisekiError::other(format!("invalid tar.gz archive: {error}")))?;
    for entry_result in entries {
        let entry = entry_result
            .map_err(|error| KisekiError::other(format!("corrupt tar entry: {error}")))?;
        let ty = entry.header().entry_type();
        if ty.is_pax_global_extensions()
            || ty.is_pax_local_extensions()
            || ty.is_gnu_longname()
            || ty.is_gnu_longlink()
        {
            continue;
        }
        let raw_path = entry
            .path()
            .map_err(|error| KisekiError::other(format!("unreadable tar path: {error}")))?
            .into_owned();
        let segments = sanitize_tar_path(&raw_path)?;
        if segments.is_empty() {
            continue;
        }
        if segments.len() == 1 && ty.is_file() {
            has_root_file = true;
        } else {
            top_dirs.insert(segments[0].clone());
        }
    }

    if !has_root_file && top_dirs.len() == 1 {
        Ok(top_dirs.into_iter().next())
    } else {
        Ok(None)
    }
}

fn sanitize_tar_path(path: &Path) -> Result<Vec<String>> {
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(os) => {
                let text = os.to_str().ok_or_else(|| {
                    KisekiError::unsafe_path(path.to_string_lossy(), "non-UTF-8 archive path")
                })?;
                if text.is_empty() || text.contains('\\') || text.chars().any(char::is_control) {
                    return Err(KisekiError::unsafe_path(
                        path.to_string_lossy(),
                        "illegal characters in archive path",
                    ));
                }
                segments.push(text.to_string());
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(KisekiError::unsafe_path(
                    path.to_string_lossy(),
                    "archive path attempts directory traversal or uses an absolute root",
                ));
            }
        }
    }
    Ok(segments)
}

fn strip_prefix_and_subpath(
    segments: &[String],
    strip_prefix: Option<&str>,
    subpath: Option<&str>,
) -> Option<Vec<String>> {
    let mut rest = segments;
    if let Some(prefix) = strip_prefix {
        let prefix_parts: Vec<&str> = prefix.split('/').filter(|p| !p.is_empty()).collect();
        if rest.len() < prefix_parts.len() {
            return None;
        }
        for (a, b) in rest.iter().zip(prefix_parts.iter()) {
            if a != b {
                return None;
            }
        }
        rest = &rest[prefix_parts.len()..];
    }
    if let Some(sub) = subpath {
        let sub_parts: Vec<&str> = sub.split('/').filter(|p| !p.is_empty()).collect();
        if rest.len() < sub_parts.len() {
            return None;
        }
        for (a, b) in rest.iter().zip(sub_parts.iter()) {
            if a != b {
                return None;
            }
        }
        rest = &rest[sub_parts.len()..];
    }
    Some(rest.to_vec())
}

fn join_segments(root: &Path, segments: &[String]) -> PathBuf {
    let mut path = root.to_path_buf();
    for seg in segments {
        path.push(seg);
    }
    path
}

/// Packs a local theme directory into a deterministic `.tar.gz` byte buffer.
pub fn pack_dir_tar_gz(dir: &Path, top_prefix: Option<&str>) -> Result<Vec<u8>> {
    let files = kiseki_pkg::collect_files(dir)?;
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = tar::Builder::new(encoder);

    for file in files {
        if file.is_dir {
            continue;
        }
        if file.is_symlink || file.is_special {
            return Err(KisekiError::unsafe_path(
                &file.relative,
                "cannot pack symlink or special file",
            ));
        }
        let disk_path = dir.join(&file.relative);
        let archive_path = match top_prefix.filter(|p| !p.is_empty()) {
            Some(prefix) => format!("{}/{}", prefix.trim_matches('/'), file.relative),
            None => file.relative.clone(),
        };
        builder
            .append_path_with_name(&disk_path, &archive_path)
            .map_err(|error| KisekiError::io(&disk_path, error))?;
    }

    let encoder = builder
        .into_inner()
        .map_err(|error| KisekiError::other(format!("failed finalizing tar archive: {error}")))?;
    encoder
        .finish()
        .map_err(|error| KisekiError::other(format!("failed finalizing gzip stream: {error}")))
}
