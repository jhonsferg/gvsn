//! Local toolchain storage and version resolution.
//!
//! This module provides the interface between the rest of `gvm` and the
//! on-disk toolchain store at `~/.gvm/versions/`. It handles:
//!
//! - Listing and querying installed versions.
//! - Reading and writing the global default version file.
//! - Walking up the directory tree to find a project-local `.go-version`.
//! - Resolving a [`VersionSpec`] to the best-matching installed [`GoVersion`].

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

use crate::{config::Config, fs as gvm_fs, lock, user_version::VersionSpec, version::GoVersion};

/// Indicates where the active version was determined from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSource {
    /// Activated by a `.go-version` file found in the current directory tree.
    Local,
    /// Activated by the global default stored in `~/.gvm/version`.
    Global,
}

impl VersionSource {
    /// Returns a human-readable label suitable for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "local (.go-version)",
            Self::Global => "global",
        }
    }
}

// --- Queries -----------------------------------------------------------------

/// Returns all installed Go versions, sorted newest-first.
///
/// Reads the entries of [`Config::versions_dir`] and parses each directory
/// name as a [`GoVersion`]. Non-parsable entries are silently skipped.
///
/// # Errors
///
/// Returns an error if the versions directory cannot be read.
pub fn list_installed(config: &Config) -> Result<Vec<GoVersion>> {
    let dir = config.versions_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut versions: Vec<GoVersion> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            e.file_name()
                .into_string()
                .ok()
                .and_then(|n| GoVersion::parse(&n).ok())
        })
        .collect();

    versions.sort_by(|a, b| b.cmp(a));
    Ok(versions)
}

/// Returns `true` if `version` is present in the local toolchain store.
pub fn is_installed(config: &Config, version: &GoVersion) -> bool {
    config.version_dir(&version.tag()).exists()
}

/// Reads the global default Go version from `~/.gvm/version`.
///
/// # Errors
///
/// Returns an error if the file does not exist (i.e. no global version has
/// been set yet) or if its contents cannot be parsed as a valid version.
pub fn global_version(config: &Config) -> Result<GoVersion> {
    let path = config.version_file();
    if !path.exists() {
        bail!("No global Go version set. Run 'gvm use <version>'.");
    }
    let raw = std::fs::read_to_string(&path)?.trim().to_string();
    GoVersion::parse(&raw).context("Corrupted version file - run 'gvm use <version>'")
}

/// Returns the active Go version and the source that determined it.
///
/// The resolution order is:
///
/// 1. Walk up from the current working directory looking for `.go-version`
///    (up to 20 levels to avoid infinite loops on unusual file systems).
/// 2. Fall back to the global default stored in `~/.gvm/version`.
///
/// # Errors
///
/// Returns an error if the current directory cannot be read, if a
/// `.go-version` file contains an invalid version string, or if no global
/// version has been set.
pub fn active_version(config: &Config) -> Result<(GoVersion, VersionSource)> {
    let mut dir = std::env::current_dir()?;
    let mut depth = 0u8;

    loop {
        let vf = dir.join(".go-version");
        if vf.exists() {
            let raw = std::fs::read_to_string(&vf)?.trim().to_string();
            let v = GoVersion::parse(&raw).context("Corrupted .go-version")?;
            return Ok((v, VersionSource::Local));
        }
        // Stop if we have reached the file-system root or the depth limit.
        if depth >= 20 || !dir.pop() {
            break;
        }
        depth += 1;
    }

    global_version(config).map(|v| (v, VersionSource::Global))
}

/// Returns the path to the `bin/` directory of an installed version.
///
/// # Errors
///
/// Returns an error if the version is not installed on disk.
pub fn version_bin_path(config: &Config, version: &GoVersion) -> Result<PathBuf> {
    let bin = config.version_bin_dir(&version.tag());
    if !bin.exists() {
        bail!(
            "Go {} is not installed. Run 'gvm install {}'.",
            version,
            version
        );
    }
    Ok(bin)
}

// --- Mutations ---------------------------------------------------------------

/// Writes `version` to the global default version file (`~/.gvm/version`).
fn write_global_version_file(config: &Config, version: &GoVersion) -> Result<()> {
    std::fs::write(config.version_file(), version.tag())
        .context("Failed to write global version file")
}

/// Updates the `~/.gvm/current` junction/symlink to point to `version`.
///
/// This is what makes `go` visible to all shells (CMD, Git Bash, PowerShell)
/// and editors (VSCode, GoLand) without any per-shell hook. The PATH entry
/// `~/.gvm/current/bin` is added to the Windows registry once during install;
/// afterwards only the junction target needs to change on every `gvm use`.
fn write_current_link(config: &Config, version: &GoVersion) -> Result<()> {
    let link = config.current_dir();
    let target = config.version_dir(&version.tag());
    gvm_fs::set_version_link(&link, &target)
        .with_context(|| format!("Failed to update current link to {}", version.tag()))
}

/// Sets `version` as the active global version: writes `~/.gvm/version` and
/// repoints the `~/.gvm/current` link, both under a single lock acquisition.
///
/// Both writes happen under one locked section so another `gvm` process can
/// never observe (or interleave between) the version file and the `current`
/// link pointing at different versions.
///
/// # Errors
///
/// Returns an error if the version file cannot be written or the link cannot
/// be created.
pub fn set_active_version(config: &Config, version: &GoVersion) -> Result<()> {
    let lock_path = config.root.join(".lock");
    lock::with_lock(&lock_path, || {
        write_global_version_file(config, version)?;
        write_current_link(config, version)
    })
}

// --- Resolution --------------------------------------------------------------

/// Resolves a [`VersionSpec`] to the best-matching installed [`GoVersion`].
///
/// - `Latest` returns the newest installed version.
/// - `Partial` returns the newest installed patch within that minor line.
/// - `Exact` returns the specific version if installed.
///
/// Versions are considered in newest-first order (as returned by
/// [`list_installed`]), so the highest matching patch is always chosen for
/// partial specs.
///
/// # Errors
///
/// Returns an error if no installed version satisfies the spec.
pub fn resolve_installed(config: &Config, spec: &VersionSpec) -> Result<GoVersion> {
    let installed = list_installed(config)?;

    match spec {
        VersionSpec::Latest => installed
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No Go versions installed.")),
        _ => installed
            .into_iter()
            .find(|v| spec.matches(v))
            .ok_or_else(|| {
                anyhow::anyhow!("Go {} is not installed. Run 'gvm install {}'.", spec, spec)
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_config() -> (tempfile::TempDir, Config) {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        (dir, config)
    }

    fn install(config: &Config, tag: &str) {
        std::fs::create_dir_all(config.version_dir(tag)).unwrap();
    }

    #[test]
    fn list_installed_returns_empty_when_dir_missing() {
        let (_dir, config) = make_config();
        assert_eq!(list_installed(&config).unwrap(), vec![]);
    }

    #[test]
    fn list_installed_sorts_newest_first_and_skips_garbage() {
        let (_dir, config) = make_config();
        install(&config, "go1.21.0");
        install(&config, "go1.22.4");
        install(&config, "go1.22.3");
        // A non-version directory entry should be silently skipped.
        std::fs::create_dir_all(config.versions_dir().join("not-a-version")).unwrap();

        let versions = list_installed(&config).unwrap();
        let tags: Vec<String> = versions.iter().map(GoVersion::tag).collect();
        assert_eq!(tags, vec!["go1.22.4", "go1.22.3", "go1.21"]);
    }

    #[test]
    fn is_installed_true_and_false() {
        let (_dir, config) = make_config();
        install(&config, "go1.22.4");
        let v = GoVersion::parse("1.22.4").unwrap();
        assert!(is_installed(&config, &v));

        let missing = GoVersion::parse("1.19.0").unwrap();
        assert!(!is_installed(&config, &missing));
    }

    #[test]
    fn global_version_errors_when_unset() {
        let (_dir, config) = make_config();
        assert!(global_version(&config).is_err());
    }

    #[test]
    fn global_version_reads_written_tag() {
        let (_dir, config) = make_config();
        std::fs::write(config.version_file(), "go1.22.4").unwrap();
        let v = global_version(&config).unwrap();
        assert_eq!(v.tag(), "go1.22.4");
    }

    #[test]
    fn global_version_errors_on_corrupted_file() {
        let (_dir, config) = make_config();
        std::fs::write(config.version_file(), "not-a-version").unwrap();
        assert!(global_version(&config).is_err());
    }

    #[test]
    fn version_bin_path_errors_when_not_installed() {
        let (_dir, config) = make_config();
        let v = GoVersion::parse("1.22.4").unwrap();
        assert!(version_bin_path(&config, &v).is_err());
    }

    #[test]
    fn version_bin_path_succeeds_when_installed() {
        let (_dir, config) = make_config();
        let tag = "go1.22.4";
        std::fs::create_dir_all(config.version_bin_dir(tag)).unwrap();
        let v = GoVersion::parse("1.22.4").unwrap();
        let bin = version_bin_path(&config, &v).unwrap();
        assert_eq!(bin, config.version_bin_dir(tag));
    }

    #[test]
    fn set_active_version_writes_tag_to_file() {
        let (_dir, config) = make_config();
        let tag = "go1.22.4";
        install(&config, tag);
        let v = GoVersion::parse("1.22.4").unwrap();
        set_active_version(&config, &v).unwrap();
        assert_eq!(
            std::fs::read_to_string(config.version_file()).unwrap(),
            "go1.22.4"
        );
    }

    #[test]
    fn set_active_version_points_current_link_to_version_dir() {
        let (_dir, config) = make_config();
        let tag = "go1.22.4";
        install(&config, tag);
        std::fs::write(config.version_dir(tag).join("marker.txt"), b"x").unwrap();
        let v = GoVersion::parse("1.22.4").unwrap();

        set_active_version(&config, &v).unwrap();

        assert!(config.current_dir().join("marker.txt").exists());
    }

    #[test]
    fn resolve_installed_latest_picks_newest() {
        let (_dir, config) = make_config();
        install(&config, "go1.21.0");
        install(&config, "go1.22.4");
        let resolved = resolve_installed(&config, &VersionSpec::Latest).unwrap();
        assert_eq!(resolved.tag(), "go1.22.4");
    }

    #[test]
    fn resolve_installed_latest_errors_when_none_installed() {
        let (_dir, config) = make_config();
        assert!(resolve_installed(&config, &VersionSpec::Latest).is_err());
    }

    #[test]
    fn resolve_installed_partial_matches_installed_patch() {
        let (_dir, config) = make_config();
        install(&config, "go1.22.4");
        let spec = VersionSpec::Partial {
            major: 1,
            minor: 22,
        };
        let resolved = resolve_installed(&config, &spec).unwrap();
        assert_eq!(resolved.tag(), "go1.22.4");
    }

    #[test]
    fn resolve_installed_exact_errors_when_not_found() {
        let (_dir, config) = make_config();
        install(&config, "go1.22.4");
        let spec = VersionSpec::Exact {
            major: 1,
            minor: 19,
            patch: 0,
        };
        assert!(resolve_installed(&config, &spec).is_err());
    }

    #[test]
    fn version_source_label_text() {
        assert_eq!(VersionSource::Local.label(), "local (.go-version)");
        assert_eq!(VersionSource::Global.label(), "global");
    }
}
