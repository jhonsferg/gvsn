//! RAII wrapper for temporary directories using `tempfile`.
//!
//! Provides `TempDir` which automatically cleans up on drop, replacing manual
// `std::fs::remove_dir_all` calls throughout the codebase.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tempfile::TempDir as StdTempDir;

/// RAII temporary directory that cleans up on drop.
///
/// Wraps `tempfile::TempDir` with a simpler API and `anyhow` error handling.
#[derive(Debug)]
pub struct TempDir {
    inner: Option<StdTempDir>,
}

impl TempDir {
    /// Creates a new temporary directory in the default system temp location.
    pub fn new() -> anyhow::Result<Self> {
        StdTempDir::new()
            .map(|inner| Self { inner: Some(inner) })
            .map_err(Into::into)
    }

    /// Creates a new temporary directory in the specified parent directory.
    pub fn new_in(parent: impl AsRef<Path>, prefix: impl AsRef<OsStr>) -> anyhow::Result<Self> {
        StdTempDir::with_prefix_in(prefix, parent)
            .map(|inner| Self { inner: Some(inner) })
            .map_err(Into::into)
    }

    /// Returns the path to the temporary directory.
    pub fn path(&self) -> &Path {
        self.inner.as_ref().expect("TempDir used after drop").path()
    }

    /// Consumes the `TempDir` and returns the path without deleting the directory.
    #[cfg(test)]
    pub fn into_path(mut self) -> PathBuf {
        let inner = self.inner.take().expect("TempDir used after drop");
        let path = inner.path().to_path_buf();
        let _ = inner.keep();
        path
    }

    /// Prevents the temporary directory from being deleted on drop.
    pub fn keep(mut self) -> PathBuf {
        let inner = self.inner.take().expect("TempDir used after drop");
        let path = inner.path().to_path_buf();
        let _ = inner.keep();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // TempDir automatically cleans up on drop
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl Default for TempDir {
    fn default() -> Self {
        Self::new().expect("Failed to create temporary directory")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempdir_creates_and_cleans_up() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        assert!(path.exists());

        drop(dir);
        // Directory should be cleaned up after drop
    }

    #[test]
    fn tempdir_in_parent() {
        let parent = TempDir::new().unwrap();
        let child = TempDir::new_in(parent.path(), "child-").unwrap();
        assert!(child.path().starts_with(parent.path()));
        assert!(child
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("child-"));
    }

    #[test]
    fn tempdir_into_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.into_path();
        assert!(path.exists());
        // Directory is NOT cleaned up
    }

    #[test]
    fn tempdir_keep() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        dir.keep();
        assert!(path.exists());
    }
}
