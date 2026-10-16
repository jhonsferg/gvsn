//! `gvsn completions` - generate shell completion scripts.
//!
//! Uses [`clap_complete`] to generate completion scripts for all supported
//! shells. The output is written to stdout so the caller can redirect it to
//! the appropriate location.
//!
//! # Examples
//!
//! ```text
//! # Bash
//! gvsn completions bash > ~/.local/share/bash-completion/completions/gvsn
//!
//! # Zsh  (fpath must be configured)
//! gvsn completions zsh > "${fpath[1]}/_gvsn"
//!
//! # Fish
//! gvsn completions fish > ~/.config/fish/completions/gvsn.fish
//!
//! # PowerShell
//! gvsn completions powershell >> $PROFILE
//! ```

use anyhow::{bail, Result};
use clap::CommandFactory;
use clap_complete::{generate, Shell as CompShell};

use crate::cli::Cli;

/// Writes the completion script for `shell_str` to stdout.
///
/// # Errors
///
/// Returns an error if `shell_str` does not name a supported shell.
/// Supported values: `bash`, `zsh`, `fish`, `powershell` (or `pwsh`).
pub fn run(shell_str: &str) -> Result<()> {
    let shell = match shell_str.to_lowercase().replace('-', "").as_str() {
        "bash" => CompShell::Bash,
        "zsh" => CompShell::Zsh,
        "fish" => CompShell::Fish,
        "powershell" | "pwsh" => CompShell::PowerShell,
        _ => bail!(
            "Unknown shell '{}'. Supported: bash, zsh, fish, powershell",
            shell_str
        ),
    };

    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "gvsn", &mut std::io::stdout());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_accepts_all_supported_shells() {
        for shell in [
            "bash",
            "zsh",
            "fish",
            "powershell",
            "pwsh",
            "PowerShell",
            "Bash",
        ] {
            run(shell).unwrap();
        }
    }

    #[test]
    fn run_errors_on_unsupported_shell() {
        let err = run("not-a-shell").unwrap_err();
        assert!(err.to_string().contains("Unknown shell"));
    }
}
