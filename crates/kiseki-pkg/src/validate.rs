//! Strict theme-pack validation (RFC-0004 §4).
//!
//! A theme pack is untrusted input. This module is the only gate between "bytes
//! on disk" and "installed", and it enforces the extension whitelist, Zip Slip
//! and symlink rejection, size/count caps, the SGR-only logo audit, schema
//! validation, `naga` shader validation and WCAG contrast.
//!
//! Two rules keep a hostile pack from being read at all: every fatal diagnostic
//! from the structural pass aborts before a single file is opened, and every
//! content read is bounded (`Read::take`) and only allowed for a path that is on
//! the validated regular-file listing.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use kiseki_platform::Platform;
use kiseki_schema::{
    lint_contrast, security, validate_pack, ContrastReport, PackAssets, ResolvedTheme, ThemePack, ThemeTokens,
};
use kiseki_types::{Diagnostic, DiagnosticReport, KisekiError, RenderingProfile, Result};

/// Maximum number of files in a theme pack.
pub const MAX_FILES: usize = 64;
/// Maximum total unpacked size (30 MB).
pub const MAX_BYTES: u64 = 30 * 1024 * 1024;
/// Maximum logo size (32 KiB).
pub const MAX_LOGO_BYTES: usize = 32 * 1024;
/// Maximum size of a single shader template (256 KiB).
pub const MAX_SHADER_BYTES: usize = 256 * 1024;
/// Maximum size of `manifest.json` / `tokens.json` (1 MiB).
pub const MAX_JSON_BYTES: usize = 1024 * 1024;
/// Maximum number of bytes read to parse an image header.
pub const MAX_IMAGE_HEADER_BYTES: usize = 64 * 1024;
/// Largest width or height a pack image may declare.
pub const MAX_IMAGE_DIMENSION: u32 = 8192;

/// One file discovered under a pack root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeFile {
    pub relative: String,
    pub bytes: u64,
    pub is_symlink: bool,
    pub is_dir: bool,
    /// True for FIFOs, sockets and device nodes: anything that is neither a
    /// regular file, a directory nor a symlink.
    pub is_special: bool,
}

/// Everything validation learned about a pack.
#[derive(Debug, Clone, Default)]
pub struct ThemeValidation {
    pub pack: Option<ThemePack>,
    pub diagnostics: DiagnosticReport,
    pub shader_reports: Vec<kiseki_shader::ShaderReport>,
    pub contrast: Option<ContrastReport>,
    pub file_count: usize,
    pub total_bytes: u64,
}

impl ThemeValidation {
    pub fn is_valid(&self) -> bool {
        !self.diagnostics.has_fatal()
    }

    pub fn shader_summaries(&self) -> Vec<String> {
        self.shader_reports
            .iter()
            .map(kiseki_shader::ShaderReport::summary)
            .collect()
    }
}

impl From<ThemeValidation> for DiagnosticReport {
    fn from(validation: ThemeValidation) -> Self {
        validation.diagnostics
    }
}

/// Walks a pack without following symlinks, sorted for determinism.
pub fn collect_files(root: &Path) -> Result<Vec<ThemeFile>> {
    if !root.is_dir() {
        return Err(KisekiError::unsafe_path(
            root.to_string_lossy(),
            "theme pack path is not a directory",
        ));
    }
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|error| KisekiError::io(&dir, error))?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| KisekiError::io(&dir, error))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| KisekiError::io(&path, error))?;
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let is_symlink = metadata.file_type().is_symlink();
            let is_dir = metadata.is_dir();
            let is_file = metadata.is_file();
            files.push(ThemeFile {
                relative: relative.clone(),
                bytes: if is_dir { 0 } else { metadata.len() },
                is_symlink,
                is_dir: is_dir && !is_symlink,
                is_special: !is_symlink && !is_dir && !is_file,
            });
            if is_dir && !is_symlink {
                stack.push(path);
            }
        }
    }
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(files)
}

/// True when a pack-relative path is allowed by RFC-0004 §4.2.
pub fn allowed_file(relative: &str) -> std::result::Result<(), String> {
    security::validate_relative_path(relative)?;
    let extension = relative
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    if let Some(extension) = &extension {
        if security::FORBIDDEN_EXTENSIONS.contains(&extension.as_str()) {
            return Err(format!(
                "extension '.{extension}' is executable/script content and is never allowed"
            ));
        }
    }
    let exact = matches!(
        relative,
        "manifest.json" | "tokens.json" | "LICENSE" | "CREDITS.md" | "assets/logo.txt"
    );
    if exact {
        return Ok(());
    }
    if relative.starts_with("assets/") || relative.starts_with("previews/") {
        let extension = extension.unwrap_or_default();
        if security::ALLOWED_ASSET_EXTENSIONS.contains(&extension.as_str()) {
            return Ok(());
        }
        return Err(format!(
            "image under {relative:?} must be one of {}",
            security::ALLOWED_ASSET_EXTENSIONS.join(", ")
        ));
    }
    if relative.starts_with("shaders/") {
        let extension = extension.unwrap_or_default();
        if matches!(extension.as_str(), "glsl" | "hlsl") {
            return Ok(());
        }
        return Err(format!(
            "shader {relative:?} must be .glsl or .hlsl, not .{extension}"
        ));
    }
    Err(format!(
        "{relative:?} is not part of the allowed theme-pack layout"
    ))
}

/// Validates a complete theme directory.
pub fn validate_theme_dir(root: &Path) -> Result<ThemeValidation> {
    let mut validation = ThemeValidation::default();

    let files = match collect_files(root) {
        Ok(files) => files,
        Err(error) => {
            validation.diagnostics.push(Diagnostic::error(
                "validation.pack.missing",
                error.to_string(),
            ));
            return Ok(validation);
        }
    };

    // --- 1. Structural pass: listing only, nothing is opened ----------------
    let mut listing = Vec::new();
    let mut regular: BTreeSet<String> = BTreeSet::new();
    let mut sizes: BTreeMap<String, u64> = BTreeMap::new();
    for file in &files {
        if file.is_symlink {
            validation.diagnostics.push(
                Diagnostic::error(
                    "security.symlink",
                    format!("theme packs must not contain symlinks: {}", file.relative),
                )
                .with_hint("replace the symlink with the real file before packing"),
            );
            continue;
        }
        if file.is_dir {
            continue;
        }
        if file.is_special {
            validation.diagnostics.push(
                Diagnostic::error(
                    "security.special-file",
                    format!(
                        "theme packs must contain only regular files and directories: {}",
                        file.relative
                    ),
                )
                .with_hint("remove FIFOs, sockets and device nodes from the pack"),
            );
            continue;
        }
        match allowed_file(&file.relative) {
            Ok(()) => {
                regular.insert(file.relative.clone());
            }
            Err(reason) => validation.diagnostics.push(Diagnostic::error(
                "security.file-not-allowed",
                format!("{}: {reason}", file.relative),
            )),
        }
        sizes.insert(file.relative.clone(), file.bytes);
        listing.push(file.relative.clone());
        validation.total_bytes += file.bytes;
    }
    validation.file_count = listing.len();

    if validation.file_count > MAX_FILES {
        validation.diagnostics.push(Diagnostic::error(
            "security.too-many-files",
            format!(
                "theme pack has {} files, exceeding the limit of {MAX_FILES}",
                validation.file_count
            ),
        ));
    }
    if validation.total_bytes > MAX_BYTES {
        validation.diagnostics.push(Diagnostic::error(
            "security.too-large",
            format!(
                "theme pack is {} bytes, exceeding the limit of {MAX_BYTES}",
                validation.total_bytes
            ),
        ));
    }
    for required in ["manifest.json", "tokens.json", "LICENSE", "CREDITS.md"] {
        if !listing.iter().any(|path| path == required) {
            validation.diagnostics.push(
                Diagnostic::error(
                    "validation.pack.missing-file",
                    format!("theme pack is missing required file {required}"),
                )
                .with_hint("add LICENSE and CREDITS.md; both are mandatory for every theme"),
            );
        }
    }

    // --- 2. Early abort: never open a pack that already failed structurally --
    if validation.diagnostics.has_fatal() {
        return Ok(validation);
    }
    if !regular.contains("manifest.json") || !regular.contains("tokens.json") {
        validation.diagnostics.push(Diagnostic::error(
            "validation.pack.missing",
            "manifest.json and tokens.json must be regular files inside the pack",
        ));
        return Ok(validation);
    }

    // --- 3. Manifest + tokens (bounded reads) -------------------------------
    let manifest_raw = match read_text_bounded(root, "manifest.json", MAX_JSON_BYTES) {
        Ok(Some(raw)) => raw,
        Ok(None) => {
            validation.diagnostics.push(Diagnostic::error(
                "validation.manifest.missing",
                "manifest.json is missing",
            ));
            return Ok(validation);
        }
        Err(error) => {
            validation.diagnostics.push(Diagnostic::error(
                "validation.manifest.read",
                error.to_string(),
            ));
            return Ok(validation);
        }
    };
    let tokens_raw = match read_text_bounded(root, "tokens.json", MAX_JSON_BYTES) {
        Ok(Some(raw)) => raw,
        Ok(None) => {
            validation.diagnostics.push(Diagnostic::error(
                "validation.tokens.missing",
                "tokens.json is missing",
            ));
            return Ok(validation);
        }
        Err(error) => {
            validation.diagnostics.push(Diagnostic::error(
                "validation.tokens.read",
                error.to_string(),
            ));
            return Ok(validation);
        }
    };

    let manifest = match serde_json::from_str(&manifest_raw) {
        Ok(manifest) => manifest,
        Err(error) => {
            validation.diagnostics.push(Diagnostic::error(
                "validation.manifest.json",
                format!("manifest.json does not parse: {error}"),
            ));
            return Ok(validation);
        }
    };
    let tokens: ThemeTokens = match serde_json::from_str(&tokens_raw) {
        Ok(tokens) => tokens,
        Err(error) => {
            validation.diagnostics.push(Diagnostic::error(
                "validation.tokens.json",
                format!("tokens.json does not parse: {error}"),
            ));
            return Ok(validation);
        }
    };
    let pack = ThemePack { manifest, tokens };

    // --- 4. Logo: invalid UTF-8 is a hard failure, never silently ignored ----
    let mut logo_text = None;
    if regular.contains("assets/logo.txt") {
        if sizes.get("assets/logo.txt").is_some_and(|size| *size > MAX_LOGO_BYTES as u64) {
            validation.diagnostics.push(Diagnostic::error(
                "security.logo-size",
                format!("assets/logo.txt exceeds the {MAX_LOGO_BYTES} byte limit"),
            ));
        } else {
            match read_text_bounded(root, "assets/logo.txt", MAX_LOGO_BYTES) {
                Ok(text) => logo_text = text,
                Err(error) => validation.diagnostics.push(Diagnostic::error(
                    "security.logo-text",
                    format!("assets/logo.txt cannot be read as UTF-8 text: {error}"),
                )),
            }
        }
    }

    let outcome = validate_pack(
        &pack,
        &PackAssets {
            logo_text,
            file_listing: listing.clone(),
        },
    );
    validation.diagnostics.extend(outcome.diagnostics.items);

    // --- 5. Images: bounded header parsing, no full decode ------------------
    check_image_headers(root, &regular, &mut validation);

    // --- 6. Shaders: only listed, validated relative paths are opened --------
    let glsl = read_referenced(
        root,
        &regular,
        pack.tokens
            .atmosphere
            .as_ref()
            .and_then(|atmosphere| atmosphere.shader.as_ref())
            .and_then(|shader| shader.glsl.as_deref()),
        MAX_SHADER_BYTES,
        &mut validation,
    );
    let hlsl = read_referenced(
        root,
        &regular,
        pack.tokens
            .atmosphere
            .as_ref()
            .and_then(|atmosphere| atmosphere.shader.as_ref())
            .and_then(|shader| shader.hlsl.as_deref()),
        MAX_SHADER_BYTES,
        &mut validation,
    );
    for profile in [
        RenderingProfile::Eco,
        RenderingProfile::Balanced,
        RenderingProfile::Ultra,
    ] {
        let theme = ResolvedTheme::from_pack(&pack, profile);
        if let Some(path) = theme.glsl_path() {
            match glsl.as_deref() {
                Some(template) => {
                    let report = kiseki_shader::validate_glsl(&theme, template);
                    validation
                        .diagnostics
                        .extend(report.diagnostics.iter().cloned());
                    validation.shader_reports.push(report);
                }
                None => validation.diagnostics.push(Diagnostic::error(
                    "validation.shader.missing-glsl",
                    format!("{path} is declared but missing"),
                )),
            }
        }
        if let Some(path) = theme.hlsl_path() {
            match hlsl.as_deref() {
                Some(template) => {
                    let report = kiseki_shader::validate_hlsl(&theme, template);
                    validation
                        .diagnostics
                        .extend(report.diagnostics.iter().cloned());
                    validation.shader_reports.push(report);
                }
                None => validation.diagnostics.push(Diagnostic::error(
                    "validation.shader.missing-hlsl",
                    format!("{path} is declared but missing"),
                )),
            }
        }
    }

    // --- 7. Contrast -------------------------------------------------------
    let balanced = ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced);
    let contrast = lint_contrast(&balanced.palette, Some(&balanced.syntax));
    validation
        .diagnostics
        .extend(contrast.to_diagnostics().items);
    validation.contrast = Some(contrast);
    validation.pack = Some(pack);

    Ok(validation)
}

/// Ensures `path` is a regular file (never a symlink, directory, FIFO, socket
/// or device) *before* opening it so discovery and validation cannot hang.
fn check_regular_file(path: &Path) -> Result<bool> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(KisekiError::io(path, error)),
    };
    if metadata.file_type().is_symlink() {
        return Err(KisekiError::security(format!(
            "refusing to read symlink inside theme pack: {}",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(KisekiError::security(format!(
            "refusing to read non-regular file inside theme pack: {}",
            path.display()
        )));
    }
    Ok(true)
}

/// Reads at most `limit` bytes, returning `None` when the file is absent.
///
/// Exceeding the limit is an error rather than a truncated read.
fn read_bounded(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    if !check_regular_file(path)? {
        return Ok(None);
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(KisekiError::io(path, error)),
    };
    let mut buffer = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut buffer)
        .map_err(|error| KisekiError::io(path, error))?;
    if buffer.len() > limit {
        return Err(KisekiError::security(format!(
            "{} exceeds the {limit} byte limit",
            path.display()
        )));
    }
    Ok(Some(buffer))
}

/// Reads the first `limit` bytes without failing on large files (image headers).
fn read_head(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    if !check_regular_file(path)? {
        return Ok(None);
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(KisekiError::io(path, error)),
    };
    let mut buffer = Vec::new();
    file.take(limit as u64)
        .read_to_end(&mut buffer)
        .map_err(|error| KisekiError::io(path, error))?;
    Ok(Some(buffer))
}

/// Reads a UTF-8 pack file with a hard byte cap.
pub(crate) fn read_text_bounded(root: &Path, relative: &str, limit: usize) -> Result<Option<String>> {
    if security::validate_relative_path(relative).is_err() {
        return Err(KisekiError::security(format!(
            "{relative:?} is not a safe pack-relative path"
        )));
    }
    let Some(bytes) = read_bounded(&root.join(relative), limit)? else {
        return Ok(None);
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| KisekiError::security(format!("{relative} is not valid UTF-8")))
}

/// Reads a file referenced by `tokens.json`, but only when the path is safe and
/// present on the validated regular-file listing.
fn read_referenced(
    root: &Path,
    regular: &BTreeSet<String>,
    relative: Option<&str>,
    limit: usize,
    validation: &mut ThemeValidation,
) -> Option<String> {
    let relative = relative?;
    if security::validate_relative_path(relative).is_err() || !regular.contains(relative) {
        // `validate_pack` reports `schema.asset.missing` for this case.
        return None;
    }
    match read_text_bounded(root, relative, limit) {
        Ok(text) => text,
        Err(error) => {
            validation.diagnostics.push(Diagnostic::error(
                "validation.asset.read",
                format!("{relative} cannot be read: {error}"),
            ));
            None
        }
    }
}

/// Parses the header of every shipped image and rejects spoofed or oversized
/// dimensions. Nothing is decoded; only the first few KiB are read.
fn check_image_headers(root: &Path, regular: &BTreeSet<String>, validation: &mut ThemeValidation) {
    for relative in regular {
        if !(relative.starts_with("assets/") || relative.starts_with("previews/")) {
            continue;
        }
        if !security::ALLOWED_ASSET_EXTENSIONS
            .iter()
            .any(|extension| relative.to_ascii_lowercase().ends_with(&format!(".{extension}")))
        {
            continue;
        }
        let head = match read_head(&root.join(relative), MAX_IMAGE_HEADER_BYTES) {
            Ok(Some(head)) => head,
            Ok(None) => continue,
            Err(error) => {
                validation.diagnostics.push(Diagnostic::error(
                    "validation.asset.read",
                    format!("{relative} cannot be read: {error}"),
                ));
                continue;
            }
        };
        match image_dimensions(&head) {
            Err(reason) => validation.diagnostics.push(Diagnostic::error(
                "security.image-header",
                format!("{relative}: {reason}"),
            )),
            Ok((format, width, height)) => {
                if let Some(expected) = expected_image_format(relative) {
                    if expected != format {
                        validation.diagnostics.push(Diagnostic::error(
                            "security.image-header",
                            format!(
                                "{relative}: file extension expects {} but the header is {}",
                                expected.extension(),
                                format.extension()
                            ),
                        ));
                        continue;
                    }
                }
                if width == 0
                    || height == 0
                    || width > MAX_IMAGE_DIMENSION
                    || height > MAX_IMAGE_DIMENSION
                {
                    validation.diagnostics.push(Diagnostic::error(
                        "security.image-dimensions",
                        format!(
                            "{relative} is {width}x{height}; images must be between 1x1 and {MAX_IMAGE_DIMENSION}x{MAX_IMAGE_DIMENSION}"
                        ),
                    ));
                }
            }
        }
    }
}

/// The image format a pack-relative path's extension promises.
fn expected_image_format(relative: &str) -> Option<ImageFormat> {
    let lower = relative.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some(ImageFormat::Png)
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some(ImageFormat::Jpeg)
    } else if lower.ends_with(".webp") {
        Some(ImageFormat::Webp)
    } else {
        None
    }
}

/// Image container formats a pack may ship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

impl ImageFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => ".png",
            Self::Jpeg => ".jpg/.jpeg",
            Self::Webp => ".webp",
        }
    }
}

/// Parses PNG, JPEG and WebP dimensions from a bounded header buffer.
pub fn image_dimensions(bytes: &[u8]) -> std::result::Result<(ImageFormat, u32, u32), String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return png_dimensions(bytes).map(|(width, height)| (ImageFormat::Png, width, height));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg_dimensions(bytes).map(|(width, height)| (ImageFormat::Jpeg, width, height));
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return webp_dimensions(bytes).map(|(width, height)| (ImageFormat::Webp, width, height));
    }
    Err("not a PNG, JPEG or WebP header".to_string())
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn le_u24(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 3)?;
    Some(u32::from(slice[0]) | (u32::from(slice[1]) << 8) | (u32::from(slice[2]) << 16))
}

fn png_dimensions(bytes: &[u8]) -> std::result::Result<(u32, u32), String> {
    // signature(8) + chunk length(4) + "IHDR"(4) + width(4) + height(4)
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return Err("PNG is missing its IHDR chunk".to_string());
    }
    let width = be_u32(bytes, 16).ok_or("truncated PNG IHDR")?;
    let height = be_u32(bytes, 20).ok_or("truncated PNG IHDR")?;
    Ok((width, height))
}

fn jpeg_dimensions(bytes: &[u8]) -> std::result::Result<(u32, u32), String> {
    let mut index = 2usize;
    while index + 1 < bytes.len() {
        if bytes[index] != 0xFF {
            return Err("malformed JPEG marker stream".to_string());
        }
        let marker = bytes[index + 1];
        index += 2;
        // Standalone markers carry no length.
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 {
            continue;
        }
        if marker == 0xDA {
            // Start of scan: no frame header was found before the pixel data.
            return Err("JPEG has no SOF frame header".to_string());
        }
        let length = usize::from(be_u16(bytes, index).ok_or("truncated JPEG segment length")?);
        if length < 2 {
            return Err("JPEG segment length is invalid".to_string());
        }
        let is_sof = (0xC0..=0xCF).contains(&marker)
            && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let height = u32::from(be_u16(bytes, index + 3).ok_or("truncated JPEG SOF height")?);
            let width = u32::from(be_u16(bytes, index + 5).ok_or("truncated JPEG SOF width")?);
            return Ok((width, height));
        }
        index = index.saturating_add(length);
    }
    Err("JPEG is missing a SOF frame header".to_string())
}

fn webp_dimensions(bytes: &[u8]) -> std::result::Result<(u32, u32), String> {
    let chunk = bytes.get(12..16).ok_or("truncated WebP chunk header")?;
    match chunk {
        b"VP8X" => {
            // 4 bytes flags/reserved, then 3-byte little-endian canvas size - 1.
            let width = le_u24(bytes, 24).ok_or("truncated WebP VP8X width")?;
            let height = le_u24(bytes, 27).ok_or("truncated WebP VP8X height")?;
            Ok((width + 1, height + 1))
        }
        b"VP8L" => {
            if bytes.get(20) != Some(&0x2F) {
                return Err("WebP VP8L signature is invalid".to_string());
            }
            let bits = u32::from_le_bytes([
                *bytes.get(21).ok_or("truncated WebP VP8L width")?,
                *bytes.get(22).ok_or("truncated WebP VP8L width")?,
                *bytes.get(23).ok_or("truncated WebP VP8L width")?,
                *bytes.get(24).ok_or("truncated WebP VP8L width")?,
            ]);
            let width = (bits & 0x3FFF) + 1;
            let height = ((bits >> 14) & 0x3FFF) + 1;
            Ok((width, height))
        }
        b"VP8 " => {
            // 3-byte frame tag, then the 3-byte start code 9D 01 2A.
            if bytes.get(23..26) != Some(&[0x9D, 0x01, 0x2A]) {
                return Err("WebP VP8 start code is invalid".to_string());
            }
            let width = u16::from_le_bytes([
                *bytes.get(26).ok_or("truncated WebP VP8 width")?,
                *bytes.get(27).ok_or("truncated WebP VP8 width")?,
            ]) & 0x3FFF;
            let height = u16::from_le_bytes([
                *bytes.get(28).ok_or("truncated WebP VP8 height")?,
                *bytes.get(29).ok_or("truncated WebP VP8 height")?,
            ]) & 0x3FFF;
            Ok((u32::from(width), u32::from(height)))
        }
        _ => Err(format!(
            "unsupported WebP chunk {:?}",
            String::from_utf8_lossy(chunk)
        )),
    }
}

/// An installed community theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledTheme {
    pub id: String,
    pub path: PathBuf,
    pub file_count: usize,
    pub total_bytes: u64,
}

/// Validates and copies a local directory into the community theme store.
///
/// Only allowlisted files are copied. Symlinks are rejected rather than
/// followed, so a malicious pack cannot smuggle `~/.ssh` into the store.
pub fn install_from_dir(
    platform: &dyn Platform,
    source: &Path,
    force: bool,
) -> Result<InstalledTheme> {
    let validation = validate_theme_dir(source)?;
    if !validation.is_valid() {
        let failures: BTreeSet<String> = validation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_fatal())
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
            .collect();
        return Err(KisekiError::security(format!(
            "theme pack rejected: {}",
            failures.into_iter().collect::<Vec<_>>().join("; ")
        )));
    }
    let pack = validation
        .pack
        .as_ref()
        .ok_or_else(|| KisekiError::security("theme pack has no manifest"))?;
    let id = pack.manifest.id.as_str().to_string();
    let destination = platform.paths().community_themes_dir().join(&id);
    if destination.exists() && !force {
        return Err(KisekiError::security(format!(
            "theme '{id}' is already installed at {}; pass --force to replace it",
            destination.display()
        )));
    }

    let files = collect_files(source)?;
    let mut installed_files = 0;
    let mut total_bytes = 0;
    for file in files {
        if file.is_dir || file.is_symlink || file.is_special {
            continue;
        }
        if allowed_file(&file.relative).is_err() {
            continue;
        }
        let bytes = std::fs::read(source.join(&file.relative))
            .map_err(|error| KisekiError::io(source.join(&file.relative), error))?;
        platform
            .write_preserving_symlinks(&destination.join(&file.relative), &bytes)?;
        installed_files += 1;
        total_bytes += file.bytes;
    }

    Ok(InstalledTheme {
        id,
        path: destination,
        file_count: installed_files,
        total_bytes,
    })
}

/// Removes a community theme. Official/bundled themes are protected.
pub fn remove_installed(platform: &dyn Platform, id: &str) -> Result<PathBuf> {
    // Validate before joining so `.`, `..` or a nested path can never escape
    // the community store, and require the target to be a *direct* child.
    let theme_id = kiseki_types::ThemeId::new(id)?;
    let community = platform.paths().community_themes_dir();
    let destination = community.join(theme_id.as_str());
    if !destination.exists() {
        return Err(KisekiError::ThemeNotFound(id.to_string()));
    }
    let canonical = std::fs::canonicalize(&destination)
        .map_err(|error| KisekiError::io(&destination, error))?;
    let canonical_root = std::fs::canonicalize(&community)
        .map_err(|error| KisekiError::io(&community, error))?;
    if canonical == canonical_root || canonical.parent() != Some(canonical_root.as_path()) {
        return Err(KisekiError::security(format!(
            "refusing to remove {id}: it is not a direct child of the community theme store"
        )));
    }
    std::fs::remove_dir_all(&destination)
        .map_err(|error| KisekiError::io(&destination, error))?;
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_platform::MockPlatform;

    fn nerv_eva_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("themes")
            .join("nerv-eva")
    }

    #[test]
    fn official_nerv_eva_pack_is_clean() {
        let validation = validate_theme_dir(&nerv_eva_dir()).unwrap();
        let failures: Vec<String> = validation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_fatal())
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
            .collect();
        assert!(failures.is_empty(), "{failures:#?}");
        assert!(validation.is_valid());
        assert_eq!(validation.file_count, 9);
        assert!(validation.total_bytes > 0);
        assert!(validation.contrast.as_ref().unwrap().is_compliant());
        // 3 profiles x 2 languages, no shader at static.
        assert_eq!(validation.shader_reports.len(), 6);
        assert!(validation.shader_reports.iter().all(kiseki_shader::ShaderReport::is_ok));
        assert_eq!(
            validation.pack.unwrap().manifest.id.as_str(),
            "nerv-eva"
        );
    }

    #[test]
    fn allowed_file_whitelist_matches_the_rfc() {
        for allowed in [
            "manifest.json",
            "tokens.json",
            "LICENSE",
            "CREDITS.md",
            "assets/logo.txt",
            "assets/wallpaper.png",
            "previews/preview.png",
            "shaders/live.glsl",
            "shaders/live.hlsl",
        ] {
            assert!(allowed_file(allowed).is_ok(), "{allowed} must be allowed");
        }
        for rejected in [
            "evil.lua",
            "assets/evil.sh",
            "shaders/evil.py",
            "theme.json",
            "fastfetch.jsonc",
            "prompt.omp.json",
            "root.glsl",
            "assets/font.ttf",
            "scripts/setup.zsh",
            "../escape.png",
            "assets/../../escape.png",
        ] {
            assert!(allowed_file(rejected).is_err(), "{rejected} must be rejected");
        }
    }

    #[test]
    fn symlinks_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        std::fs::write(&target, "x").unwrap();
        let link = dir.path().join("assets").join("logo.txt");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &link).unwrap();
        let validation = validate_theme_dir(dir.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.symlink" && diagnostic.is_fatal()));
    }

    #[test]
    fn script_extension_is_rejected() {
        let pack = copy_nerv_eva();
        std::fs::write(pack.path().join("evil.lua"), "os.execute('rm -rf /')").unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.file-not-allowed"));
        pack.close();
    }

    #[test]
    fn too_many_files_is_rejected() {
        let pack = copy_nerv_eva();
        for index in 0..(MAX_FILES + 1) {
            std::fs::write(pack.path().join(format!("assets/extra-{index}.png")), b"png").unwrap();
        }
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.too-many-files"));
        pack.close();
    }

    #[test]
    fn oversized_pack_is_rejected_without_reading_the_payload() {
        let pack = copy_nerv_eva();
        let big = pack.path().join("assets/big.png");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.too-large"));
        pack.close();
    }

    #[test]
    fn logo_with_osc_escape_is_rejected() {
        let pack = copy_nerv_eva();
        std::fs::write(
            pack.path().join("assets/logo.txt"),
            "\x1b]8;;https://evil\x07click\x1b]8;;\x07\n",
        )
        .unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.logo-escape"));
        pack.close();
    }

    #[test]
    fn valid_pack_installs_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let source = copy_nerv_eva();
        let installed = install_from_dir(&platform, source.path(), false).unwrap();
        assert_eq!(installed.id, "nerv-eva");
        assert!(installed.path.starts_with(platform.paths().community_themes_dir()));
        assert!(installed.path.join("manifest.json").exists());
        assert!(installed.path.join("assets/wallpaper.png").exists());
        assert!(!installed.path.join("evil.lua").exists());

        let error = install_from_dir(&platform, source.path(), false).unwrap_err();
        assert!(error.to_string().contains("already installed"), "{error}");
        let replaced = install_from_dir(&platform, source.path(), true).unwrap();
        assert_eq!(replaced.id, "nerv-eva");

        let removed = remove_installed(&platform, "nerv-eva").unwrap();
        assert!(!removed.exists());
        assert!(remove_installed(&platform, "nerv-eva").is_err());
        source.close();
    }

    #[test]
    fn broken_shader_is_rejected() {
        let pack = copy_nerv_eva();
        std::fs::write(
            pack.path().join("shaders/live.glsl"),
            "void mainImage(out vec4 c, in vec2 p) { c = vec4(smoothstep(1.0, 0.0, p.x)); }",
        )
        .unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "shader.smoothstep.reversed"));
        pack.close();
    }

    #[test]
    fn structural_failures_abort_before_any_file_is_read() {
        let pack = copy_nerv_eva();
        // A disallowed file plus an unparseable manifest: the second happens
        // only if contents are read, which the early abort must prevent.
        std::fs::write(pack.path().join("evil.lua"), "os.execute('rm -rf /')").unwrap();
        std::fs::write(pack.path().join("manifest.json"), b"\xFF\xFE not json").unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.file-not-allowed"));
        assert!(
            !validation
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.starts_with("validation.manifest")),
            "manifest must not be read after a structural failure: {:?}",
            validation.diagnostics
        );
        assert!(validation.pack.is_none());
        pack.close();
    }

    #[test]
    fn oversized_logo_is_rejected_without_reading_it() {
        let pack = copy_nerv_eva();
        std::fs::write(
            pack.path().join("assets/logo.txt"),
            "a".repeat(MAX_LOGO_BYTES + 1),
        )
        .unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.logo-size"));
        pack.close();
    }

    #[test]
    fn logo_with_invalid_utf8_is_a_fatal_error() {
        let pack = copy_nerv_eva();
        std::fs::write(pack.path().join("assets/logo.txt"), b"UNIT\xFF\xFE-02\n").unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        let diagnostic = validation
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "security.logo-text")
            .expect("invalid UTF-8 logo must be reported");
        assert!(diagnostic.is_fatal());
        pack.close();
    }

    #[test]
    fn spoofed_and_oversized_images_are_rejected() {
        let pack = copy_nerv_eva();
        std::fs::write(pack.path().join("assets/wallpaper.png"), b"not a png at all").unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.image-header"));
        std::fs::write(
            pack.path().join("assets/wallpaper.png"),
            png_header(MAX_IMAGE_DIMENSION + 1, 1080),
        )
        .unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.image-dimensions"));
        std::fs::write(pack.path().join("assets/wallpaper.png"), png_header(0, 1080)).unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        assert!(validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "security.image-dimensions"));
        pack.close();
    }

    #[test]
    fn image_content_must_match_the_file_extension() {
        let pack = copy_nerv_eva();
        // JPEG bytes behind a .png name are a spoofing attempt, not a typo.
        std::fs::write(
            pack.path().join("assets/wallpaper.png"),
            jpeg_header(640, 480),
        )
        .unwrap();
        let validation = validate_theme_dir(pack.path()).unwrap();
        let diagnostic = validation
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "security.image-header")
            .expect("a format/extension mismatch must be fatal");
        assert!(diagnostic.is_fatal());
        assert!(diagnostic.message.contains("expects .png"), "{}", diagnostic.message);
        pack.close();
    }

    #[cfg(unix)]
    #[test]
    fn special_files_are_rejected() {
        let pack = copy_nerv_eva();
        let fifo = pack.path().join("assets/pipe.png");
        let status = std::process::Command::new("mkfifo").arg(&fifo).status();
        if !status.map(|status| status.success()).unwrap_or(false) {
            // mkfifo is unavailable on this host; nothing to assert.
            pack.close();
            return;
        }
        let validation = validate_theme_dir(pack.path()).unwrap();
        let diagnostic = validation
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "security.special-file")
            .expect("a FIFO must be a fatal diagnostic");
        assert!(diagnostic.is_fatal());
        assert!(diagnostic.message.contains("assets/pipe.png"));
        // The FIFO is never opened or added to the regular-file listing.
        assert!(validation.pack.is_none());
        pack.close();
    }

    #[test]
    fn image_header_parsers_cover_png_jpeg_and_webp() {
        assert_eq!(
            image_dimensions(&png_header(1920, 1080)).unwrap(),
            (ImageFormat::Png, 1920, 1080)
        );
        assert_eq!(
            image_dimensions(&jpeg_header(640, 480)).unwrap(),
            (ImageFormat::Jpeg, 640, 480)
        );
        assert_eq!(
            image_dimensions(&webp_vp8x_header(800, 600)).unwrap(),
            (ImageFormat::Webp, 800, 600)
        );
        assert_eq!(
            image_dimensions(&webp_vp8l_header(320, 200)).unwrap(),
            (ImageFormat::Webp, 320, 200)
        );
        assert_eq!(
            image_dimensions(&webp_vp8_header(1024, 768)).unwrap(),
            (ImageFormat::Webp, 1024, 768)
        );
        for broken in [
            b"".to_vec(),
            b"GIF89a".to_vec(),
            b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0d".to_vec(),
            b"RIFF\x00\x00\x00\x00WEBPXXXX".to_vec(),
        ] {
            assert!(image_dimensions(&broken).is_err(), "{broken:?}");
        }
    }

    #[test]
    fn remove_installed_rejects_non_child_paths() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        let community = platform.paths().community_themes_dir();
        platform.create_dir_all(&community).unwrap();
        // `.` and `..` never parse as a theme id.
        for raw in [".", "..", "a/b", ""] {
            let error = remove_installed(&platform, raw).unwrap_err();
            assert!(matches!(error, KisekiError::InvalidThemeId(_)), "{raw}: {error}");
        }
        // A symlink that resolves back to the store itself is not a child.
        #[cfg(unix)]
        {
            platform.create_dir_all(&community.join("real-theme")).unwrap();
            std::os::unix::fs::symlink(&community, community.join("evil")).unwrap();
            let error = remove_installed(&platform, "evil").unwrap_err();
            assert!(error.to_string().contains("community theme store"), "{error}");
            assert!(community.exists());
        }
    }

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    fn jpeg_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        bytes.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&(height as u16).to_be_bytes());
        bytes.extend_from_slice(&(width as u16).to_be_bytes());
        bytes.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        bytes
    }

    fn riff(chunk: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&((payload.len() + 8) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(chunk);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn webp_vp8x_header(width: u32, height: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 4];
        let w = width - 1;
        let h = height - 1;
        payload.extend_from_slice(&[w as u8, (w >> 8) as u8, (w >> 16) as u8]);
        payload.extend_from_slice(&[h as u8, (h >> 8) as u8, (h >> 16) as u8]);
        riff(b"VP8X", &payload)
    }

    fn webp_vp8l_header(width: u32, height: u32) -> Vec<u8> {
        let bits = (width - 1) | ((height - 1) << 14);
        let mut payload = vec![0x2F];
        payload.extend_from_slice(&bits.to_le_bytes());
        riff(b"VP8L", &payload)
    }

    fn webp_vp8_header(width: u32, height: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 3];
        payload.extend_from_slice(&[0x9D, 0x01, 0x2A]);
        payload.extend_from_slice(&(width as u16).to_le_bytes());
        payload.extend_from_slice(&(height as u16).to_le_bytes());
        riff(b"VP8 ", &payload)
    }

    fn copy_nerv_eva() -> TempPack {
        let dir = tempfile::tempdir().unwrap();
        let source = nerv_eva_dir();
        for file in collect_files(&source).unwrap() {
            if file.is_dir {
                continue;
            }
            let destination = dir.path().join(&file.relative);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::copy(source.join(&file.relative), &destination).unwrap();
        }
        TempPack { dir }
    }

    struct TempPack {
        dir: tempfile::TempDir,
    }

    impl TempPack {
        fn path(&self) -> &Path {
            self.dir.path()
        }
        fn close(self) {}
    }
}
