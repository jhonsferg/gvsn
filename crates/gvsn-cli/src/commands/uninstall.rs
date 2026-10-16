//! `gvsn uninstall` - remove an installed Go version from disk.
//!
//! The command refuses to uninstall the currently active version to avoid
//! leaving the environment in a broken state. Unless `--force` is passed, it
//! also asks for confirmation before deleting anything - consistent with the
//! other commands that remove installed toolchains (`gvsn prune`, `gvsn implode`).

use anyhow::{bail, Result};
use colored::Colorize;

use gvsn_core::{config::Config, lock, toolchain, user_version::VersionSpec};

use crate::prompt;

/// Removes the Go version described by `spec_str` from the local store.
///
/// The function first checks whether the target version is the currently
/// active one (via `.go-version` or global default). If it is, the command
/// aborts with an error to protect the user from an unbootable environment.
/// Otherwise, unless `force` is `true`, it asks for confirmation before
/// removing the version directory.
///
/// # Errors
///
/// Returns an error if:
/// - `spec_str` is not a valid version spec.
/// - No installed version matches the spec.
/// - The target is the currently active version.
/// - The confirmation prompt is declined.
/// - The version directory cannot be removed.
pub fn run(config: &Config, spec_str: &str, force: bool) -> Result<()> {
    let spec = VersionSpec::parse(spec_str)?;
    let version = toolchain::resolve_installed(config, &spec)?;

    // Protect against uninstalling the active version to avoid leaving
    // the environment pointing at a non-existent toolchain.
    if let Ok((active, _)) = toolchain::active_version(config) {
        if active == version {
            bail!(
                "Cannot uninstall the currently active version ({}). \
                 Switch first with 'gvsn use <version>'.",
                version.tag()
            );
        }
    }

    if !force {
        let message = format!("  Remove Go {}? [y/N] ", version.tag().bold());
        if !prompt::confirm(&message)? {
            bail!("Aborted.");
        }
        println!();
    }

    let dir = config.version_dir(&version.tag());
    let lock_path = config.root.join(".lock");
    lock::with_lock(&lock_path, || Ok(std::fs::remove_dir_all(&dir)?))?;

    println!("{} Go {} uninstalled.", "✓".green(), version.tag().bold());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn install_version(config: &Config, tag: &str) {
        std::fs::create_dir_all(config.version_dir(tag)).unwrap();
    }

    #[test]
    fn run_removes_installed_version_directory() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        // Use a tag distinct from any ambient locally-active version so this
        // test is not coupled to the real process working directory's
        // `.go-version` resolution.
        install_version(&config, "go1.19.9");

        run(&config, "1.19.9", true).unwrap();

        assert!(!config.version_dir("go1.19.9").exists());
    }

    #[test]
    fn run_errors_when_version_not_installed() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        assert!(run(&config, "1.19.9", true).is_err());
    }

    #[test]
    fn run_refuses_to_uninstall_the_active_version() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        install_version(&config, "go1.19.9");
        std::fs::write(config.version_file(), "go1.19.9").unwrap();

        let err = run(&config, "1.19.9", true).unwrap_err();
        assert!(err.to_string().contains("currently active version"));
        // The version must not have been removed.
        assert!(config.version_dir("go1.19.9").exists());
    }

    #[test]
    fn run_errors_on_invalid_spec() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        assert!(run(&config, "not-a-version", true).is_err());
    }
}
