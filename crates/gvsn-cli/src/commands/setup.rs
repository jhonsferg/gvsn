//! `gvsn setup` - configure the shell environment for gvsn.
//!
//! Responsible for ALL environment configuration: shell hooks, static PATH
//! entries in login profiles (Linux/macOS), and registry PATH entries
//! (Windows). The install script places the binary and then delegates here.
//!
//! # What this command does
//!
//! 1. Injects `# gvsn init` + `# gvsn wrapper` into the shell's interactive
//!    profile (e.g. `~/.bashrc`).
//! 2. On Linux/macOS: injects `# gvsn path` into the login profile
//!    (`~/.profile` for bash, `~/.zprofile` for zsh) so `~/.gvsn/current/bin`
//!    is on PATH for GUI applications (VSCode, GoLand, etc.) that do not
//!    source the interactive profile.
//! 3. On Windows: adds the gvsn binary directory and `~\.gvsn\current\bin` to
//!    the user PATH in the registry so all applications see them.
//!
//! # `--reset` flag
//!
//! Strips every `# gvsn ...` block from the interactive and login profiles
//! (and the Windows registry on Windows), then re-applies clean configuration.
//! Only gvsn-managed blocks are touched; all other content is preserved.

use anyhow::Result;
use colored::Colorize;

use gvsn_shell::{shell, shell::ShellConfig};

/// Runs `gvsn setup`, optionally with a full reset.
///
/// `shell_str` overrides auto-detection. `reset` strips all previous gvsn
/// configuration before re-applying.
///
/// # Errors
///
/// Returns an error if the shell cannot be detected, profiles cannot be
/// written, or (on Windows) the registry cannot be accessed.
pub fn run(shell_str: Option<&str>, reset: bool) -> Result<()> {
    let sh: Box<dyn ShellConfig> = match shell_str {
        Some(s) => {
            let sh = shell::from_str(s)?;
            if !shell::is_available(sh.as_ref()) {
                let hint = hint_for_missing_explicit_shell(&shell::available_shells());
                anyhow::bail!(
                    "Shell '{}' is not installed or not found in PATH.\n  {}",
                    s,
                    hint
                );
            }
            sh
        }
        None => match shell::detect() {
            Some(sh) => {
                // Sanity-check: the detected shell should always be available,
                // but guard against a stale $SHELL pointing to a removed binary.
                if !shell::is_available(sh.as_ref()) {
                    let hint = hint_for_stale_detected_shell(&shell::available_shells());
                    anyhow::bail!(
                        "Detected shell '{}' but its binary was not found in PATH.\n  {}",
                        sh.name(),
                        hint
                    );
                }
                sh
            }
            None => {
                let hint = hint_for_no_shell_detected(&shell::available_shells());
                anyhow::bail!("Could not detect current shell.\n  {}", hint);
            }
        },
    };

    println!("Setting up gvsn for {}...", sh.name().bold());

    // ---- Optional reset: strip all previous gvsn config ----------------------
    if reset {
        strip_all(sh.as_ref())?;
        println!();
        println!("  Previous configuration removed. Re-applying...");
        println!();
    }

    // ---- Interactive profile: eval hook + wrapper ---------------------------
    let gvsn_bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(std::path::PathBuf::from));
    shell::inject_profile(sh.as_ref(), gvsn_bin_dir.as_deref())?;

    // ---- Login profile (Linux/macOS): static PATH for GUI apps --------------
    #[cfg(not(target_os = "windows"))]
    shell::inject_login_profile(sh.as_ref())?;

    // ---- Windows registry: PATH entries for gvsn binary + current/bin --------
    #[cfg(target_os = "windows")]
    inject_windows_registry()?;

    // ---- Warn if gvsn binary itself is not yet on PATH -----------------------
    if !shell::gvsn_in_path() {
        if let Ok(exe) = std::env::current_exe() {
            let dir = exe
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            println!();
            println!("{} gvsn is not in PATH yet.", "!".yellow());
            println!(
                "  Add {} to your PATH so the shell hook can call 'gvsn path'.",
                dir.cyan()
            );
        }
    }

    println!();
    println!(
        "{} Restart your shell or run: {}",
        "✓".green(),
        sh.init_line().cyan()
    );
    Ok(())
}

// --- Hint building -------------------------------------------------------

/// Builds the hint appended to the error when an explicit `--shell <s>`
/// value names a shell that isn't installed or isn't on PATH.
fn hint_for_missing_explicit_shell(available: &[&str]) -> String {
    if available.is_empty() {
        "No supported shells found in PATH.".to_string()
    } else {
        format!("Shells available on this system: {}", available.join(", "))
    }
}

/// Builds the hint appended to the error when the auto-detected shell (e.g.
/// via `$SHELL`) points at a binary that can no longer be found on PATH.
fn hint_for_stale_detected_shell(available: &[&str]) -> String {
    if available.is_empty() {
        "No supported shells found in PATH. Install bash, zsh, or fish first.".to_string()
    } else {
        format!(
            "Try: gvsn setup --shell {}",
            available.first().copied().unwrap_or("bash")
        )
    }
}

/// Builds the hint appended to the error when no shell could be detected at
/// all (neither `$SHELL` nor `PSModulePath` were set).
fn hint_for_no_shell_detected(available: &[&str]) -> String {
    if available.is_empty() {
        "No supported shells found. Install bash, zsh, fish, or PowerShell first.".to_string()
    } else {
        format!(
            "Detected shells: {}. Use --shell <name> to select one.",
            available.join(", ")
        )
    }
}

// --- Reset helpers -----------------------------------------------------------

/// Strips all gvsn-managed blocks from the interactive profile and, on
/// non-Windows, from the login profile as well.
fn strip_all(sh: &dyn ShellConfig) -> Result<()> {
    // Interactive profile (e.g. ~/.bashrc)
    if let Some(p) = sh.profile_path() {
        match shell::strip_profile(&p) {
            Ok(true) => println!("  {} Cleaned {}", "✓".green(), p.display()),
            Ok(false) => println!("  No gvsn config found in {}", p.display()),
            Err(e) => println!("  {} Could not clean {}: {e}", "!".yellow(), p.display()),
        }
    }

    // Login profile (e.g. ~/.profile or ~/.zprofile)
    #[cfg(not(target_os = "windows"))]
    if let Some(p) = sh.login_profile_path() {
        match shell::strip_profile(&p) {
            Ok(true) => println!("  {} Cleaned {}", "✓".green(), p.display()),
            Ok(false) => println!("  No gvsn config found in {}", p.display()),
            Err(e) => println!("  {} Could not clean {}: {e}", "!".yellow(), p.display()),
        }
    }

    // Windows registry
    #[cfg(target_os = "windows")]
    strip_windows_registry()?;

    Ok(())
}

// --- Windows registry --------------------------------------------------------

/// Adds the gvsn binary directory and `~\.gvsn\current\bin` to the user PATH
/// in the Windows registry (HKCU\Environment).
///
/// Idempotent: entries that are already present are not duplicated.
#[cfg(target_os = "windows")]
fn inject_windows_registry() -> Result<()> {
    let mut dirs = Vec::new();
    // gvsn binary directory
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::PathBuf::from))
    {
        dirs.push(dir);
    }
    // ~/.gvsn/current/bin
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".gvsn").join("current").join("bin"));
    }

    let report = gvsn_platform::user_path::prepend_user_path_entries(&dirs)?;
    for (dir, added) in &report {
        if *added {
            println!("  Added {} to user PATH (registry)", dir.display());
        } else {
            println!("  {} already in user PATH", dir.display());
        }
    }

    if report.iter().any(|(_, added)| *added) {
        println!("  {} User PATH updated in registry", "✓".green());
        println!(
            "  New terminals and GUI apps (VSCode, GoLand, ...) will pick this up automatically."
        );
        println!("  Already-open ones need to be restarted to see it.");
    }

    Ok(())
}

/// Removes all gvsn-managed entries from the Windows user PATH in the registry.
///
/// `pub(crate)` so `commands::implode` can reuse it - `implode` must undo
/// everything `setup`'s `--reset` path undoes, including this.
#[cfg(target_os = "windows")]
pub(crate) fn strip_windows_registry() -> Result<()> {
    // Remove any entry that is inside the user's .gvsn directory.
    let home = dirs::home_dir().unwrap_or_default();
    let gvsn_root = home.join(".gvsn");

    if gvsn_platform::user_path::remove_user_path_under(&gvsn_root)? {
        println!(
            "  {} Removed gvsn entries from user PATH (registry)",
            "✓".green()
        );
    }

    Ok(())
}

// --- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_os = "windows"))]
    use std::path::PathBuf;

    #[test]
    fn hint_for_missing_explicit_shell_lists_available() {
        assert_eq!(
            hint_for_missing_explicit_shell(&["bash", "zsh"]),
            "Shells available on this system: bash, zsh"
        );
    }

    #[test]
    fn hint_for_missing_explicit_shell_handles_empty() {
        assert_eq!(
            hint_for_missing_explicit_shell(&[]),
            "No supported shells found in PATH."
        );
    }

    #[test]
    fn hint_for_stale_detected_shell_suggests_first_available() {
        assert_eq!(
            hint_for_stale_detected_shell(&["zsh", "fish"]),
            "Try: gvsn setup --shell zsh"
        );
    }

    #[test]
    fn hint_for_stale_detected_shell_handles_empty() {
        assert_eq!(
            hint_for_stale_detected_shell(&[]),
            "No supported shells found in PATH. Install bash, zsh, or fish first."
        );
    }

    #[test]
    fn hint_for_no_shell_detected_lists_available() {
        assert_eq!(
            hint_for_no_shell_detected(&["bash", "fish"]),
            "Detected shells: bash, fish. Use --shell <name> to select one."
        );
    }

    #[test]
    fn hint_for_no_shell_detected_handles_empty() {
        assert_eq!(
            hint_for_no_shell_detected(&[]),
            "No supported shells found. Install bash, zsh, fish, or PowerShell first."
        );
    }

    /// Fake [`ShellConfig`] whose `profile_path()`/`login_profile_path()`
    /// point into a tempdir, so `strip_all` can be exercised without
    /// touching real shell profiles or (on Windows) the registry.
    #[cfg(not(target_os = "windows"))]
    #[derive(Debug)]
    struct FakeShell {
        profile: PathBuf,
        login_profile: PathBuf,
    }

    #[cfg(not(target_os = "windows"))]
    impl ShellConfig for FakeShell {
        fn name(&self) -> &'static str {
            "bash"
        }
        fn env_script(&self, _ctx: &shell::EnvContext<'_>) -> String {
            String::new()
        }
        fn profile_path(&self) -> Option<PathBuf> {
            Some(self.profile.clone())
        }
        fn login_profile_path(&self) -> Option<PathBuf> {
            Some(self.login_profile.clone())
        }
        fn init_line(&self) -> &'static str {
            "eval gvsn"
        }
        fn wrapper_function(&self) -> &'static str {
            "gvsn() { command gvsn \"$@\"; }"
        }
        fn shell_version_script(
            &self,
            _tag: &str,
            _bin: &std::path::Path,
            _root: &std::path::Path,
        ) -> String {
            String::new()
        }
        fn shell_unset_script(&self) -> &'static str {
            ""
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn strip_all_cleans_both_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let sh = FakeShell {
            profile: dir.path().join("profile"),
            login_profile: dir.path().join("login_profile"),
        };

        // Seed both files with gvsn-managed blocks plus user content.
        std::fs::write(
            &sh.profile,
            "# user\nexport FOO=bar\n\n# gvsn init\neval gvsn\n",
        )
        .unwrap();
        std::fs::write(
            &sh.login_profile,
            "# gvsn path\nexport PATH=\"$HOME/.gvsn/current/bin:$PATH\"\n",
        )
        .unwrap();

        strip_all(&sh).unwrap();

        let profile_content = std::fs::read_to_string(&sh.profile).unwrap();
        assert!(!profile_content.contains("# gvsn init"));
        assert!(profile_content.contains("export FOO=bar"));

        let login_content = std::fs::read_to_string(&sh.login_profile).unwrap();
        assert!(!login_content.contains("# gvsn path"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn strip_all_is_a_noop_on_files_without_gvsn_config() {
        let dir = tempfile::tempdir().unwrap();
        let sh = FakeShell {
            profile: dir.path().join("profile"),
            login_profile: dir.path().join("login_profile"),
        };
        std::fs::write(&sh.profile, "export FOO=bar\n").unwrap();

        // Must not error even though the login profile doesn't exist yet.
        strip_all(&sh).unwrap();

        let content = std::fs::read_to_string(&sh.profile).unwrap();
        assert_eq!(content, "export FOO=bar\n");
    }
}
