//! `gvsn current` - print the active Go version and its source.
//!
//! The source indicates whether the version was determined from a local
//! `.go-version` file (`local (.go-version)`) or from the global default
//! (`global`).

use anyhow::Result;
use colored::Colorize;

use crate::{config::Config, toolchain};

/// Prints the active Go version tag and the source that determined it.
///
/// Example output:
///
/// ```text
/// go1.22.4  (local (.go-version))
/// go1.23.0  (global)
/// ```
///
/// # Errors
///
/// Returns an error if no global default has been set and no `.go-version`
/// file is found in the current directory tree.
pub fn run(config: &Config) -> Result<()> {
    let (version, source) = toolchain::active_version(config)?;
    println!(
        "{} {}",
        version.tag().bold(),
        format!("({})", source.label()).dimmed()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn run_succeeds_when_global_version_is_set() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        std::fs::write(config.version_file(), "go1.22.4").unwrap();

        // `active_version` prefers a local `.go-version` found by walking up
        // from the real process cwd, so we can't assert on *which* version
        // wins here - only that a global default being present doesn't
        // cause `run` to error.
        run(&config).unwrap();
    }
}
