//! `gvm env` - emit shell initialisation commands.
//!
//! Generates the shell script that sets `GVM_DIR`, `PATH`, and `GOROOT` for
//! the active Go version, and registers the `cd` hook that keeps those
//! variables in sync as the user navigates the file system.
//!
//! The output is meant to be evaluated by the shell, not executed as a script:
//!
//! - Bash / Zsh: `eval "$(gvm env --shell bash)"`
//! - Fish: `gvm env --shell fish | source`
//! - PowerShell: `gvm env --shell powershell | Out-String | Invoke-Expression`

use anyhow::Result;

use crate::{
    config::Config,
    shell::{self, EnvContext},
    toolchain,
    toolchain::VersionSource,
};

/// Prints the shell initialisation script for the active Go version.
///
/// The target shell is taken from `shell_str` when provided, otherwise it is
/// auto-detected from the environment via [`shell::detect`]. If no version is
/// currently active, the script still sets `GVM_DIR` and installs the hook,
/// but omits the `PATH` and `GOROOT` assignments.
///
/// # Errors
///
/// Returns an error if `shell_str` names an unsupported shell or if no shell
/// can be detected and `shell_str` is `None`.
pub fn run(config: &Config, shell_str: Option<&str>) -> Result<()> {
    let shell = match shell_str {
        Some(s) => shell::from_str(s)?,
        None => shell::detect()
            .ok_or_else(|| anyhow::anyhow!("Could not detect shell. Use --shell <name>."))?,
    };

    let (active_bin, active_root) = match toolchain::active_version(config) {
        Ok((v, src)) => match toolchain::version_bin_path(config, &v) {
            Ok(bin) => (Some(bin), Some(config.version_dir(&v.tag()))),
            Err(_) => {
                if src == VersionSource::Local {
                    eprintln!(
                        "gvm: Go {} (from .go-version or .tool-versions) is not installed. Run: gvm install {}",
                        v, v
                    );
                }
                (None, None)
            }
        },
        Err(_) => (None, None),
    };

    let ctx = EnvContext {
        gvm_dir: &config.root,
        active_bin: active_bin.as_deref(),
        active_root: active_root.as_deref(),
    };

    print!("{}", shell.env_script(&ctx));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn run_succeeds_with_explicit_shell_and_no_active_version() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        // No version file and no .go-version anywhere: active_version() fails,
        // but run() must still succeed and just print the base env script.
        run(&config, Some("bash")).unwrap();
    }

    #[test]
    fn run_errors_on_unknown_shell() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        assert!(run(&config, Some("not-a-real-shell")).is_err());
    }

    #[test]
    fn run_uses_global_version_when_installed() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        let tag = "go1.22.4";
        std::fs::create_dir_all(config.version_bin_dir(tag)).unwrap();
        let go_name = if cfg!(windows) { "go.exe" } else { "go" };
        std::fs::write(config.version_bin_dir(tag).join(go_name), b"").unwrap();
        std::fs::write(config.version_file(), tag).unwrap();

        // Should resolve the global version and print a script containing
        // the GOROOT for that version without erroring.
        run(&config, Some("bash")).unwrap();
    }
}
