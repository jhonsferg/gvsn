//! `gvm use` - set the global default Go version.
//!
//! Resolves the supplied spec against the local toolchain store, writes the
//! version tag to `~/.gvm/version`, and prints the shell-appropriate command
//! the user should run to activate the change in their current session.

use anyhow::Result;
use colored::Colorize;

use crate::{config::Config, shell, toolchain, user_version::VersionSpec};

/// Sets the global default Go version to the version described by `spec_str`.
///
/// The version must already be installed. The hint printed after a successful
/// switch is tailored to the detected shell so the user can copy-paste it
/// directly.
///
/// # Errors
///
/// Returns an error if `spec_str` is not a valid version spec or if no
/// installed version matches the spec.
pub fn run(config: &Config, spec_str: &str) -> Result<()> {
    let spec = VersionSpec::parse(spec_str)?;
    let version = toolchain::resolve_installed(config, &spec)?;

    toolchain::set_active_version(config, &version)?;

    println!(
        "{} Now using Go {} (global).",
        "✓".green(),
        version.tag().bold()
    );
    println!(
        "  Active in all shells (CMD, Git Bash, PowerShell) and editors via {}.",
        "~/.gvm/current".cyan()
    );

    // When the gvm wrapper function is active (injected by `gvm setup`) the
    // current shell session is already updated automatically. Print a fallback
    // hint for sessions where the wrapper is not loaded (scripts, CI, raw shell).
    let hint = match shell::detect() {
        Some(sh) if sh.name() == "powershell" => {
            format!(
                "gvm env --shell powershell | Out-String | {}",
                "Invoke-Expression".cyan()
            )
        }
        Some(sh) => format!("eval \"$(gvm env --shell {})\"", sh.name().cyan()),
        None => "eval \"$(gvm env)\"".to_string(),
    };
    println!("  (current session without wrapper: {hint})");
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
    fn run_sets_global_version_and_current_link() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        install_version(&config, "go1.22.4");

        run(&config, "1.22.4").unwrap();

        assert_eq!(
            std::fs::read_to_string(config.version_file()).unwrap(),
            "go1.22.4"
        );
        assert!(config.current_dir().exists());
    }

    #[test]
    fn run_errors_when_version_not_installed() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        assert!(run(&config, "1.22.4").is_err());
    }

    #[test]
    fn run_errors_on_invalid_spec() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        assert!(run(&config, "garbage").is_err());
    }
}
