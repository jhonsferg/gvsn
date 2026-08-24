//! User configuration and directory layout.
//!
//! [`Config`] is the single source of truth for every path that `gvsn` reads
//! from or writes to. All other modules receive a `&Config` reference rather
//! than constructing paths themselves, which keeps the directory layout easy
//! to change in one place.
//!
//! The root directory defaults to `~/.gvsn` and can be overridden with the
//! `GVSN_DIR` environment variable.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Immutable configuration loaded at startup.
///
/// All path helpers are derived from the single [`root`](Config::root) field
/// so the entire directory tree moves atomically when `GVSN_DIR` is set.
#[derive(Debug)]
pub struct Config {
    /// Root directory for all gvsn data (default: `~/.gvsn`).
    pub root: PathBuf,
}

impl Config {
    /// Loads configuration from the environment.
    ///
    /// The root directory is taken from `GVSN_DIR` if set; otherwise it
    /// defaults to `~/.gvsn`.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined and `GVSN_DIR`
    /// is not set.
    pub fn load() -> Result<Self> {
        let root = match std::env::var("GVSN_DIR") {
            Ok(dir) => PathBuf::from(dir),
            Err(_) => dirs::home_dir()
                .context("Cannot determine home directory")?
                .join(".gvsn"),
        };
        Ok(Self { root })
    }

    /// Returns the directory that holds all installed Go versions.
    ///
    /// Layout: `<root>/versions/`
    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    /// Returns the staging directory used during downloads and extraction.
    ///
    /// Layout: `<root>/tmp/`
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Returns the path to the file that records the global default version.
    ///
    /// Layout: `<root>/version` (plain text, e.g. `go1.22.4`)
    pub fn version_file(&self) -> PathBuf {
        self.root.join("version")
    }

    /// Returns the installation directory for a specific Go version tag.
    ///
    /// Layout: `<root>/versions/<tag>/`  (e.g. `~/.gvsn/versions/go1.22.4/`)
    pub fn version_dir(&self, tag: &str) -> PathBuf {
        self.versions_dir().join(tag)
    }

    /// Returns the `bin/` subdirectory for a specific Go version tag.
    ///
    /// Layout: `<root>/versions/<tag>/bin/`
    pub fn version_bin_dir(&self, tag: &str) -> PathBuf {
        self.version_dir(tag).join("bin")
    }

    /// Returns the path to the `current` junction/symlink that always points to
    /// the active Go version directory.
    ///
    /// Adding `<root>/current/bin` to the OS PATH once (during install) makes
    /// `go` available in every shell - CMD, Git Bash, editors - without session
    /// hooks. `gvsn use` updates this link; the PATH entry never changes.
    ///
    /// Layout: `<root>/current` -> `<root>/versions/<active-tag>/`
    pub fn current_dir(&self) -> PathBuf {
        self.root.join("current")
    }

    /// Returns the path to the optional default-packages file.
    ///
    /// When this file exists, `gvsn install` reads it after a successful
    /// installation and runs `go install <pkg>` for each non-blank, non-comment
    /// line. This mirrors mise's `MISE_GO_DEFAULT_PACKAGES_FILE` feature.
    ///
    /// Layout: `<root>/default-packages`
    pub fn default_packages_file(&self) -> PathBuf {
        self.root.join("default-packages")
    }
}

/// Mutation operations on the config directory.
///
/// Separated from [`Config`] to follow the Interface Segregation Principle:
/// callers that only need path queries don't need mutation capabilities.
pub trait ConfigMut {
    /// Creates the [`versions_dir`](Config::versions_dir) and
    /// [`tmp_dir`](Config::tmp_dir) directories if they do not already exist.
    ///
    /// # Errors
    ///
    /// Returns an error if either directory cannot be created (e.g. permission
    /// denied).
    fn ensure_dirs(&self) -> anyhow::Result<()>;
}

impl ConfigMut for Config {
    fn ensure_dirs(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(self.versions_dir())
            .context("Failed to create versions directory")?;
        std::fs::create_dir_all(self.tmp_dir()).context("Failed to create tmp directory")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn config_load_uses_gvsn_dir() {
        let dir = tempdir().unwrap();
        std::env::set_var("GVSN_DIR", dir.path());
        let config = Config::load().unwrap();
        assert_eq!(config.root, dir.path());
        std::env::remove_var("GVSN_DIR");
    }

    #[test]
    fn config_load_defaults_to_home_gvsn() {
        std::env::remove_var("GVSN_DIR");
        let config = Config::load().unwrap();
        assert!(config.root.ends_with(".gvsn"));
    }

    #[test]
    fn config_paths_are_derived_from_root() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };

        assert_eq!(config.versions_dir(), dir.path().join("versions"));
        assert_eq!(config.tmp_dir(), dir.path().join("tmp"));
        assert_eq!(config.version_file(), dir.path().join("version"));
        assert_eq!(
            config.version_dir("go1.22.4"),
            dir.path().join("versions/go1.22.4")
        );
        assert_eq!(
            config.version_bin_dir("go1.22.4"),
            dir.path().join("versions/go1.22.4/bin")
        );
        assert_eq!(config.current_dir(), dir.path().join("current"));
        assert_eq!(
            config.default_packages_file(),
            dir.path().join("default-packages")
        );
    }

    #[test]
    fn config_ensure_dirs_creates_directories() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };

        config.ensure_dirs().unwrap();

        assert!(dir.path().join("versions").exists());
        assert!(dir.path().join("tmp").exists());
    }

    #[test]
    fn config_ensure_dirs_is_idempotent() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };

        config.ensure_dirs().unwrap();
        config.ensure_dirs().unwrap(); // Should not fail on second call

        assert!(dir.path().join("versions").exists());
        assert!(dir.path().join("tmp").exists());
    }
}
