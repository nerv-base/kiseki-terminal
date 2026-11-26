//! The `Platform` trait plus its real and mock implementations.
//!
//! Every side effect in Kiseki goes through this trait: reading and writing
//! files, resolving symlinks, locking, running helper binaries, probing the
//! clock and checking whether a PID is alive. That is what lets the transaction
//! engine be exercised end-to-end in tests without touching the developer's
//! dotfiles.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kiseki_types::{KisekiError, OsKind, Result};

use crate::fs::{self as fsutil, list_dir, read_bytes, read_to_string, PathKind, WriteOutcome};
use crate::lock::{Lock, DEFAULT_TIMEOUT};
use crate::paths::KisekiPaths;

/// Captured result of running a helper binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status == 0
    }

    pub fn new(status: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            status,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }
}

/// Everything the engine needs from the host.
pub trait Platform: Send + Sync {
    fn os(&self) -> OsKind;
    fn paths(&self) -> &KisekiPaths;
    fn env_var(&self, key: &str) -> Option<String>;

    fn read_to_string(&self, path: &Path) -> Result<Option<String>>;
    fn read_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>>;
    fn inspect(&self, path: &Path) -> PathKind;
    fn write_preserving_symlinks(&self, path: &Path, contents: &[u8]) -> Result<WriteOutcome>;
    fn remove_file(&self, path: &Path) -> Result<()>;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>>;
    fn exists(&self, path: &Path) -> bool;

    /// Acquires the engine-wide lock. Defaults to the on-disk implementation.
    fn acquire_lock(&self, path: &Path, timeout: Duration) -> Result<Lock> {
        Lock::acquire(path, timeout)
    }

    /// Runs a helper binary (`ghostty +validate-config`, `pkill -USR2`, ...).
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput>;

    /// Finds an executable on `PATH` without running it.
    fn which(&self, program: &str) -> Option<PathBuf>;

    /// True when a PID is still alive; used to reap crashed preview sessions.
    fn process_alive(&self, pid: u32) -> bool;

    /// RFC-3339 UTC timestamp with second precision.
    fn now_rfc3339(&self) -> String;
}

/// The real host.
#[derive(Debug, Clone)]
pub struct SystemPlatform {
    paths: KisekiPaths,
}

impl SystemPlatform {
    pub fn new(paths: KisekiPaths) -> Self {
        Self { paths }
    }

    /// Detects the host layout from the real environment.
    pub fn detect() -> Self {
        let os = OsKind::current();
        let paths = KisekiPaths::from_env(os, &|key| std::env::var(key).ok());
        Self::new(paths)
    }
}

impl Platform for SystemPlatform {
    fn os(&self) -> OsKind {
        OsKind::current()
    }

    fn paths(&self) -> &KisekiPaths {
        &self.paths
    }

    fn env_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn read_to_string(&self, path: &Path) -> Result<Option<String>> {
        read_to_string(path)
    }

    fn read_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        read_bytes(path)
    }

    fn inspect(&self, path: &Path) -> PathKind {
        fsutil::inspect_path(path)
    }

    fn write_preserving_symlinks(&self, path: &Path, contents: &[u8]) -> Result<WriteOutcome> {
        fsutil::write_preserving_symlinks(path, contents)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        fsutil::remove_file(path)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path).map_err(|error| KisekiError::io(path, error))
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        list_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|error| KisekiError::io(program, error))?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn which(&self, program: &str) -> Option<PathBuf> {
        if let Some(path) = self.env_var("PATH") {
            let separator = if cfg!(windows) { ';' } else { ':' };
            for dir in path.split(separator).filter(|dir| !dir.is_empty()) {
                let candidate = Path::new(dir).join(program);
                if is_executable(&candidate) {
                    return Some(candidate);
                }
                if cfg!(windows) {
                    let exe = Path::new(dir).join(format!("{program}.exe"));
                    if is_executable(&exe) {
                        return Some(exe);
                    }
                }
            }
        }
        // On macOS, Ghostty is typically installed as an `.app` bundle without
        // symlinking its CLI binary onto PATH.
        if cfg!(target_os = "macos") && program == "ghostty" {
            let mut bundles = vec![PathBuf::from(
                "/Applications/Ghostty.app/Contents/MacOS/ghostty",
            )];
            if let Some(home) = self.env_var("HOME") {
                bundles.push(
                    PathBuf::from(home)
                        .join("Applications")
                        .join("Ghostty.app")
                        .join("Contents")
                        .join("MacOS")
                        .join("ghostty"),
                );
            }
            for candidate in bundles {
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn process_alive(&self, pid: u32) -> bool {
        if cfg!(target_os = "linux") {
            return Path::new("/proc").join(pid.to_string()).exists();
        }
        if cfg!(target_os = "windows") {
            let output = std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/NH"])
                .output();
            return output
                .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
                .unwrap_or(false);
        }
        // `kill -0` never delivers a signal; it only probes existence.
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn now_rfc3339(&self) -> String {
        format_rfc3339(SystemTime::now())
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// A platform backed by a real temporary directory that records every command.
///
/// Using a real filesystem (rather than an in-memory fake) means symlink,
/// permission and atomic-rename behaviour under test is exactly the behaviour
/// users get.
#[derive(Debug)]
pub struct MockPlatform {
    paths: KisekiPaths,
    env: Mutex<BTreeMap<String, String>>,
    commands: Mutex<Vec<RecordedCommand>>,
    command_outputs: Mutex<HashMap<String, CommandOutput>>,
    now: Mutex<String>,
    alive_pids: Mutex<Vec<u32>>,
    os: Mutex<Option<OsKind>>,
}

/// One command invocation observed by [`MockPlatform`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl RecordedCommand {
    pub fn matches(&self, program: &str, args: &[&str]) -> bool {
        self.program == program && self.args.iter().map(String::as_str).eq(args.iter().copied())
    }
}

impl MockPlatform {
    /// Creates a mock rooted at `root` with `config/`, `cache/` and `data/`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let paths = KisekiPaths::new(root.join("config"), root.join("cache"), root.join("data"));
        Self {
            paths,
            env: Mutex::new(BTreeMap::new()),
            commands: Mutex::new(Vec::new()),
            command_outputs: Mutex::new(HashMap::new()),
            now: Mutex::new("2026-11-26T09:15:00Z".to_string()),
            alive_pids: Mutex::new(Vec::new()),
            os: Mutex::new(None),
        }
    }

    pub fn with_paths(paths: KisekiPaths) -> Self {
        Self {
            paths,
            env: Mutex::new(BTreeMap::new()),
            commands: Mutex::new(Vec::new()),
            command_outputs: Mutex::new(HashMap::new()),
            now: Mutex::new("2026-11-26T09:15:00Z".to_string()),
            alive_pids: Mutex::new(Vec::new()),
            os: Mutex::new(None),
        }
    }

    /// Overrides the reported OS so host-specific paths and gates are testable.
    pub fn set_os(&self, os: OsKind) -> &Self {
        *self.os.lock().unwrap() = Some(os);
        self
    }

    pub fn set_env(&self, key: &str, value: &str) -> &Self {
        self.env.lock().unwrap().insert(key.to_string(), value.to_string());
        self
    }

    pub fn set_now(&self, value: &str) -> &Self {
        *self.now.lock().unwrap() = value.to_string();
        self
    }

    /// Registers the result of a command. The key is `program` alone.
    pub fn set_command_output(&self, program: &str, output: CommandOutput) -> &Self {
        self.command_outputs
            .lock()
            .unwrap()
            .insert(program.to_string(), output);
        self
    }

    pub fn set_pid_alive(&self, pid: u32, alive: bool) -> &Self {
        let mut pids = self.alive_pids.lock().unwrap();
        pids.retain(|existing| *existing != pid);
        if alive {
            pids.push(pid);
        }
        self
    }

    pub fn commands(&self) -> Vec<RecordedCommand> {
        self.commands.lock().unwrap().clone()
    }

    pub fn command_count(&self) -> usize {
        self.commands.lock().unwrap().len()
    }

    pub fn clear_commands(&self) {
        self.commands.lock().unwrap().clear();
    }
}

impl Platform for MockPlatform {
    fn os(&self) -> OsKind {
        self.os.lock().unwrap().unwrap_or_else(OsKind::current)
    }

    fn paths(&self) -> &KisekiPaths {
        &self.paths
    }

    fn env_var(&self, key: &str) -> Option<String> {
        self.env.lock().unwrap().get(key).cloned()
    }

    fn read_to_string(&self, path: &Path) -> Result<Option<String>> {
        read_to_string(path)
    }

    fn read_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        read_bytes(path)
    }

    fn inspect(&self, path: &Path) -> PathKind {
        fsutil::inspect_path(path)
    }

    fn write_preserving_symlinks(&self, path: &Path, contents: &[u8]) -> Result<WriteOutcome> {
        fsutil::write_preserving_symlinks(path, contents)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        fsutil::remove_file(path)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path).map_err(|error| KisekiError::io(path, error))
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        list_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        self.commands.lock().unwrap().push(RecordedCommand {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        });
        Ok(self
            .command_outputs
            .lock()
            .unwrap()
            .get(program)
            .cloned()
            .unwrap_or_else(|| CommandOutput::new(0, "", "")))
    }

    fn which(&self, program: &str) -> Option<PathBuf> {
        self.env_var(&format!("KISEKI_WHICH_{program}"))
            .map(PathBuf::from)
    }

    fn process_alive(&self, pid: u32) -> bool {
        self.alive_pids.lock().unwrap().contains(&pid)
    }

    fn now_rfc3339(&self) -> String {
        self.now.lock().unwrap().clone()
    }
}

impl Default for MockPlatform {
    fn default() -> Self {
        let dir = std::env::temp_dir().join(format!("kiseki-mock-{}", std::process::id()));
        Self::new(dir)
    }
}

/// Formats a `SystemTime` as `YYYY-MM-DDTHH:MM:SSZ` without external crates.
pub fn format_rfc3339(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's civil-from-days algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

/// Default lock timeout, re-exported for callers that do not care to choose.
pub const LOCK_TIMEOUT: Duration = DEFAULT_TIMEOUT;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn mock_platform_uses_real_filesystem_but_records_commands() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        platform.set_env("TERM_PROGRAM", "ghostty");
        platform.set_command_output("ghostty", CommandOutput::new(0, "ok", ""));
        platform.set_now("2026-11-26T09:15:00Z");

        assert_eq!(platform.env_var("TERM_PROGRAM").as_deref(), Some("ghostty"));
        let file = platform.paths().generated("x.txt");
        platform.write_preserving_symlinks(&file, b"hi").unwrap();
        assert_eq!(platform.read_to_string(&file).unwrap().as_deref(), Some("hi"));

        let output = platform.run("ghostty", &["+version"]).unwrap();
        assert!(output.success());
        assert_eq!(platform.command_count(), 1);
        assert!(platform.commands()[0].matches("ghostty", &["+version"]));
        assert_eq!(platform.now_rfc3339(), "2026-11-26T09:15:00Z");
    }

    #[test]
    fn mock_pid_liveness_is_controllable() {
        let platform = MockPlatform::new(tempfile::tempdir().unwrap().path());
        assert!(!platform.process_alive(42));
        platform.set_pid_alive(42, true);
        assert!(platform.process_alive(42));
        platform.set_pid_alive(42, false);
        assert!(!platform.process_alive(42));
    }

    #[test]
    fn mock_which_is_opt_in() {
        let platform = MockPlatform::new(tempfile::tempdir().unwrap().path());
        assert_eq!(platform.which("ghostty"), None);
        platform.set_env("KISEKI_WHICH_ghostty", "/opt/homebrew/bin/ghostty");
        assert_eq!(
            platform.which("ghostty"),
            Some(PathBuf::from("/opt/homebrew/bin/ghostty"))
        );
    }

    #[test]
    fn rfc3339_formatting_is_correct_for_known_instants() {
        assert_eq!(format_rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            format_rfc3339(UNIX_EPOCH + Duration::from_secs(1_764_148_500)),
            "2025-11-26T09:15:00Z"
        );
        // Leap day.
        assert_eq!(
            format_rfc3339(UNIX_EPOCH + Duration::from_secs(1_709_164_800)),
            "2024-02-29T00:00:00Z"
        );
    }
}
