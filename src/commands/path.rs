//! `gvsn path` - print the `bin/` directory of a Go version.
//!
//! The output is a plain, undecorated path designed to be captured by shell
//! hooks and scripts:
//!
//! ```sh
//! export PATH="$(gvsn path):$PATH"
//! export GOROOT="$(dirname "$(gvsn path)")"
//! ```

use anyhow::Result;

use crate::{config::Config, toolchain, user_version::VersionSpec};

/// Prints the `bin/` directory path for the active or specified Go version.
///
/// When `spec_str` is `None`, the active version is resolved via
/// [`toolchain::active_version`] (`.go-version` lookup followed by global
/// default). When a spec is provided it must refer to an installed version.
///
/// Output is a single line containing only the path, with no decorations, so
/// it can be used directly in shell command substitution.
///
/// # Errors
///
/// Returns an error if the spec is invalid, no matching version is installed,
/// or no active version can be determined.
pub fn run(config: &Config, spec_str: Option<&str>) -> Result<()> {
    let version = match spec_str {
        Some(s) => toolchain::resolve_installed(config, &VersionSpec::parse(s)?)?,
        None => toolchain::active_version(config)?.0,
    };

    let bin = toolchain::version_bin_path(config, &version)?;
    println!("{}", bin.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn install_version(config: &Config, tag: &str) {
        std::fs::create_dir_all(config.version_bin_dir(tag)).unwrap();
    }

    #[test]
    fn run_prints_bin_path_for_explicit_installed_spec() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        install_version(&config, "go1.22.4");

        run(&config, Some("1.22.4")).unwrap();
    }

    #[test]
    fn run_errors_when_spec_not_installed() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        assert!(run(&config, Some("1.22.4")).is_err());
    }

    #[test]
    fn run_errors_on_invalid_spec_string() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        assert!(run(&config, Some("not-a-version")).is_err());
    }

    #[test]
    fn run_resolves_partial_spec_to_installed_patch() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        install_version(&config, "go1.22.4");

        run(&config, Some("1.22")).unwrap();
    }
}
