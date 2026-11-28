//! Exhaustive integration tests for `kiseki deps install` and `kiseki fonts install`
//! backed by a deterministic, multi-threaded local HTTP mock server.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn kiseki_bin() -> &'static str {
    env!("CARGO_BIN_EXE_kiseki")
}

fn sha256_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut h = Sha256::new();
    h.update(data);
    let digest = h.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for &b in digest.as_slice() {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            writer.start_file(*name, opts).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    cursor.into_inner()
}

fn mock_executable_bytes(version_line: &str) -> Vec<u8> {
    #[cfg(unix)]
    {
        format!("#!/bin/sh\necho \"{version_line}\"\n").into_bytes()
    }
    #[cfg(windows)]
    {
        let _ = version_line;
        fs::read(kiseki_bin()).unwrap()
    }
}

fn current_dep_platform() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "darwin-arm64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "darwin-amd64"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux-arm64"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-amd64"
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "windows-arm64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-amd64"
    }
}

fn tool_suffix() -> &'static str {
    if cfg!(windows) {
        ".exe"
    } else {
        ""
    }
}

#[derive(Clone)]
enum MockRoute {
    /// Serve exact bytes with optional per-request delay.
    Bytes { body: Vec<u8>, delay_ms: u64 },
    /// Return a fixed HTTP status error (e.g. 500 / 404).
    Status(u16),
    /// Accept TCP connection, send nothing and sleep for `stall_ms` to simulate a hung mirror.
    Stall { stall_ms: u64 },
    /// On the first non-Range request, announce full Content-Length, write `cut_at`
    /// bytes and abruptly close TCP; on subsequent `Range:` requests serve 206 Partial Content.
    DropThenResume {
        body: Vec<u8>,
        cut_at: usize,
        dropped_once: Arc<AtomicBool>,
    },
}

struct MockServerStats {
    requests: Mutex<Vec<String>>,
    in_flight: AtomicUsize,
    peak_in_flight: AtomicUsize,
    range_requests: AtomicUsize,
}

impl MockServerStats {
    fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            in_flight: AtomicUsize::new(0),
            peak_in_flight: AtomicUsize::new(0),
            range_requests: AtomicUsize::new(0),
        }
    }

    fn dep_request_count(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.starts_with("/deps/") && !p.starts_with("/deps/licenses/"))
            .count()
    }
}

struct MockHttpServer {
    base_url: String,
    stats: Arc<MockServerStats>,
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockHttpServer {
    fn start(routes: HashMap<String, MockRoute>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();

        let routes = Arc::new(routes);
        let stats = Arc::new(MockServerStats::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let routes_bg = Arc::clone(&routes);
        let stats_bg = Arc::clone(&stats);
        let shutdown_bg = Arc::clone(&shutdown);

        let handle = thread::spawn(move || {
            let mut conns = Vec::new();
            while !shutdown_bg.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                        let r = Arc::clone(&routes_bg);
                        let s = Arc::clone(&stats_bg);
                        conns.push(thread::spawn(move || handle_connection(stream, &r, &s)));
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
            for c in conns {
                let _ = c.join();
            }
        });

        Self {
            base_url: format!("http://{addr}"),
            stats,
            shutdown,
            handle: Some(handle),
        }
    }
}

impl Drop for MockHttpServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn handle_connection(
    mut stream: std::net::TcpStream,
    routes: &HashMap<String, MockRoute>,
    stats: &MockServerStats,
) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut req_line = String::new();
    if reader.read_line(&mut req_line).unwrap_or(0) == 0 {
        return;
    }
    let path = req_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();

    let mut range_start: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            if k.eq_ignore_ascii_case("range") {
                if let Some(rest) = v.trim().strip_prefix("bytes=") {
                    if let Some((start_str, _)) = rest.split_once('-') {
                        range_start = start_str.trim().parse::<usize>().ok();
                    }
                }
            }
        }
    }

    stats.requests.lock().unwrap().push(path.clone());
    if range_start.is_some() {
        stats.range_requests.fetch_add(1, Ordering::SeqCst);
    }

    let cur = stats.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    let mut peak = stats.peak_in_flight.load(Ordering::SeqCst);
    while cur > peak {
        match stats
            .peak_in_flight
            .compare_exchange(peak, cur, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => break,
            Err(actual) => peak = actual,
        }
    }

    match routes.get(&path) {
        Some(MockRoute::Bytes { body, delay_ms }) => {
            if *delay_ms > 0 {
                thread::sleep(Duration::from_millis(*delay_ms));
            }
            if let Some(start) = range_start {
                if start < body.len() {
                    let slice = &body[start..];
                    let end = body.len() - 1;
                    let header = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        start,
                        end,
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(slice);
                } else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            } else {
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
            }
        }
        Some(MockRoute::Status(code)) => {
            let header = format!(
                "HTTP/1.1 {code} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(header.as_bytes());
        }
        Some(MockRoute::Stall { stall_ms }) => {
            thread::sleep(Duration::from_millis(*stall_ms));
        }
        Some(MockRoute::DropThenResume {
            body,
            cut_at,
            dropped_once,
        }) => {
            if range_start.is_none() && !dropped_once.swap(true, Ordering::SeqCst) {
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let end = (*cut_at).min(body.len());
                let _ = stream.write_all(&body[..end]);
                let _ = stream.flush();
                // Abruptly close socket mid-stream without sending remaining bytes.
            } else {
                let start = range_start.unwrap_or(0).min(body.len());
                let slice = &body[start..];
                let header = if range_start.is_some() && start < body.len() {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        start,
                        body.len() - 1,
                        body.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                };
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(slice);
            }
        }
        None => {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    }

    let _ = stream.flush();
    stats.in_flight.fetch_sub(1, Ordering::SeqCst);
}

struct TestEnv {
    temp: TempDir,
    kiseki_home: PathBuf,
    install_dir: PathBuf,
    font_dir: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let kiseki_home = temp.path().join("kiseki-home");
        let install_dir = temp.path().join("install-bin");
        let font_dir = temp.path().join("fonts");
        fs::create_dir_all(&kiseki_home).unwrap();
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&font_dir).unwrap();
        Self {
            temp,
            kiseki_home,
            install_dir,
            font_dir,
        }
    }

    fn vendor_bin_dir(&self) -> PathBuf {
        if cfg!(windows) {
            self.install_dir.join("vendor").join("bin")
        } else {
            self.kiseki_home.join("vendor").join("bin")
        }
    }

    fn vendor_root(&self) -> PathBuf {
        if cfg!(windows) {
            self.install_dir.join("vendor")
        } else {
            self.kiseki_home.join("vendor")
        }
    }

    fn cmd(&self, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(kiseki_bin());
        command
            .args(args)
            .env("HOME", self.temp.path())
            .env("USERPROFILE", self.temp.path())
            .env("KISEKI_HOME", &self.kiseki_home)
            .env("KISEKI_INSTALL_DIR", &self.install_dir)
            .env("KISEKI_FONT_DIR", &self.font_dir)
            .env("KISEKI_DEP_PLATFORM", current_dep_platform())
            .env("NO_COLOR", "1");
        for (k, v) in extra_env {
            command.env(k, v);
        }
        command.output().unwrap()
    }

    fn run_json(&self, args: &[&str], extra_env: &[(&str, &str)]) -> (bool, Value) {
        let out = self.cmd(args, extra_env);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let val: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "invalid JSON from `{args:?}` (status={:?}): {e}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                out.status
            )
        });
        (out.status.success(), val)
    }
}

struct MockAssets {
    omp_bytes: Vec<u8>,
    omp_sha: String,
    ff_zip_bytes: Vec<u8>,
    ff_sha: String,
    jb_bytes: Vec<u8>,
    jb_sha: String,
    dep_zip_bytes: Vec<u8>,
    dep_sha: String,
    maple_zip_bytes: Vec<u8>,
    maple_sha: String,
    deps_tsv: String,
    checksums_txt: String,
}

impl MockAssets {
    fn build(upstream_base: &str) -> Self {
        let plat = current_dep_platform();
        let suffix = tool_suffix();

        let omp_bytes = mock_executable_bytes("oh-my-posh 33.4.1");
        let omp_sha = sha256_hex(&omp_bytes);

        let ff_bin = mock_executable_bytes("fastfetch 2.71.0");
        let ff_entry = if cfg!(windows) {
            "fastfetch.exe"
        } else {
            "fastfetch-macos/usr/bin/fastfetch"
        };
        let ff_zip_bytes = make_zip(&[(ff_entry, &ff_bin)]);
        let ff_sha = sha256_hex(&ff_zip_bytes);

        let jb_bytes = b"FAKE-TTF-JETBRAINS-REGULAR-PAYLOAD".to_vec();
        let jb_sha = sha256_hex(&jb_bytes);

        let dep_zip_bytes = make_zip(&[
            ("DepartureMono/DepartureMonoNerdFont-Regular.otf", b"FAKE-OTF-DEPARTURE"),
            ("DepartureMono/README.md", b"ignore me"),
        ]);
        let dep_sha = sha256_hex(&dep_zip_bytes);

        // Include non-matching weights & a Zip-Slip entry to test filtering + defense.
        let maple_zip_bytes = make_zip(&[
            ("MapleMono-NF-CN-Regular.ttf", b"FAKE-TTF-MAPLE-REGULAR"),
            ("MapleMono-NF-CN-Medium.ttf", b"FAKE-TTF-MAPLE-MEDIUM"),
            ("MapleMono-NF-CN-Bold.ttf", b"FAKE-TTF-MAPLE-BOLD"),
            ("MapleMono-NF-CN-Italic.ttf", b"FAKE-TTF-MAPLE-ITALIC"),
            ("MapleMono-NF-CN-ExtraLight.ttf", b"SHOULD-BE-SKIPPED"),
            ("../../evil-Regular.ttf", b"SHOULD-BE-REJECTED"),
            ("README.txt", b"ignore me"),
        ]);
        let maple_sha = sha256_hex(&maple_zip_bytes);

        let deps_tsv = format!(
            "# kind\tplatform\turl\tsha256\tdest\tmode\n\
             # font-dir\tmacos\t~/Library/Fonts\n\
             # font-dir\tlinux\t~/.local/share/fonts/kiseki\n\
             # font-dir\twindows\t%LOCALAPPDATA%\\Microsoft\\Windows\\Fonts\n\
             # font\tdeparture-mono-nf\t\\.(otf|ttf)$\n\
             # font\tmaple-mono-nf-cn\t-(Regular|Medium|Bold|Italic)\\.ttf$\n\
             # font\tfusion-pixel-12-zh\t(zh_hans|ja)\\.ttf$\n\
             tool\t{plat}\t{upstream_base}/upstream/omp\t{omp_sha}\tvendor/bin/oh-my-posh{suffix}\t0755\n\
             tool\t{plat}\t{upstream_base}/upstream/ff.zip\t{ff_sha}\tvendor/bin/fastfetch{suffix}\t0755\n\
             font-raw\t{plat}\t{upstream_base}/upstream/jb.ttf\t{jb_sha}\tfonts/JetBrainsMonoNerdFont-Regular.ttf\t0644\n\
             font\t{plat}\t{upstream_base}/upstream/dep.zip\t{dep_sha}\tfonts/departure-mono-nf\t0644\n\
             font\t{plat}\t{upstream_base}/upstream/maple.zip\t{maple_sha}\tfonts/maple-mono-nf-cn\t0644\n"
        );
        let deps_sha = sha256_hex(deps_tsv.as_bytes());
        let checksums_txt = format!("{deps_sha}  deps.tsv\n");

        Self {
            omp_bytes,
            omp_sha,
            ff_zip_bytes,
            ff_sha,
            jb_bytes,
            jb_sha,
            dep_zip_bytes,
            dep_sha,
            maple_zip_bytes,
            maple_sha,
            deps_tsv,
            checksums_txt,
        }
    }

    fn routes(&self, tag: &str, delay_ms: u64) -> HashMap<String, MockRoute> {
        let mut r = HashMap::new();
        r.insert(
            format!("/releases/{tag}/checksums.txt"),
            MockRoute::Bytes {
                body: self.checksums_txt.as_bytes().to_vec(),
                delay_ms: 0,
            },
        );
        r.insert(
            format!("/releases/{tag}/deps.tsv"),
            MockRoute::Bytes {
                body: self.deps_tsv.as_bytes().to_vec(),
                delay_ms: 0,
            },
        );
        r.insert(
            format!("/deps/{}", self.omp_sha),
            MockRoute::Bytes {
                body: self.omp_bytes.clone(),
                delay_ms,
            },
        );
        r.insert(
            format!("/deps/{}", self.ff_sha),
            MockRoute::Bytes {
                body: self.ff_zip_bytes.clone(),
                delay_ms,
            },
        );
        r.insert(
            format!("/deps/{}", self.jb_sha),
            MockRoute::Bytes {
                body: self.jb_bytes.clone(),
                delay_ms,
            },
        );
        r.insert(
            format!("/deps/{}", self.dep_sha),
            MockRoute::Bytes {
                body: self.dep_zip_bytes.clone(),
                delay_ms,
            },
        );
        r.insert(
            format!("/deps/{}", self.maple_sha),
            MockRoute::Bytes {
                body: self.maple_zip_bytes.clone(),
                delay_ms,
            },
        );
        r.insert(
            "/deps/licenses/OFL-JetBrainsMono.txt".to_string(),
            MockRoute::Bytes {
                body: b"SIL OPEN FONT LICENSE Version 1.1\n".to_vec(),
                delay_ms: 0,
            },
        );
        r
    }
}

#[test]
fn parallel_download_installs_tools_and_fonts_concurrently_and_is_idempotent() {
    let tag = "v2.0.0-beta.1";
    let env = TestEnv::new();
    let assets = MockAssets::build("http://127.0.0.1:1");
    let server = MockHttpServer::start(assets.routes(tag, 150));

    // 1) Parallel cold install
    let (ok, report) = env.run_json(
        &[
            "deps",
            "install",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--concurrency",
            "4",
            "--json",
        ],
        &[],
    );
    assert!(ok, "expected success, got: {report:#}");
    assert_eq!(report["ok"], true);
    assert_eq!(report["summary"]["installed"], 5);
    assert_eq!(report["summary"]["upToDate"], 0);
    assert_eq!(report["summary"]["failed"], 0);

    // Proves true parallel execution across worker threads.
    let peak = server.stats.peak_in_flight.load(Ordering::SeqCst);
    assert!(
        peak >= 2,
        "expected concurrent in-flight HTTP downloads (>= 2), observed peak={peak}"
    );

    // Verify vendored tools exist & execute cleanly.
    let omp_dest = env.vendor_bin_dir().join(format!("oh-my-posh{}", tool_suffix()));
    let ff_dest = env.vendor_bin_dir().join(format!("fastfetch{}", tool_suffix()));
    assert!(omp_dest.is_file(), "oh-my-posh missing at {}", omp_dest.display());
    assert!(ff_dest.is_file(), "fastfetch missing at {}", ff_dest.display());

    // Verify raw font + ZIP fonts extracted ONLY matching faces & rejected Zip Slip.
    assert!(env.font_dir.join("JetBrainsMonoNerdFont-Regular.ttf").is_file());
    assert!(env.font_dir.join("DepartureMonoNerdFont-Regular.otf").is_file());
    assert!(!env.font_dir.join("README.md").exists());
    for weight in ["Regular", "Medium", "Bold", "Italic"] {
        assert!(
            env.font_dir
                .join(format!("MapleMono-NF-CN-{weight}.ttf"))
                .is_file(),
            "missing MapleMono weight {weight}"
        );
    }
    assert!(!env.font_dir.join("MapleMono-NF-CN-ExtraLight.ttf").exists());
    assert!(!env.temp.path().join("evil-Regular.ttf").exists());
    assert!(!env.font_dir.join("evil-Regular.ttf").exists());

    // Verify OFL license downloaded.
    let ofl = env
        .vendor_root()
        .join("licenses")
        .join("OFL-JetBrainsMono.txt");
    assert!(ofl.is_file(), "OFL license missing at {}", ofl.display());

    // 2) Rerun must skip ALL 5 items (including ZIP fonts via receipts!) with ZERO /deps/* downloads.
    let reqs_before = server.stats.dep_request_count();
    let (ok2, report2) = env.run_json(
        &[
            "deps",
            "install",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--json",
        ],
        &[],
    );
    assert!(ok2, "idempotent rerun failed: {report2:#}");
    assert_eq!(report2["summary"]["installed"], 0);
    assert_eq!(report2["summary"]["upToDate"], 5);
    assert_eq!(report2["summary"]["downloadedBytes"], 0);
    assert_eq!(
        server.stats.dep_request_count(),
        reqs_before,
        "idempotent rerun must not re-download any asset"
    );

    // 3) Deleting one extracted face from a ZIP font auto-heals ONLY that family!
    fs::remove_file(env.font_dir.join("MapleMono-NF-CN-Bold.ttf")).unwrap();
    let (ok3, report3) = env.run_json(
        &[
            "deps",
            "install",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--json",
        ],
        &[],
    );
    assert!(ok3, "healing rerun failed: {report3:#}");
    assert_eq!(report3["summary"]["installed"], 1);
    assert_eq!(report3["summary"]["upToDate"], 4);
    assert!(env.font_dir.join("MapleMono-NF-CN-Bold.ttf").is_file());

    // 4) --force re-downloads all 5 items.
    let (ok4, report4) = env.run_json(
        &[
            "deps",
            "install",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--force",
            "--json",
        ],
        &[],
    );
    assert!(ok4);
    assert_eq!(report4["summary"]["installed"], 5);
    assert_eq!(report4["summary"]["upToDate"], 0);

    // 5) `kiseki deps --json` sees the newly installed vendored tools immediately.
    let (ok5, deps_out) = env.run_json(&["deps", "--json"], &[]);
    assert!(ok5);
    let tools = deps_out["tools"].as_array().unwrap();
    let omp_probe = tools.iter().find(|t| t["tool_id"] == "oh-my-posh").unwrap();
    let ff_probe = tools.iter().find(|t| t["tool_id"] == "fastfetch").unwrap();
    assert_ne!(omp_probe["status"], "missing");
    assert_ne!(ff_probe["status"], "missing");
}

#[test]
fn mirror_failover_recovers_from_http_500_and_hung_mirror() {
    let tag = "v2.0.0-beta.1";
    let env = TestEnv::new();
    let assets = MockAssets::build("http://127.0.0.1:1");

    // Primary mirror: serves manifest OK, returns 500 for oh-my-posh, STALLS for maple-mono-nf-cn!
    let mut primary_routes = assets.routes(tag, 0);
    primary_routes.insert(format!("/deps/{}", assets.omp_sha), MockRoute::Status(500));
    primary_routes.insert(
        format!("/deps/{}", assets.maple_sha),
        MockRoute::Stall { stall_ms: 2500 },
    );
    let primary = MockHttpServer::start(primary_routes);

    // Secondary mirror: healthy for everything.
    let secondary = MockHttpServer::start(assets.routes(tag, 0));

    let dl_base = format!("{},{}", primary.base_url, secondary.base_url);
    let (ok, report) = env.run_json(
        &[
            "deps",
            "install",
            "--version",
            tag,
            "--dl-base",
            &dl_base,
            "--concurrency",
            "4",
            "--json",
        ],
        &[("KISEKI_TEST_IO_TIMEOUT_MS", "250")],
    );
    assert!(ok, "failover should succeed, got: {report:#}");
    assert_eq!(report["summary"]["installed"], 5);
    assert_eq!(report["summary"]["failed"], 0);
    assert!(env.font_dir.join("MapleMono-NF-CN-Regular.ttf").is_file());
    assert!(
        secondary.stats.dep_request_count() >= 2,
        "secondary mirror must serve failed & stalled items"
    );
}

#[test]
fn mid_stream_disconnect_resumes_via_http_range_and_verifies_sha256() {
    let tag = "v2.0.0-beta.1";
    let env = TestEnv::new();

    // Build a sizable (~320 KiB) compressible-resistant payload inside the maple ZIP.
    let mut big_face = vec![0u8; 320 * 1024];
    for (i, b) in big_face.iter_mut().enumerate() {
        *b = ((i.wrapping_mul(31) ^ (i >> 3)) & 0xff) as u8;
    }
    let big_zip = make_zip(&[
        ("MapleMono-NF-CN-Regular.ttf", &big_face),
        ("MapleMono-NF-CN-Bold.ttf", &big_face[..65536]),
    ]);
    let big_sha = sha256_hex(&big_zip);
    let plat = current_dep_platform();
    let deps_tsv = format!(
        "# kind\tplatform\turl\tsha256\tdest\tmode\n\
         # font-dir\tmacos\t~/Library/Fonts\n\
         # font-dir\tlinux\t~/.local/share/fonts/kiseki\n\
         # font-dir\twindows\t%LOCALAPPDATA%\\Microsoft\\Windows\\Fonts\n\
         # font\tmaple-mono-nf-cn\t-(Regular|Medium|Bold|Italic)\\.ttf$\n\
         font\t{plat}\thttp://127.0.0.1:1/upstream/maple.zip\t{big_sha}\tfonts/maple-mono-nf-cn\t0644\n"
    );
    let checksums_txt = format!("{}  deps.tsv\n", sha256_hex(deps_tsv.as_bytes()));

    let mut routes = HashMap::new();
    routes.insert(
        format!("/releases/{tag}/checksums.txt"),
        MockRoute::Bytes {
            body: checksums_txt.into_bytes(),
            delay_ms: 0,
        },
    );
    routes.insert(
        format!("/releases/{tag}/deps.tsv"),
        MockRoute::Bytes {
            body: deps_tsv.into_bytes(),
            delay_ms: 0,
        },
    );
    let cut_at = big_zip.len() / 2;
    routes.insert(
        format!("/deps/{big_sha}"),
        MockRoute::DropThenResume {
            body: big_zip,
            cut_at,
            dropped_once: Arc::new(AtomicBool::new(false)),
        },
    );
    let server = MockHttpServer::start(routes);

    let (ok, report) = env.run_json(
        &[
            "deps",
            "install",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--font",
            "maple-mono-nf-cn",
            "--json",
        ],
        &[],
    );
    assert!(ok, "Range resume should succeed, got: {report:#}");
    assert_eq!(report["summary"]["installed"], 1);
    assert!(
        server.stats.range_requests.load(Ordering::SeqCst) >= 1,
        "expected at least one HTTP Range resume request"
    );
    let installed_face = fs::read(env.font_dir.join("MapleMono-NF-CN-Regular.ttf")).unwrap();
    assert_eq!(installed_face, big_face);
}

#[test]
fn sha256_mismatch_rejects_corrupted_asset_and_preserves_existing_file() {
    let tag = "v2.0.0-beta.1";
    let env = TestEnv::new();
    let assets = MockAssets::build("http://127.0.0.1:1");

    // Pre-populate a valid existing font file.
    let existing = env.font_dir.join("JetBrainsMonoNerdFont-Regular.ttf");
    fs::write(&existing, b"ORIGINAL-GOOD-FONT").unwrap();

    // Serve corrupted bytes for JetBrainsMono on the mirror.
    let mut routes = assets.routes(tag, 0);
    routes.insert(
        format!("/deps/{}", assets.jb_sha),
        MockRoute::Bytes {
            body: b"CORRUPTED-MALICIOUS-BYTES".to_vec(),
            delay_ms: 0,
        },
    );
    let server = MockHttpServer::start(routes);

    let (ok, report) = env.run_json(
        &[
            "deps",
            "install",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--font",
            "jetbrains-mono-nf",
            "--force",
            "--json",
        ],
        &[],
    );
    assert!(!ok, "corrupted download must fail");
    assert_eq!(report["ok"], false);
    assert_eq!(report["summary"]["failed"], 1);
    assert!(report["items"][0]["error"]
        .as_str()
        .unwrap()
        .contains("SHA-256 mismatch"));
    // Existing file must be completely untouched.
    assert_eq!(fs::read(&existing).unwrap(), b"ORIGINAL-GOOD-FONT");
}

#[test]
fn filtering_flags_dry_run_fonts_install_and_rescue_mode_work() {
    let tag = "v2.0.0-beta.1";
    let env = TestEnv::new();
    let assets = MockAssets::build("http://127.0.0.1:1");
    let server = MockHttpServer::start(assets.routes(tag, 0));

    // Corrupt state.json to verify `deps install` works in rescue mode.
    fs::write(env.kiseki_home.join("state.json"), b"{not-valid-json").unwrap();

    // 1) --dry-run plans without downloading or writing files.
    let (ok_dry, dry) = env.run_json(
        &[
            "deps",
            "install",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--dry-run",
            "--json",
        ],
        &[],
    );
    assert!(ok_dry, "dry-run failed: {dry:#}");
    assert_eq!(dry["dryRun"], true);
    assert_eq!(dry["summary"]["planned"], 5);
    assert_eq!(server.stats.dep_request_count(), 0);
    assert!(!env.vendor_bin_dir().exists());

    // 2) --skip-fonts installs ONLY tools.
    let (ok_tools, tools_rep) = env.run_json(
        &[
            "deps",
            "install",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--skip-fonts",
            "--json",
        ],
        &[],
    );
    assert!(ok_tools, "--skip-fonts failed: {tools_rep:#}");
    assert_eq!(tools_rep["summary"]["installed"], 2);
    assert!(!env.font_dir.join("JetBrainsMonoNerdFont-Regular.ttf").exists());

    // 3) `kiseki fonts install jetbrains-mono-nf` installs ONLY that raw font.
    // Remove corrupt state.json first since `fonts` uses normal state loading.
    fs::remove_file(env.kiseki_home.join("state.json")).unwrap();
    let (ok_jb, jb_rep) = env.run_json(
        &[
            "fonts",
            "install",
            "jetbrains-mono-nf",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--json",
        ],
        &[],
    );
    assert!(ok_jb, "fonts install jetbrains-mono-nf failed: {jb_rep:#}");
    assert_eq!(jb_rep["summary"]["installed"], 1);
    assert!(env.font_dir.join("JetBrainsMonoNerdFont-Regular.ttf").is_file());
    assert!(!env.font_dir.join("DepartureMonoNerdFont-Regular.otf").exists());

    // 4) `kiseki fonts install all` installs remaining fonts and skips already-installed JetBrainsMono.
    let (ok_all, all_rep) = env.run_json(
        &[
            "fonts",
            "install",
            "all",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--json",
        ],
        &[],
    );
    assert!(ok_all, "fonts install all failed: {all_rep:#}");
    assert_eq!(all_rep["summary"]["installed"], 2);
    assert_eq!(all_rep["summary"]["upToDate"], 1);
    assert!(env.font_dir.join("DepartureMonoNerdFont-Regular.otf").is_file());
    assert!(env.font_dir.join("MapleMono-NF-CN-Regular.ttf").is_file());

    // 5) `kiseki fonts install theme` installs only the active theme's catalog fonts.
    let theme_env = TestEnv::new();
    let (ok_theme, theme_rep) = theme_env.run_json(
        &[
            "fonts",
            "install",
            "theme",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--json",
        ],
        &[],
    );
    assert!(ok_theme, "fonts install theme failed: {theme_rep:#}");
    assert!(
        theme_rep["summary"]["installed"].as_u64().unwrap() >= 1,
        "default theme should install at least one catalog font: {theme_rep:#}"
    );
    // Default theme (`nerv-eva`) should NOT install unrelated `departure-mono-nf`.
    assert!(!theme_env
        .font_dir
        .join("DepartureMonoNerdFont-Regular.otf")
        .exists());

    // 6) Unknown font ID returns a clear error.
    let (ok_bad, bad_rep) = env.run_json(
        &[
            "fonts",
            "install",
            "nonexistent-font",
            "--version",
            tag,
            "--dl-base",
            &server.base_url,
            "--json",
        ],
        &[],
    );
    assert!(!ok_bad);
    assert_eq!(bad_rep["ok"], false);
    assert!(bad_rep["error"]
        .as_str()
        .unwrap()
        .contains("unknown font ID"));
}

#[cfg(unix)]
#[test]
fn install_sh_two_stage_bootstrap_and_offline_rerun_work_end_to_end() {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let tag = "v2.0.0-beta.1";
    let env = TestEnv::new();
    let assets = MockAssets::build("http://127.0.0.1:1");

    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        other => panic!("unsupported test target: {other:?}"),
    };
    let archive_name = format!("kiseki-{tag}-{target}.tar.gz");

    // Package our real newly compiled `kiseki` binary into a release `.tar.gz`.
    let bin_bytes = fs::read(kiseki_bin()).unwrap();
    let mut tar_buf = Vec::new();
    {
        let enc = GzEncoder::new(&mut tar_buf, Compression::fast());
        let mut builder = tar::Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_size(bin_bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                format!("kiseki-{tag}-{target}/kiseki"),
                &bin_bytes[..],
            )
            .unwrap();
        let enc = builder.into_inner().unwrap();
        enc.finish().unwrap();
    }
    let archive_sha = sha256_hex(&tar_buf);
    let deps_sha = sha256_hex(assets.deps_tsv.as_bytes());
    let combined_checksums = format!("{archive_sha}  {archive_name}\n{deps_sha}  deps.tsv\n");

    let mut routes = assets.routes(tag, 50);
    routes.insert(
        "/releases/latest.json".to_string(),
        MockRoute::Bytes {
            body: format!("{{\"tag\":\"{tag}\"}}").into_bytes(),
            delay_ms: 0,
        },
    );
    routes.insert(
        format!("/releases/{tag}/checksums.txt"),
        MockRoute::Bytes {
            body: combined_checksums.into_bytes(),
            delay_ms: 0,
        },
    );
    routes.insert(
        format!("/releases/{tag}/{archive_name}"),
        MockRoute::Bytes {
            body: tar_buf,
            delay_ms: 0,
        },
    );
    let server = MockHttpServer::start(routes);

    let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("scripts/install.sh");

    let out = Command::new("sh")
        .arg(&script_path)
        .arg("--install-dir")
        .arg(&env.install_dir)
        .arg("--concurrency")
        .arg("4")
        .env("HOME", env.temp.path())
        .env("KISEKI_HOME", &env.kiseki_home)
        .env("KISEKI_DL_BASE", &server.base_url)
        .env("KISEKI_FONT_DIR", &env.font_dir)
        .env("KISEKI_DEP_PLATFORM", current_dep_platform())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "install.sh failed ({:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
    assert!(env.install_dir.join("kiseki").is_file());
    assert!(env.install_dir.join(".kiseki-release").is_file());
    assert!(env.vendor_bin_dir().join("oh-my-posh").is_file());
    assert!(env.vendor_bin_dir().join("fastfetch").is_file());
    assert!(env.font_dir.join("JetBrainsMonoNerdFont-Regular.ttf").is_file());
    assert!(env.font_dir.join("MapleMono-NF-CN-Regular.ttf").is_file());
    assert!(stdout.contains("Installed to"), "stdout:\n{stdout}");

    // Verify offline rerun reuses the verified `.kiseki-release` receipt cleanly.
    drop(server);
    let offline = Command::new("sh")
        .arg(&script_path)
        .arg("--install-dir")
        .arg(&env.install_dir)
        .env("HOME", env.temp.path())
        .env("KISEKI_HOME", &env.kiseki_home)
        .env("KISEKI_DL_BASE", "http://127.0.0.1:1")
        .env("KISEKI_FONT_DIR", &env.font_dir)
        .env("KISEKI_DEP_PLATFORM", current_dep_platform())
        .output()
        .unwrap();
    let off_stdout = String::from_utf8_lossy(&offline.stdout);
    let off_stderr = String::from_utf8_lossy(&offline.stderr);
    assert!(
        offline.status.success(),
        "offline rerun failed ({:?})\nstdout:\n{off_stdout}\nstderr:\n{off_stderr}",
        offline.status
    );
    assert!(
        off_stdout.contains("已保留安装验证过的 Kiseki"),
        "expected offline reuse message, got:\n{off_stdout}"
    );
}

#[test]
fn local_manifest_and_checksums_verification_rejects_tampered_manifest() {
    let env = TestEnv::new();
    let assets = MockAssets::build("http://127.0.0.1:1");
    let manifest_path = env.temp.path().join("deps.tsv");
    let checksums_path = env.temp.path().join("checksums.txt");

    fs::write(&manifest_path, &assets.deps_tsv).unwrap();
    fs::write(&checksums_path, &assets.checksums_txt).unwrap();

    // Valid local manifest + checksums works in dry-run with zero network.
    let (ok, rep) = env.run_json(
        &[
            "deps",
            "install",
            "--manifest",
            manifest_path.to_str().unwrap(),
            "--checksums",
            checksums_path.to_str().unwrap(),
            "--dry-run",
            "--json",
        ],
        &[],
    );
    assert!(ok, "valid local manifest failed: {rep:#}");
    assert_eq!(rep["summary"]["planned"], 5);

    // Tamper with manifest -> checksum verification MUST reject it.
    let mut tampered = assets.deps_tsv.clone();
    tampered.push_str("# tampered\n");
    fs::write(&manifest_path, tampered).unwrap();

    let (ok_bad, bad) = env.run_json(
        &[
            "deps",
            "install",
            "--manifest",
            manifest_path.to_str().unwrap(),
            "--checksums",
            checksums_path.to_str().unwrap(),
            "--dry-run",
            "--json",
        ],
        &[],
    );
    assert!(!ok_bad);
    assert!(bad["error"]
        .as_str()
        .unwrap()
        .contains("SHA-256 mismatch for deps.tsv"));
}

// Keep unused imports check clean across platforms.
#[allow(dead_code)]
fn _assert_path_helper(_p: &Path, _r: &mut dyn Read) {}
