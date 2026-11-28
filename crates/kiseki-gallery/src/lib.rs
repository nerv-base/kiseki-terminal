//! Kiseki Gallery client, index schema, safe archive extraction, and update
//! orchestration (RFC-0006 / ADR-016).
//!
//! Deliberately isolated from `kiseki-tx`, `kiseki-compiler`, and routine
//! theme switching so offline operations never pull in or invoke HTTP code.

pub mod archive;
pub mod client;
pub mod model;

pub use archive::{extract_tar_gz_safely, pack_dir_tar_gz, sha256_hex, ExtractStats};
pub use client::{GalleryClient, GalleryInstallReport, LoadedIndex, UpdateOutcome};
pub use model::{
    compute_trending_score, filter_and_sort, normalize_subpath, parse_install_target,
    GalleryAuthor, GalleryCapabilities, GalleryDownload, GalleryFilter, GalleryIndex,
    GalleryMetrics, GalleryPalettePreview, GalleryPreview, GallerySort, GalleryTheme,
    GalleryThemeUpdateNotes, GalleryUpdates, InstallTarget, OriginRegistry, ThemeOrigin,
    DEFAULT_GALLERY_SOURCES, GALLERY_SCHEMA_VERSION,
};

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use flate2::write::GzEncoder;
    use flate2::Compression;
    use kiseki_pkg::{scaffold_theme_pack, ScaffoldOptions};
    use kiseki_platform::{MockPlatform, Platform};
    use kiseki_state::GalleryConfig;
    use kiseki_types::{Appearance, ThemeId};

    use super::*;

    fn sample_pack_tarball(id: &str, version: &str) -> (Vec<u8>, String) {
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path().join(id);
        scaffold_theme_pack(
            &pack_dir,
            &ScaffoldOptions {
                id: ThemeId::new(id).unwrap(),
                name: format!("Theme {id}"),
                author: "Alice".to_string(),
                appearance: Appearance::Dark,
                with_shader: true,
                force: true,
            },
        )
        .unwrap();
        let manifest_path = pack_dir.join("manifest.json");
        let raw = std::fs::read_to_string(&manifest_path).unwrap();
        std::fs::write(&manifest_path, raw.replace("\"0.1.0\"", &format!("\"{version}\"")))
            .unwrap();
        let bytes = pack_dir_tar_gz(&pack_dir, Some(&format!("{id}-abc12345"))).unwrap();
        let sha = sha256_hex(&bytes);
        (bytes, sha)
    }

    #[allow(clippy::too_many_arguments)]
    fn sample_entry(
        id: &str,
        version: &str,
        commit: &str,
        url: String,
        track_url: Option<String>,
        sha256: String,
        stars: u64,
        downloads: u64,
    ) -> GalleryTheme {
        GalleryTheme {
            id: id.to_string(),
            name: format!("Theme {id}"),
            subtitle: format!("Community pack {id}"),
            version: version.to_string(),
            appearance: Appearance::Dark,
            tags: vec!["dark".into(), "live-shader".into()],
            authors: vec![GalleryAuthor {
                name: "Alice".into(),
                url: None,
            }],
            license: "MIT".into(),
            repo: format!("alice/{id}"),
            subpath: ".".into(),
            commit: commit.to_string(),
            created_at: "2026-11-01T00:00:00Z".into(),
            updated_at: "2026-11-28T00:00:00Z".into(),
            capabilities: GalleryCapabilities {
                has_wallpaper: false,
                has_shader: true,
                shader_profiles: vec!["eco".into(), "balanced".into(), "ultra".into()],
                wcag_grade: "AA".into(),
            },
            metrics: GalleryMetrics {
                stars,
                stars7d: stars / 5,
                downloads,
                downloads30d: downloads / 2,
                trending_score: compute_trending_score(downloads / 2, stars / 5, stars, 1.0),
            },
            download: GalleryDownload {
                track_url,
                url,
                sha256,
                bytes: 1024,
                strip_prefix: Some(format!("{id}-abc12345")),
            },
            preview: GalleryPreview {
                thumb_url: None,
                palette: GalleryPalettePreview {
                    base: "#131722".into(),
                    surface0: "#1D2333".into(),
                    text_primary: "#E9EEF8".into(),
                    accent_primary: "#74C7EC".into(),
                    accent_secondary: "#F5A9B8".into(),
                    keyword: "#CBA6F7".into(),
                    string: "#A6E3A1".into(),
                    function: "#89B4FA".into(),
                },
            },
        }
    }

    #[test]
    fn parses_all_install_target_kinds() {
        assert_eq!(
            parse_install_target("tokyo-midnight", false, false).unwrap(),
            InstallTarget::GalleryId("tokyo-midnight".into())
        );
        assert_eq!(
            parse_install_target("github:alice/kiseki-tokyo@v1.2.0#themes/dark", false, false)
                .unwrap(),
            InstallTarget::GitHub {
                owner: "alice".into(),
                repo: "kiseki-tokyo".into(),
                git_ref: Some("v1.2.0".into()),
                subpath: Some("themes/dark".into()),
            }
        );
        assert_eq!(
            parse_install_target(
                "https://github.com/alice/kiseki-pack/tree/main/themes/sakura",
                false,
                false
            )
            .unwrap(),
            InstallTarget::GitHub {
                owner: "alice".into(),
                repo: "kiseki-pack".into(),
                git_ref: Some("main".into()),
                subpath: Some("themes/sakura".into()),
            }
        );
        assert!(matches!(
            parse_install_target("./pack.kiseki.tar.gz", false, false).unwrap(),
            InstallTarget::LocalArchive(_)
        ));
        assert!(matches!(
            parse_install_target("./my-theme", false, false).unwrap(),
            InstallTarget::LocalDir(_)
        ));
        assert!(parse_install_target("tokyo-midnight", true, true).is_err());
        assert!(parse_install_target("github:alice/repo#../escape", false, false).is_err());
    }

    #[test]
    fn archive_extractor_rejects_traversal_symlinks_and_bad_sha() {
        // 1. Symlink inside tar.gz must fail.
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_cksum();
        builder
            .append_link(&mut header, "evil-link", "/etc/passwd")
            .unwrap();
        let evil_bytes = builder.into_inner().unwrap().finish().unwrap();
        let dest = tempfile::tempdir().unwrap();
        assert!(extract_tar_gz_safely(&evil_bytes, None, None, None, dest.path()).is_err());

        // 2. SHA-256 mismatch must fail.
        let (good_bytes, _sha) = sample_pack_tarball("sha-check", "0.1.0");
        let dest2 = tempfile::tempdir().unwrap();
        assert!(
            extract_tar_gz_safely(&good_bytes, Some("0000"), None, None, dest2.path()).is_err()
        );
    }

    #[test]
    fn local_http_gallery_supports_etag_redirect_install_and_update() {
        let (tar_v1, sha_v1) = sample_pack_tarball("tokyo-night", "1.0.0");
        let (tar_v2, sha_v2) = sample_pack_tarball("tokyo-night", "1.1.0");

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let index_v1 = serde_json::to_vec(&GalleryIndex {
            schema_version: 1,
            generated_at: "2026-11-28T00:00:00Z".into(),
            themes: vec![sample_entry(
                "tokyo-night",
                "1.0.0",
                "11111111",
                format!("{base}/archive/v1.tar.gz"),
                Some(format!("{base}/dl/v1")),
                sha_v1.clone(),
                120,
                500,
            )],
        })
        .unwrap();

        let index_v2 = serde_json::to_vec(&GalleryIndex {
            schema_version: 1,
            generated_at: "2026-11-28T01:00:00Z".into(),
            themes: vec![sample_entry(
                "tokyo-night",
                "1.1.0",
                "22222222",
                format!("{base}/archive/v2.tar.gz"),
                None,
                sha_v2.clone(),
                150,
                700,
            )],
        })
        .unwrap();

        let switch_v2 = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let switch_v2_srv = switch_v2.clone();
        let tar_v1_srv = tar_v1.clone();
        let tar_v2_srv = tar_v2.clone();
        let base_srv = base.clone();

        let server = thread::spawn(move || {
            for stream in listener.incoming().take(6) {
                let Ok(mut stream) = stream else { break };
                let mut req_buf = [0u8; 4096];
                let n = stream.read(&mut req_buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&req_buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let path = first_line.split_whitespace().nth(1).unwrap_or("/");
                let is_v2 = switch_v2_srv.load(std::sync::atomic::Ordering::SeqCst);

                match path {
                    "/index.json" => {
                        let (etag, body) = if is_v2 {
                            ("\"v2\"", index_v2.as_slice())
                        } else {
                            ("\"v1\"", index_v1.as_slice())
                        };
                        if req
                            .to_ascii_lowercase()
                            .contains(&format!("if-none-match: {etag}"))
                        {
                            let _ = stream.write_all(b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n");
                        } else {
                            let header = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: {etag}\r\nContent-Length: {}\r\n\r\n",
                                body.len()
                            );
                            let _ = stream.write_all(header.as_bytes());
                            let _ = stream.write_all(body);
                        }
                    }
                    "/dl/v1" => {
                        let resp = format!(
                            "HTTP/1.1 302 Found\r\nLocation: {base_srv}/archive/v1.tar.gz\r\nContent-Length: 0\r\n\r\n"
                        );
                        let _ = stream.write_all(resp.as_bytes());
                    }
                    "/archive/v1.tar.gz" => {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                            tar_v1_srv.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(&tar_v1_srv);
                    }
                    "/archive/v2.tar.gz" => {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                            tar_v2_srv.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(&tar_v2_srv);
                    }
                    _ => {
                        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    }
                }
            }
        });

        let root = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(root.path());
        let cfg = GalleryConfig {
            sources: vec![format!("{base}/index.json")],
            github_proxy: None,
            cache_ttl_secs: 600,
        };
        let client = GalleryClient::from_config(&cfg, platform.paths().gallery_cache_dir());

        // 1. Fresh load -> 200 OK.
        let first = client.load_index(false).unwrap();
        assert!(!first.from_cache);
        assert_eq!(first.index.themes.len(), 1);

        // 2. Force refresh with matching ETag -> 304 Not Modified.
        let second = client.load_index(true).unwrap();
        assert!(second.from_cache);

        // 3. Install via gallery short ID (follows 302 trackUrl -> archive/v1.tar.gz).
        let report = client
            .install_target(&platform, "tokyo-night", false, false, false, false)
            .unwrap();
        assert_eq!(report.id, "tokyo-night");
        assert_eq!(report.source_kind, "gallery");
        assert_eq!(
            report.origin.as_ref().unwrap().version.as_deref(),
            Some("1.0.0")
        );

        // 4. Switch server to v2 and run `update_installed`.
        switch_v2.store(true, std::sync::atomic::Ordering::SeqCst);
        let outcomes = client.update_installed(&platform, None, false).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, "updated");
        assert_eq!(outcomes[0].from_version.as_deref(), Some("1.0.0"));
        assert_eq!(outcomes[0].to_version.as_deref(), Some("1.1.0"));

        let origins = OriginRegistry::load(&platform, platform.paths()).unwrap();
        assert_eq!(
            origins.themes["tokyo-night"].version.as_deref(),
            Some("1.1.0")
        );

        let _ = server.join();
    }

    #[test]
    fn sorting_orders_by_trending_downloads_and_stars() {
        let a = sample_entry("alpha", "1.0.0", "aaa", "u".into(), None, "s".into(), 50, 1000);
        let b = sample_entry("beta", "1.0.0", "bbb", "u".into(), None, "s".into(), 500, 100);
        let list = [a, b];

        let by_dl = filter_and_sort(
            &list,
            &GalleryFilter {
                sort: GallerySort::Downloads,
                ..Default::default()
            },
        );
        assert_eq!(by_dl[0].id, "alpha");

        let by_stars = filter_and_sort(
            &list,
            &GalleryFilter {
                sort: GallerySort::Stars,
                ..Default::default()
            },
        );
        assert_eq!(by_stars[0].id, "beta");
    }
}
