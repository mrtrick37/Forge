//! Single-flight lock for NVIDIA akmods builds.
//!
//! The returned `File` owns the lock; dropping it releases the kernel lock.
//! No build command is started here.

use rustix::fs::{flock, FlockOperation};
use rustix::io::Errno;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DEFAULT_LOCK_PATH: &str = "/run/kyth-akmods.lock";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(900);

pub fn lock_path(override_path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = override_path {
        return path.as_ref().to_path_buf();
    }
    std::env::var_os("KYTH_AKMODS_LOCK")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_LOCK_PATH))
}

fn busy(error: Errno) -> bool {
    error == Errno::AGAIN || error == Errno::WOULDBLOCK
}

pub fn acquire(path: impl AsRef<Path>, timeout: Duration) -> std::io::Result<File> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)?;
    let deadline = Instant::now() + timeout;
    loop {
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => return Ok(file),
            Err(error) if busy(error) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(200))
            }
            Err(error) if busy(error) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "another NVIDIA module build is already running; wait for it to finish",
                ))
            }
            Err(error) => return Err(std::io::Error::from(error)),
        }
    }
}

pub fn build_in_progress(path: impl AsRef<Path>) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    match flock(&file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => {
            let _ = flock(&file, FlockOperation::NonBlockingUnlock);
            false
        }
        Err(error) => busy(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn lock_is_single_flight_and_drop_releases_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("akmods.lock");
        let first = acquire(&path, Duration::from_millis(20)).unwrap();
        assert!(build_in_progress(&path));
        assert!(acquire(&path, Duration::from_millis(10)).is_err());
        drop(first);
        // flock releases synchronously, but a busy CI runner can briefly
        // delay the probe's next open/lock attempt. Keep the assertion
        // bounded so this tests eventual release without hiding a stuck lock.
        let deadline = Instant::now() + Duration::from_secs(1);
        while build_in_progress(&path) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!build_in_progress(&path));
    }
}
