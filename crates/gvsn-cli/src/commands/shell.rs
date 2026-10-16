//! `gvsn shell` - activate a Go version for the current session only.
//!
//! Unlike `gvsn use`, this command does **not** write to `~/.gvsn/version` or
//! create a `.go-version` file. Instead it outputs a shell script (to stdout)
//! that the shell wrapper function immediately evaluates, setting
//! `GVSN_SHELL_VERSION`, `GOROOT`, and `PATH` only for the lifetime of the
//! current terminal session.
//!
//! When `--unset` is passed the script clears `GVSN_SHELL_VERSION` and triggers
//! `_gvsn_hook` so the version reverts immediately to whatever `.go-version` or
//! the global default says.
//!
//! # Usage
//!
//! ```text
//! gvsn shell 1.23        # activate for this session
//! gvsn shell --unset     # revert to file-based version
//! ```
//!
//! The output of this command is meant to be eval'd by the shell wrapper
//! function that `gvsn setup` injects into the user's profile. Running the
//! command directly in a terminal will print the raw shell script - use
//! `gvsn env` for that use-case instead.

use anyhow::{bail, Result};
use colored::Colorize;

use gvsn_core::{config::Config, toolchain, user_version::VersionSpec};
use gvsn_shell::shell;

/// Outputs the shell script that activates a specific version for this session,
/// or the script that reverts to the file-based version when `unset` is true.
///
/// All human-readable messages are printed to **stderr** so the stdout output
/// remains a clean, eval-able shell script.
///
/// # Errors
///
/// Returns an error if:
/// - Both `version_str` and `unset` are provided (mutually exclusive).
/// - Neither `version_str` nor `unset` is provided.
/// - `version_str` is not a valid version spec or not installed.
/// - `shell_str` names an unsupported shell or no shell can be detected.
pub fn run(
    config: &Config,
    version_str: Option<&str>,
    unset: bool,
    shell_str: Option<&str>,
) -> Result<()> {
    if unset && version_str.is_some() {
        bail!("Cannot specify a version together with --unset.");
    }
    if !unset && version_str.is_none() {
        bail!(
            "Specify a version to activate (e.g. `gvsn shell 1.23`) \
             or use --unset to revert."
        );
    }

    let sh = match shell_str {
        Some(s) => shell::from_str(s)?,
        None => shell::detect()
            .ok_or_else(|| anyhow::anyhow!("Could not detect shell. Use --shell <name>."))?,
    };

    if unset {
        eprintln!("{} Reverted to file-based Go version.", "→".cyan());
        print!("{}", sh.shell_unset_script());
        return Ok(());
    }

    let spec = VersionSpec::parse(version_str.unwrap())?;
    let version = toolchain::resolve_installed(config, &spec)?;
    let bin = toolchain::version_bin_path(config, &version)?;
    let root = config.version_dir(&version.tag());

    eprintln!(
        "{} Go {} activated for this session ({}). \
         Run {} to revert.",
        "→".cyan(),
        version.tag().bold(),
        "GVSN_SHELL_VERSION".dimmed(),
        "gvsn shell --unset".cyan()
    );

    print!("{}", sh.shell_version_script(&version.tag(), &bin, &root));
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use gvsn_shell::shell::{Bash, PowerShell, ShellConfig};
    use std::path::Path;

    #[test]
    fn bash_shell_version_script_sets_gvsn_shell_version() {
        let script = Bash.shell_version_script(
            "go1.23.4",
            Path::new("/home/user/.gvsn/versions/go1.23.4/bin"),
            Path::new("/home/user/.gvsn/versions/go1.23.4"),
        );
        assert!(
            script.contains("GVSN_SHELL_VERSION"),
            "Must set GVSN_SHELL_VERSION"
        );
        assert!(script.contains("go1.23.4"), "Must include the version tag");
        assert!(script.contains("GOROOT"), "Must set GOROOT");
        assert!(script.contains("PATH"), "Must update PATH");
    }

    #[test]
    fn bash_shell_version_script_includes_bin_path() {
        let bin = Path::new("/home/user/.gvsn/versions/go1.23.4/bin");
        let script = Bash.shell_version_script(
            "go1.23.4",
            bin,
            Path::new("/home/user/.gvsn/versions/go1.23.4"),
        );
        assert!(
            script.contains(bin.to_str().unwrap()),
            "Bin path must appear in the script"
        );
    }

    #[test]
    fn bash_shell_unset_script_clears_gvsn_shell_version() {
        let script = Bash.shell_unset_script();
        assert!(
            script.contains("GVSN_SHELL_VERSION"),
            "Must reference GVSN_SHELL_VERSION for clearing"
        );
    }

    #[test]
    fn bash_shell_unset_script_triggers_hook() {
        let script = Bash.shell_unset_script();
        assert!(
            script.contains("_gvsn_hook"),
            "Must call _gvsn_hook to restore env"
        );
    }

    #[test]
    fn powershell_shell_version_script_sets_env_var() {
        let script = PowerShell.shell_version_script(
            "go1.23.4",
            Path::new(r"C:\Users\user\.gvsn\versions\go1.23.4\bin"),
            Path::new(r"C:\Users\user\.gvsn\versions\go1.23.4"),
        );
        assert!(script.contains("GVSN_SHELL_VERSION"));
        assert!(script.contains("go1.23.4"));
        assert!(script.contains("GOROOT"));
    }

    #[test]
    fn powershell_shell_unset_script_clears_env_var() {
        let script = PowerShell.shell_unset_script();
        assert!(script.contains("GVSN_SHELL_VERSION"));
        assert!(script.contains("_gvsn_hook"));
    }

    #[test]
    fn shell_version_script_not_empty_for_all_shells() {
        use gvsn_shell::shell::{Fish, Zsh};
        let bin = Path::new("/home/user/.gvsn/versions/go1.23.4/bin");
        let root = Path::new("/home/user/.gvsn/versions/go1.23.4");
        assert!(!Bash.shell_version_script("go1.23.4", bin, root).is_empty());
        assert!(!Zsh.shell_version_script("go1.23.4", bin, root).is_empty());
        assert!(!Fish.shell_version_script("go1.23.4", bin, root).is_empty());
        assert!(!PowerShell
            .shell_version_script("go1.23.4", bin, root)
            .is_empty());
    }

    #[test]
    fn shell_unset_script_not_empty_for_all_shells() {
        use gvsn_shell::shell::{Fish, Zsh};
        assert!(!Bash.shell_unset_script().is_empty());
        assert!(!Zsh.shell_unset_script().is_empty());
        assert!(!Fish.shell_unset_script().is_empty());
        assert!(!PowerShell.shell_unset_script().is_empty());
    }

    // ── run() validation and success paths ──────────────────────────────────

    use super::run;
    use gvsn_core::config::Config;
    use tempfile::tempdir;

    fn make_config() -> (tempfile::TempDir, Config) {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        (dir, config)
    }

    #[test]
    fn run_errors_when_version_and_unset_both_given() {
        let (_dir, config) = make_config();
        let err = run(&config, Some("1.22.4"), true, Some("bash")).unwrap_err();
        assert!(err.to_string().contains("Cannot specify a version"));
    }

    #[test]
    fn run_errors_when_neither_version_nor_unset_given() {
        let (_dir, config) = make_config();
        let err = run(&config, None, false, Some("bash")).unwrap_err();
        assert!(err.to_string().contains("Specify a version"));
    }

    #[test]
    fn run_errors_on_unknown_shell() {
        let (_dir, config) = make_config();
        assert!(run(&config, None, true, Some("not-a-shell")).is_err());
    }

    #[test]
    fn run_unset_succeeds_with_explicit_shell() {
        let (_dir, config) = make_config();
        run(&config, None, true, Some("bash")).unwrap();
    }

    #[test]
    fn run_errors_when_requested_version_not_installed() {
        let (_dir, config) = make_config();
        assert!(run(&config, Some("1.22.4"), false, Some("bash")).is_err());
    }

    #[test]
    fn run_succeeds_for_installed_version() {
        let (_dir, config) = make_config();
        let tag = "go1.22.4";
        std::fs::create_dir_all(config.version_bin_dir(tag)).unwrap();
        run(&config, Some("1.22.4"), false, Some("bash")).unwrap();
    }
}
