//! Cross-process advisory lock (RFC-0001 §4).
//!
//! A lock file created with `create_new` is the portable primitive: it is
//! atomic on every filesystem we support, survives a crashed process (the file
//! stays behind), and is trivially inspectable by `doctor`.
//!
//! Stale locks are reclaimed after a grace period so a SIGKILLed `kiseki`
//! cannot permanently wedge the user's terminal.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use kiseki_types::{KisekiError, Result};

/// Default time a caller waits for the lock before reporting the owner.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// A held lock. Dropping it releases the lock.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    released: bool,
}

impl Lock {
    /// Acquires `path` exclusively, retrying until `timeout` elapses.
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| KisekiError::io(parent, error))?;
        }

        let deadline = Instant::now() + timeout;
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(mut file) => {
                    let _ = writeln!(file, "pid={}", std::process::id());
                    return Ok(Self {
                        path: path.to_path_buf(),
                        released: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if is_stale(path) {
                        let _ = fs::remove_file(path);
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(KisekiError::Locked(path.to_path_buf()));
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(KisekiError::io(path, error)),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Releases the lock early. Idempotent.
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if !self.released {
            let _ = fs::remove_file(&self.path);
            self.released = true;
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        self.release_inner();
    }
}

/// True when a lock file is old enough to be considered abandoned.
///
/// Five minutes is comfortably longer than any legitimate operation and short
/// enough that a crashed session heals itself without user intervention.
fn is_stale(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age > Duration::from_secs(300))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn second_acquire_times_out_with_owner_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lock");
        let first = Lock::acquire(&path, Duration::from_millis(50)).unwrap();
        assert!(path.exists());

        let error = Lock::acquire(&path, Duration::from_millis(50)).unwrap_err();
        assert!(matches!(error, KisekiError::Locked(_)));
        assert!(error.to_string().contains(".lock"));

        first.release();
        assert!(!path.exists());
        let _second = Lock::acquire(&path, Duration::from_millis(50)).unwrap();
    }

    #[test]
    fn stale_lock_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lock");
        fs::write(&path, "pid=999999\n").unwrap();
        // Backdate the file by two hours.
        let old = SystemTime::now() - Duration::from_secs(7200);
        set_modified(&path, old);
        let lock = Lock::acquire(&path, Duration::from_millis(50)).unwrap();
        assert!(path.exists());
        drop(lock);
        assert!(!path.exists());
    }

    fn set_modified(path: &Path, when: SystemTime) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(when).unwrap();
    }
}
