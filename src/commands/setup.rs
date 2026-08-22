//! `gvm setup` - configure the shell environment for gvm.
//!
//! Responsible for ALL environment configuration: shell hooks, static PATH
//! entries in login profiles (Linux/macOS), and registry PATH entries
//! (Windows). The install script places the binary and then delegates here.
//!
//! # What this command does
//!
//! 1. Injects `# gvm init` + `# gvm wrapper` into the shell's interactive
//!    profile (e.g. `~/.bashrc`).
//! 2. On Linux/macOS: injects `# gvm path` into the login profile
//!    (`~/.profile` for bash, `~/.zprofile` for zsh) so `~/.gvm/current/bin`
//!    is on PATH for GUI applications (VSCode, GoLand, etc.) that do not
//!    source the interactive profile.
//! 3. On Windows: adds the gvm binary directory and `~\.gvm\current\bin` to
//!    the user PATH in the registry so all applications see them.
//!
//! # `--reset` flag
//!
//! Strips every `# gvm ...` block from the interactive and login profiles
//! (and the Windows registry on Windows), then re-applies clean configuration.
//! Only gvm-managed blocks are touched; all other content is preserved.

use anyhow::Result;
use colored::Colorize;

use crate::{shell, shell::ShellConfig};

/// Runs `gvm setup`, optionally with a full reset.
///
/// `shell_str` overrides auto-detection. `reset` strips all previous gvm
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

    println!("Setting up gvm for {}...", sh.name().bold());

    // ---- Optional reset: strip all previous gvm config ----------------------
    if reset {
        strip_all(sh.as_ref())?;
        println!();
        println!("  Previous configuration removed. Re-applying...");
        println!();
    }

    // ---- Interactive profile: eval hook + wrapper ---------------------------
    let gvm_bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(std::path::PathBuf::from));
    shell::inject_profile(sh.as_ref(), gvm_bin_dir.as_deref())?;

    // ---- Login profile (Linux/macOS): static PATH for GUI apps --------------
    #[cfg(not(target_os = "windows"))]
    shell::inject_login_profile(sh.as_ref())?;

    // ---- Windows registry: PATH entries for gvm binary + current/bin --------
    #[cfg(target_os = "windows")]
    inject_windows_registry()?;

    // ---- Warn if gvm binary itself is not yet on PATH -----------------------
    if !shell::gvm_in_path() {
        if let Ok(exe) = std::env::current_exe() {
            let dir = exe
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            println!();
            println!("{} gvm is not in PATH yet.", "!".yellow());
            println!(
                "  Add {} to your PATH so the shell hook can call 'gvm path'.",
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
            "Try: gvm setup --shell {}",
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

/// Strips all gvm-managed blocks from the interactive profile and, on
/// non-Windows, from the login profile as well.
fn strip_all(sh: &dyn ShellConfig) -> Result<()> {
    // Interactive profile (e.g. ~/.bashrc)
    if let Some(p) = sh.profile_path() {
        match shell::strip_profile(&p) {
            Ok(true) => println!("  {} Cleaned {}", "✓".green(), p.display()),
            Ok(false) => println!("  No gvm config found in {}", p.display()),
            Err(e) => println!("  {} Could not clean {}: {e}", "!".yellow(), p.display()),
        }
    }

    // Login profile (e.g. ~/.profile or ~/.zprofile)
    #[cfg(not(target_os = "windows"))]
    if let Some(p) = sh.login_profile_path() {
        match shell::strip_profile(&p) {
            Ok(true) => println!("  {} Cleaned {}", "✓".green(), p.display()),
            Ok(false) => println!("  No gvm config found in {}", p.display()),
            Err(e) => println!("  {} Could not clean {}: {e}", "!".yellow(), p.display()),
        }
    }

    // Windows registry
    #[cfg(target_os = "windows")]
    strip_windows_registry()?;

    Ok(())
}

// --- Windows registry --------------------------------------------------------

/// Returns `true` if `candidate` is already present in `entries`.
///
/// Windows paths are case-insensitive, so a PATH entry that differs from
/// `candidate` only in case (e.g. a different drive-letter casing from
/// another shell) still counts as present - comparing `Path` values directly
/// would miss this and insert a duplicate entry on every `gvm setup` run.
///
/// Kept unconditional (not `#[cfg(windows)]`) so its logic is unit-tested on
/// every platform; its only production caller, `inject_windows_registry`, is
/// Windows-only, so the lint is suppressed on other platforms instead.
#[cfg_attr(not(windows), allow(dead_code))]
fn path_already_present(entries: &[String], candidate: &std::path::Path) -> bool {
    let candidate = candidate.to_string_lossy();
    entries
        .iter()
        .any(|e| e.eq_ignore_ascii_case(candidate.as_ref()))
}

/// Broadcasts `WM_SETTINGCHANGE` so running processes - most importantly
/// Explorer - refresh their cached environment block immediately.
///
/// Writing `HKCU\Environment` alone is not enough: Explorer (and anything it
/// spawns - the Start Menu, the taskbar, double-clicking an `.exe`) keeps its
/// own copy of the environment from whenever it last started, and normally
/// only refreshes it on this broadcast, a logoff, or a restart. Without it, a
/// GUI application like VS Code or GoLand launched right after `gvm setup`/
/// `gvm use` still sees the *old* PATH and fails to find `go`, even though
/// any terminal opened afterward works fine (terminals inherit PATH fresh
/// from Explorer at spawn time once Explorer itself has the update).
///
/// This only affects newly spawned processes - a GUI app that was already
/// running keeps whatever environment it started with regardless, since
/// Windows has no way to inject new environment variables into a live
/// process. It still needs to be closed and reopened once.
///
/// Best-effort: failures are ignored. Worst case, the pre-existing "restart
/// your terminal or log out/in" guidance still applies.
#[cfg(target_os = "windows")]
fn broadcast_environment_change() {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    let param: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result: usize = 0;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST as HWND,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        );
    }
}

/// Adds the gvm binary directory and `~\.gvm\current\bin` to the user PATH
/// in the Windows registry (HKCU\Environment).
///
/// Idempotent: entries that are already present are not duplicated.
#[cfg(target_os = "windows")]
fn inject_windows_registry() -> Result<()> {
    use anyhow::Context;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("Cannot open HKCU\\Environment registry key")?;

    let current_path: String = env.get_value("PATH").unwrap_or_default();
    let mut entries: Vec<String> = current_path
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    let mut changed = false;

    // gvm binary directory
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let dir_str = dir.to_string_lossy().to_string();
            if !path_already_present(&entries, dir) {
                entries.insert(0, dir_str);
                println!("  Added {} to user PATH (registry)", dir.display());
                changed = true;
            } else {
                println!("  {} already in user PATH", dir.display());
            }
        }
    }

    // ~/.gvm/current/bin
    if let Some(home) = dirs::home_dir() {
        let current_bin = home.join(".gvm").join("current").join("bin");
        let current_bin_str = current_bin.to_string_lossy().to_string();
        if !path_already_present(&entries, &current_bin) {
            entries.insert(0, current_bin_str);
            println!("  Added {} to user PATH (registry)", current_bin.display());
            changed = true;
        } else {
            println!("  {} already in user PATH", current_bin.display());
        }
    }

    if changed {
        let new_path = entries.join(";");
        env.set_value("PATH", &new_path)
            .context("Cannot write PATH to HKCU\\Environment")?;
        broadcast_environment_change();
        println!("  {} User PATH updated in registry", "✓".green());
        println!(
            "  New terminals and GUI apps (VSCode, GoLand, ...) will pick this up automatically."
        );
        println!("  Already-open ones need to be restarted to see it.");
    }

    Ok(())
}

/// Removes all gvm-managed entries from the Windows user PATH in the registry.
#[cfg(target_os = "windows")]
fn strip_windows_registry() -> Result<()> {
    use anyhow::Context;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("Cannot open HKCU\\Environment registry key")?;

    let current_path: String = env.get_value("PATH").unwrap_or_default();

    // Remove any entry that is inside the user's .gvm directory.
    let home = dirs::home_dir().unwrap_or_default();
    let gvm_root = home.join(".gvm");

    let filtered: Vec<&str> = current_path
        .split(';')
        .filter(|e| {
            let p = std::path::Path::new(e);
            !p.starts_with(&gvm_root)
        })
        .collect();

    let new_path = filtered.join(";");
    if new_path != current_path {
        env.set_value("PATH", &new_path)
            .context("Cannot write PATH to HKCU\\Environment")?;
        broadcast_environment_change();
        println!(
            "  {} Removed gvm entries from user PATH (registry)",
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

    // Smoke test only: confirms the WM_SETTINGCHANGE broadcast itself is a
    // well-formed call that doesn't crash the process. It never touches the
    // registry, so it's safe to run in CI - it can't verify that Explorer
    // actually refreshes its cached environment, which requires a real
    // interactive desktop session to observe.
    #[cfg(target_os = "windows")]
    #[test]
    fn broadcast_environment_change_does_not_panic() {
        broadcast_environment_change();
    }

    #[test]
    fn path_already_present_matches_case_insensitively() {
        let entries = vec![r"C:\Users\jhon\.gvm\current\bin".to_string()];
        let candidate = std::path::Path::new(r"c:\users\jhon\.gvm\current\bin");
        assert!(path_already_present(&entries, candidate));
    }

    #[test]
    fn path_already_present_is_false_for_unrelated_entry() {
        let entries = vec![r"C:\Users\jhon\.cargo\bin".to_string()];
        let candidate = std::path::Path::new(r"C:\Users\jhon\.gvm\current\bin");
        assert!(!path_already_present(&entries, candidate));
    }

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
            "Try: gvm setup --shell zsh"
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
            "eval gvm"
        }
        fn wrapper_function(&self) -> &'static str {
            "gvm() { command gvm \"$@\"; }"
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

        // Seed both files with gvm-managed blocks plus user content.
        std::fs::write(
            &sh.profile,
            "# user\nexport FOO=bar\n\n# gvm init\neval gvm\n",
        )
        .unwrap();
        std::fs::write(
            &sh.login_profile,
            "# gvm path\nexport PATH=\"$HOME/.gvm/current/bin:$PATH\"\n",
        )
        .unwrap();

        strip_all(&sh).unwrap();

        let profile_content = std::fs::read_to_string(&sh.profile).unwrap();
        assert!(!profile_content.contains("# gvm init"));
        assert!(profile_content.contains("export FOO=bar"));

        let login_content = std::fs::read_to_string(&sh.login_profile).unwrap();
        assert!(!login_content.contains("# gvm path"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn strip_all_is_a_noop_on_files_without_gvm_config() {
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
