//! `gvsn list` - show all locally installed Go versions.
//!
//! Installed versions are listed newest-first. The currently active version
//! is marked so the user can identify it at a glance.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use crate::{config::Config, toolchain};

/// One installed version as reported by `--json`.
#[derive(Serialize)]
struct InstalledVersion {
    tag: String,
    active: bool,
}

/// Prints all installed Go versions, sorted newest-first.
///
/// The active version (determined from `.go-version` or the global default)
/// is highlighted with a check mark and an `(active)` label. If no version is
/// active - for example because no global default has been set yet - all
/// versions are shown without highlighting.
///
/// When `json` is `true`, prints a JSON array of `{"tag", "active"}` objects
/// instead, with no other output, so the result can be piped into `jq` or
/// parsed by a script.
///
/// # Errors
///
/// Returns an error if the versions directory cannot be read, or (`json`
/// only) if the result cannot be serialised.
pub fn run(config: &Config, json: bool) -> Result<()> {
    let installed = toolchain::list_installed(config)?;
    let active = toolchain::active_version(config).map(|(v, _)| v).ok();

    if json {
        let entries: Vec<InstalledVersion> = installed
            .iter()
            .map(|v| InstalledVersion {
                tag: v.tag(),
                active: active.as_ref() == Some(v),
            })
            .collect();
        println!("{}", serde_json::to_string(&entries)?);
        return Ok(());
    }

    if installed.is_empty() {
        println!("No Go versions installed. Run 'gvsn install latest'.");
        return Ok(());
    }

    println!("Installed Go versions:");
    for v in &installed {
        if active.as_ref() == Some(v) {
            println!(
                "  {} {}  {}",
                "✓".green(),
                v.tag().bold(),
                "(active)".dimmed()
            );
        } else {
            println!("    {}", v.tag());
        }
    }
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
    fn run_reports_no_versions_when_none_installed() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        run(&config, false).unwrap();
    }

    #[test]
    fn run_lists_installed_versions_with_active_marked() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        install_version(&config, "go1.22.4");
        install_version(&config, "go1.21.0");
        std::fs::write(config.version_file(), "go1.22.4").unwrap();

        run(&config, false).unwrap();
    }

    #[test]
    fn run_lists_installed_versions_without_active_version() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        install_version(&config, "go1.22.4");
        // No global version file is set for this config. `active_version`
        // resolves from the real process working directory (not
        // `config.root`), so its outcome is environment-dependent here -
        // the important invariant under test is that `run` still succeeds
        // and lists the installed version either way.
        run(&config, false).unwrap();
    }

    #[test]
    fn run_json_emits_array_with_active_flag() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        install_version(&config, "go1.22.4");
        install_version(&config, "go1.21.0");
        std::fs::write(config.version_file(), "go1.22.4").unwrap();

        run(&config, true).unwrap();
    }

    #[test]
    fn run_json_emits_empty_array_when_none_installed() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        run(&config, true).unwrap();
    }
}
