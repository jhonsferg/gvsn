//! File locking utilities using fs2 for cross-platform advisory locks.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;

/// A cross-platform advisory file lock.
///
/// Uses `fs2` which provides `flock` on Unix and `LockFileEx` on Windows.
/// The lock is released when the `FileLock` is dropped.
pub struct FileLock {
    file: File,
}

impl FileLock {
    /// Acquires an exclusive lock on the given path.
    ///
    /// Creates the lock file if it doesn't exist. Blocks until the lock
    /// can be acquired.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or the lock cannot be acquired.
    pub fn lock(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .with_context(|| format!("Failed to open lock file {}", path.display()))?;

        file.lock_exclusive()
            .with_context(|| format!("Failed to acquire lock on {}", path.display()))?;

        Ok(Self { file })
    }

    /// Attempts to acquire an exclusive lock without blocking.
    ///
    /// Returns `Ok(Some(lock))` if the lock was acquired, `Ok(None)` if the
    /// lock is already held by another process, and `Err` on other errors.
    #[allow(dead_code)]
    pub fn try_lock(path: &Path) -> Result<Option<Self>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .with_context(|| format!("Failed to open lock file {}", path.display()))?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e).with_context(|| format!("Failed to try lock on {}", path.display())),
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Executes a closure while holding an exclusive lock on `path`.
///
/// The lock is automatically released when the closure returns (even on panic).
///
/// # Errors
///
/// Returns an error if the lock cannot be acquired or if the closure returns an error.
pub fn with_lock<F, T>(path: &Path, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let _lock = FileLock::lock(path)?;
    f()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn lock_prevents_concurrent_access() {
        let dir = tempdir().unwrap();
        let lock_path = dir.path().join("test.lock");

        let lock1 = FileLock::lock(&lock_path).unwrap();

        let lock_path_clone = lock_path.clone();
        let handle = thread::spawn(move || {
            // On Windows, try_lock returns an error when lock is held
            // On Unix, it returns Ok(None). Handle both.
            match FileLock::try_lock(&lock_path_clone) {
                Ok(None) => None,                                               // Unix: lock busy
                Ok(Some(_)) => Some(FileLock::lock(&lock_path_clone).unwrap()), // Got lock (shouldn't happen)
                Err(_) => None, // Windows: lock busy
            }
        });

        thread::sleep(Duration::from_millis(50));
        let result = handle.join().unwrap();
        assert!(result.is_none(), "Second lock should fail");

        drop(lock1);

        let lock2 = FileLock::try_lock(&lock_path).unwrap();
        assert!(
            lock2.is_some(),
            "Should acquire lock after first is released"
        );
    }

    #[test]
    fn with_lock_releases_on_panic() {
        let dir = tempdir().unwrap();
        let lock_path = dir.path().join("panic.lock");

        // Use catch_unwind to test panic behavior
        let result: Result<Result<(), _>, _> = std::panic::catch_unwind(|| {
            with_lock(&lock_path, || {
                panic!("intentional panic");
            })
        });

        assert!(result.is_err());

        // Lock should be released, so we can acquire it again
        let lock = FileLock::try_lock(&lock_path).unwrap();
        assert!(lock.is_some());
    }

    #[test]
    fn with_lock_works_normally() {
        let dir = tempdir().unwrap();
        let lock_path = dir.path().join("normal.lock");

        let result = with_lock(&lock_path, || Ok(42));

        assert_eq!(result.unwrap(), 42);
    }
}
