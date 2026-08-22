//! `gvm implode` - completely remove gvm from the system.
//!
//! Removes, in order:
//! 1. The entire gvm data directory (`~/.gvm`) containing all installed Go
//!    versions.
//! 2. The `gvm` binary itself.
//! 3. Every gvm-managed line from the detected shell's profile file.
//!
//! A summary of what will be deleted is always printed before any action is
//! taken. When `--force` is omitted the user must confirm with `y`/`yes`.

use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::path::Path;

use crate::{config::Config, lock, prompt, shell};

/// Completely removes gvm and all associated data from the system.
///
/// `force` skips the interactive confirmation prompt; everything else is
/// identical between the two modes.
///
/// # Errors
///
/// Returns an error if:
/// - The confirmation prompt cannot be read from stdin.
/// - The data directory or binary cannot be removed.
///
/// Profile cleanup failures are emitted as warnings rather than errors so the
/// rest of the removal always completes.
pub fn run(config: &Config, force: bool) -> Result<()> {
    let versions_dir = config.versions_dir();
    let version_count = count_versions(&versions_dir);
    let data_mb = dir_size_mb(&config.root);

    let exe_path = std::env::current_exe().context("Cannot determine gvm binary location")?;

    let sh = shell::detect();
    let profile_path = sh.as_deref().and_then(|s| s.profile_path());

    // ── Print removal plan ────────────────────────────────────────────────────
    println!();
    println!(
        "{} This will permanently remove:",
        "gvm implode".red().bold()
    );
    println!();
    println!(
        "  {} {} ({} version{}, {:.1} MB)",
        "->".cyan(),
        config.root.display(),
        version_count,
        if version_count == 1 { "" } else { "s" },
        data_mb,
    );
    println!("  {} {}", "->".cyan(), exe_path.display());
    if let Some(ref p) = profile_path {
        println!("  {} {} (gvm lines removed)", "->".cyan(), p.display());
    }
    println!();

    // ── Confirm ───────────────────────────────────────────────────────────────
    if !force {
        let message = format!("  Type {} to confirm: ", "yes".bold());
        if !prompt::confirm(&message)? {
            bail!("Aborted.");
        }
        println!();
    }

    // ── Remove data directory ─────────────────────────────────────────────────
    if config.root.exists() {
        let lock_path = config.root.join(".lock");
        lock::with_lock(&lock_path, || Ok(std::fs::remove_dir_all(&config.root)?))
            .with_context(|| format!("Failed to remove {}", config.root.display()))?;
        println!("  {} Removed {}", "✓".green(), config.root.display());
    }

    // ── Clean shell profiles (interactive + login) ────────────────────────────
    if let Some(ref profile) = profile_path {
        match shell::strip_profile(profile) {
            Ok(true) => println!("  {} Cleaned {}", "✓".green(), profile.display()),
            Ok(false) => {}
            Err(e) => eprintln!(
                "  {} Could not clean {}: {e}",
                "!".yellow(),
                profile.display()
            ),
        }
    }
    // Also clean the login profile (e.g. ~/.profile) where gvm setup injects
    // the static PATH entry for GUI applications.
    if let Some(ref sh) = sh {
        if let Some(login_profile) = sh.login_profile_path() {
            match shell::strip_profile(&login_profile) {
                Ok(true) => println!("  {} Cleaned {}", "✓".green(), login_profile.display()),
                Ok(false) => {}
                Err(e) => eprintln!(
                    "  {} Could not clean {}: {e}",
                    "!".yellow(),
                    login_profile.display()
                ),
            }
        }
    }

    // ── Remove the binary (last, so earlier errors still have gvm available) ──
    remove_binary(&exe_path)?;
    println!("  {} Removed {}", "✓".green(), exe_path.display());

    println!();
    println!("{}", "gvm has been completely removed.".green().bold());
    println!("  Open a new terminal to apply the profile changes.");
    println!();

    Ok(())
}

// ── Directory helpers ─────────────────────────────────────────────────────────

/// Counts the number of installed Go versions in `versions_dir`.
fn count_versions(versions_dir: &Path) -> usize {
    if !versions_dir.exists() {
        return 0;
    }
    std::fs::read_dir(versions_dir)
        .map(|rd| rd.filter_map(|e| e.ok()).count())
        .unwrap_or(0)
}

/// Returns the approximate on-disk size of `root` in megabytes.
///
/// Symlinks are counted by their own metadata only (not the target) to avoid
/// loops and double-counting.
fn dir_size_mb(root: &Path) -> f64 {
    fn walk(dir: &Path) -> u64 {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return 0;
        };
        rd.filter_map(|e| e.ok())
            .fold(0u64, |acc, entry| match entry.metadata() {
                Ok(m) if m.is_dir() => acc + walk(&entry.path()),
                Ok(m) => acc + m.len(),
                Err(_) => acc,
            })
    }
    if !root.exists() {
        return 0.0;
    }
    walk(root) as f64 / (1024.0 * 1024.0)
}

// ── Binary removal ────────────────────────────────────────────────────────────

/// Removes the gvm executable.
///
/// On Unix the file is unlinked directly. On Windows a running executable
/// cannot be deleted while in use, so the binary is first renamed to
/// `gvm.exe.old` (which succeeds even for a running process) and then an
/// immediate deletion is attempted. If the deletion fails the `.old` file is
/// left behind - it is inert and can be deleted manually.
///
/// # Errors
///
/// Returns an error if the file cannot be renamed or removed.
fn remove_binary(exe: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let old = exe.with_file_name("gvm.exe.old");
        std::fs::rename(exe, &old).with_context(|| format!("Cannot rename {}", exe.display()))?;
        let _ = std::fs::remove_file(&old); // best-effort; ignored if still locked
    }
    #[cfg(not(windows))]
    {
        std::fs::remove_file(exe).with_context(|| format!("Cannot remove {}", exe.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn count_versions_returns_zero_when_dir_missing() {
        let dir = tempdir().unwrap();
        let versions = dir.path().join("versions");
        assert_eq!(count_versions(&versions), 0);
    }

    #[test]
    fn count_versions_counts_entries() {
        let dir = tempdir().unwrap();
        let versions = dir.path().join("versions");
        std::fs::create_dir_all(versions.join("go1.22.4")).unwrap();
        std::fs::create_dir_all(versions.join("go1.21.0")).unwrap();
        assert_eq!(count_versions(&versions), 2);
    }

    #[test]
    fn dir_size_mb_returns_zero_when_root_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("nope");
        assert_eq!(dir_size_mb(&root), 0.0);
    }

    #[test]
    fn dir_size_mb_sums_nested_file_sizes() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("a.bin"), vec![0u8; 1024 * 1024]).unwrap(); // 1 MiB
        std::fs::write(root.join("nested").join("b.bin"), vec![0u8; 512 * 1024]).unwrap(); // 0.5 MiB

        let size = dir_size_mb(&root);
        assert!((size - 1.5).abs() < 0.01, "expected ~1.5 MB, got {size}");
    }

    #[test]
    fn remove_binary_removes_the_file() {
        let dir = tempdir().unwrap();
        let exe = dir.path().join("gvm-fake-binary");
        std::fs::write(&exe, b"not a real binary").unwrap();

        remove_binary(&exe).unwrap();

        #[cfg(not(windows))]
        assert!(!exe.exists());

        #[cfg(windows)]
        {
            // On Windows the original path is renamed away and best-effort
            // deleted; either outcome (gone, or left as .old) is acceptable,
            // but the original path must no longer exist.
            assert!(!exe.exists());
        }
    }

    #[test]
    fn remove_binary_errors_when_file_missing() {
        let dir = tempdir().unwrap();
        let exe = dir.path().join("does-not-exist");
        assert!(remove_binary(&exe).is_err());
    }
}
