//! `gvm doctor` - diagnose the gvm environment.
//!
//! Runs a series of checks and reports the result of each one. Exits with
//! status code `1` if any check fails, making the command suitable for CI
//! health checks and pre-commit hooks.
//!
//! # Checks performed
//!
//! 1. The `gvm` binary directory is on `PATH`.
//! 2. A global Go version has been set.
//! 3. The global version is installed on disk.
//! 4. `GOROOT` resolves to the correct directory.
//! 5. The current directory's `.go-version` (if present) refers to an
//!    installed version.
//! 6. The `gvm env` hook is present in the detected shell's profile.
//! 7. Only one `go` binary is active in `PATH` and it is gvm-managed.

use anyhow::Result;
use colored::Colorize;

use crate::{commands::setup, config::Config, shell, toolchain, version::GoVersion};

/// Runs all environment checks and prints a summary.
///
/// When `shell_str` is provided it overrides shell auto-detection for the
/// profile check. The function exits the process with code `1` if any issue
/// remains after checking (and, if `fix` is `true`, after attempting repairs).
///
/// When `fix` is `true`, two classes of issue are repaired automatically
/// after the checks run:
/// - A missing/stale shell hook or missing PATH entry (checks 1 and 7) is
///   fixed by running the same logic as `gvm setup`.
/// - A missing `~/.gvm/current` junction (check 5) is recreated by
///   re-pointing it at the installed global version, if one is set.
///
/// Everything else (an uninstalled global version, a `.go-version` pointing
/// at an uninstalled version, a shadowed non-gvm `go` on `PATH`) requires a
/// choice only the user can make (which version, whether to uninstall
/// something) and is never auto-fixed.
///
/// # Errors
///
/// This function only returns `Err` for unexpected I/O failures. Diagnostic
/// failures are reported to stdout and tracked in the `issues` counter rather
/// than being propagated as errors.
pub fn run(config: &Config, shell_str: Option<&str>, fix: bool) -> Result<()> {
    println!("Checking gvm environment...\n");
    let mut issues = 0u32;
    let go_name = if cfg!(windows) { "go.exe" } else { "go" };

    // ---- Check 1: gvm binary on PATH ----------------------------------------
    let mut path_issue = false;
    if shell::gvm_in_path() {
        ok("gvm binary is in PATH");
    } else {
        let dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.display().to_string()))
            .unwrap_or_else(|| "<unknown>".to_string());
        fail(&format!("gvm is NOT in PATH  (add '{dir}' to your PATH)"));
        issues += 1;
        path_issue = true;
    }

    // ---- Check 2 & 3: global version set and installed ----------------------
    let mut installed_global: Option<GoVersion> = None;
    match toolchain::global_version(config) {
        Err(_) => {
            fail("No global Go version set  (run 'gvm use <version>')");
            issues += 1;
        }
        Ok(v) => {
            ok(&format!("Global version: {}", v.tag().bold()));

            if toolchain::is_installed(config, &v) {
                ok(&format!("{} is installed", v.tag()));

                // ---- Check 4: GOROOT directory exists -----------------------
                let root = config.version_dir(&v.tag());
                ok(&format!("GOROOT -> {}", root.display()));
                installed_global = Some(v);
            } else {
                fail(&format!(
                    "{} is NOT installed  (run 'gvm install {}')",
                    v.tag(),
                    v.tag()
                ));
                issues += 1;
            }
        }
    }

    // ---- Check 5: ~/.gvm/current junction is set up -------------------------
    //
    // Users who installed gvm before v1.1.0 (which introduced the junction)
    // will not have ~/.gvm/current until they run `gvm use` with the new
    // binary. Warn them so `go` works in CMD, Git Bash and editors.
    let current_dir = config.current_dir();
    let current_go = current_dir.join("bin").join(go_name);
    let current_link_missing = !current_go.exists();
    if !current_link_missing {
        ok(&format!(
            "~/.gvm/current junction is configured ({})",
            current_dir.display()
        ));
    } else {
        fail("~/.gvm/current not set up  (run 'gvm use <version>' to enable universal shell support)");
        issues += 1;
    }

    // ---- Check 6: local .go-version consistency -----------------------------
    if let Ok(cwd) = std::env::current_dir() {
        let vf = cwd.join(".go-version");
        if vf.exists() {
            let raw = std::fs::read_to_string(&vf)
                .unwrap_or_default()
                .trim()
                .to_string();

            match crate::version::GoVersion::parse(&raw) {
                Ok(v) if toolchain::is_installed(config, &v) => {
                    ok(&format!(".go-version = {} (installed)", v.tag()));
                }
                Ok(v) => {
                    warn(&format!(
                        ".go-version = {} but NOT installed  (run 'gvm install {}')",
                        v.tag(),
                        v.tag()
                    ));
                    issues += 1;
                }
                Err(_) => {
                    warn(&format!(".go-version contains invalid version: '{raw}'"));
                    issues += 1;
                }
            }
        }
    }

    // ---- Check 7: shell profile contains the gvm init hook ------------------
    let sh = match shell_str {
        Some(s) => shell::from_str(s).ok(),
        None => shell::detect(),
    };

    let mut profile_issue = false;
    if let Some(sh) = &sh {
        match sh.profile_path() {
            None => warn(&format!("Cannot determine profile path for {}", sh.name())),
            Some(profile) => {
                if profile.exists() {
                    let content = std::fs::read_to_string(&profile).unwrap_or_default();
                    if content.contains("# gvm init") {
                        ok(&format!("Shell profile configured ({})", profile.display()));
                    } else {
                        fail(&format!(
                            "gvm init missing from {}  (run 'gvm setup')",
                            profile.display()
                        ));
                        issues += 1;
                        profile_issue = true;
                    }
                } else {
                    fail(&format!(
                        "Profile not found: {}  (run 'gvm setup')",
                        profile.display()
                    ));
                    issues += 1;
                    profile_issue = true;
                }
            }
        }
    }

    // ---- Check 8: single, gvm-managed `go` binary in PATH ------------------
    //
    // Scans every directory in PATH for a `go` (or `go.exe`) executable.
    // Reports a warning when multiple are found (shadowing is confusing) and
    // an error when the first - i.e. the active - one is not managed by gvm.
    // This is what `whereis go` surfaces on Linux: system-installed Go
    // alongside the gvm-managed one.
    let path_sep = if cfg!(windows) { ';' } else { ':' };
    let path_var = std::env::var("PATH").unwrap_or_default();
    let versions_dir = config.versions_dir();

    let go_paths = find_go_binaries(&path_var, path_sep, go_name);

    match classify_go_paths(&go_paths, &versions_dir, &current_dir) {
        GoPathStatus::None => {
            // No Go in PATH at all - only relevant if a version is supposed to be active.
        }
        GoPathStatus::SingleManaged(go) => {
            ok(&format!("Active Go is gvm-managed ({})", go.display()));
        }
        GoPathStatus::SingleUnmanaged(go) => {
            fail(&format!(
                "Active Go is NOT managed by gvm: {}",
                go.display()
            ));
            println!(
                "    Hint: remove the system Go package or run 'gvm setup' \
                      so gvm's PATH entry comes first."
            );
            issues += 1;
        }
        GoPathStatus::MultipleAllManaged(active) => {
            // All extra entries are other gvm-managed paths (e.g. both the versioned
            // directory and the ~/.gvm/current junction appear in PATH). Harmless.
            ok(&format!("Active Go is gvm-managed ({})", active.display()));
        }
        GoPathStatus::MultipleShadowedNonGvm { shadowed, .. } => {
            warn(&format!(
                "{} Go binaries found in PATH - {} non-gvm installation(s) are shadowed \
                 (consider removing them to avoid confusion):",
                shadowed.len() + 1,
                shadowed.len()
            ));
            for path in &shadowed {
                println!("    {} (non-gvm, shadowed)", path.display());
            }
        }
        GoPathStatus::MultipleActiveNotManaged { all } => {
            fail(&format!(
                "{} Go binaries in PATH - gvm's version is being shadowed:",
                all.len()
            ));
            for (i, path) in all.iter().enumerate() {
                let label = if i == 0 {
                    " (active - NOT gvm-managed)"
                } else if path.starts_with(&versions_dir) || path.starts_with(&current_dir) {
                    " (gvm-managed - shadowed)"
                } else {
                    " (shadowed)"
                };
                println!("    {}{label}", path.display());
            }
            println!(
                "    Hint: remove the system Go package or run 'gvm setup' \
                      to ensure gvm's PATH entry is first."
            );
            issues += 1;
        }
    }

    // ---- Auto-fix -------------------------------------------------------------
    if fix {
        let mut fixed_any = false;
        println!();

        if path_issue || profile_issue {
            match &sh {
                Some(sh) => match setup::run(Some(sh.name()), false) {
                    Ok(()) => {
                        println!("  {} Ran 'gvm setup' for {}.", "->".cyan(), sh.name());
                        if profile_issue {
                            println!("    Shell profile hook fixed.");
                            issues = issues.saturating_sub(1);
                        }
                        if path_issue {
                            println!(
                                "    PATH updated - open a new shell session for it to take effect."
                            );
                        }
                        fixed_any = true;
                    }
                    Err(e) => {
                        println!("  {} Could not run 'gvm setup': {e:#}", "x".red());
                    }
                },
                None => {
                    println!(
                        "  {} Cannot auto-fix PATH/shell hook: no shell detected.",
                        "!".yellow()
                    );
                }
            }
        }

        if current_link_missing {
            match &installed_global {
                Some(v) => match toolchain::set_active_version(config, v) {
                    Ok(()) => {
                        println!("  {} Recreated ~/.gvm/current -> {}", "->".cyan(), v.tag());
                        issues = issues.saturating_sub(1);
                        fixed_any = true;
                    }
                    Err(e) => {
                        println!("  {} Could not recreate ~/.gvm/current: {e:#}", "x".red());
                    }
                },
                None => {
                    println!(
                        "  {} Cannot auto-fix ~/.gvm/current: no installed global version set.",
                        "!".yellow()
                    );
                }
            }
        }

        if !fixed_any {
            println!("  {} Nothing to auto-fix.", "i".cyan());
        }
    }

    // ---- Summary ------------------------------------------------------------
    println!();
    if issues == 0 {
        println!("{} Everything looks good!", "✓".green().bold());
    } else {
        println!("{} {} issue(s) found.", "!".yellow().bold(), issues);
        std::process::exit(1);
    }

    Ok(())
}

// --- Private helpers ---------------------------------------------------------

fn ok(msg: &str) {
    println!("  {} {msg}", "✓".green());
}

fn fail(msg: &str) {
    println!("  {} {msg}", "x".red());
}

fn warn(msg: &str) {
    println!("  {} {msg}", "!".yellow());
}

/// Scans every directory in `path_var` (a PATH-like string, entries separated
/// by `path_sep`) for an executable named `go_name`, in order.
///
/// Pure with respect to the environment - callers pass in the PATH string and
/// separator explicitly rather than reading `std::env::var("PATH")` here, so
/// this function can be exercised with a synthetic PATH in tests.
fn find_go_binaries(path_var: &str, path_sep: char, go_name: &str) -> Vec<std::path::PathBuf> {
    path_var
        .split(path_sep)
        .map(std::path::Path::new)
        .filter_map(|dir| {
            let candidate = dir.join(go_name);
            if candidate.is_file() {
                Some(candidate)
            } else {
                None
            }
        })
        .collect()
}

/// Outcome of classifying the set of `go` binaries found on `PATH` against
/// gvm's managed directories.
#[derive(Debug, PartialEq, Eq)]
enum GoPathStatus {
    /// No `go` binary found anywhere on `PATH`.
    None,
    /// Exactly one `go` binary, and it is gvm-managed.
    SingleManaged(std::path::PathBuf),
    /// Exactly one `go` binary, and it is NOT gvm-managed.
    SingleUnmanaged(std::path::PathBuf),
    /// Multiple `go` binaries, all gvm-managed (e.g. the versioned directory
    /// and the `current` junction both appear on `PATH`). Harmless.
    MultipleAllManaged(std::path::PathBuf),
    /// Multiple `go` binaries; the active (first) one is gvm-managed, but one
    /// or more non-gvm installations are shadowed behind it.
    MultipleShadowedNonGvm {
        active: std::path::PathBuf,
        shadowed: Vec<std::path::PathBuf>,
    },
    /// Multiple `go` binaries; the active (first) one is NOT gvm-managed, so
    /// gvm's own installation is being shadowed.
    MultipleActiveNotManaged { all: Vec<std::path::PathBuf> },
}

/// Classifies `go_paths` (in PATH order) against gvm's `versions_dir` and
/// `current_dir` to determine what, if anything, is wrong with the active Go
/// binary resolution.
fn classify_go_paths(
    go_paths: &[std::path::PathBuf],
    versions_dir: &std::path::Path,
    current_dir: &std::path::Path,
) -> GoPathStatus {
    let is_managed =
        |p: &std::path::Path| p.starts_with(versions_dir) || p.starts_with(current_dir);

    match go_paths.len() {
        0 => GoPathStatus::None,
        1 => {
            let go = go_paths[0].clone();
            if is_managed(&go) {
                GoPathStatus::SingleManaged(go)
            } else {
                GoPathStatus::SingleUnmanaged(go)
            }
        }
        _ => {
            if is_managed(&go_paths[0]) {
                let shadowed: Vec<_> = go_paths
                    .iter()
                    .skip(1)
                    .filter(|p| !is_managed(p))
                    .cloned()
                    .collect();
                if shadowed.is_empty() {
                    GoPathStatus::MultipleAllManaged(go_paths[0].clone())
                } else {
                    GoPathStatus::MultipleShadowedNonGvm {
                        active: go_paths[0].clone(),
                        shadowed,
                    }
                }
            } else {
                GoPathStatus::MultipleActiveNotManaged {
                    all: go_paths.to_vec(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_go(dir: &std::path::Path, go_name: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let go = dir.join(go_name);
        std::fs::write(&go, b"").unwrap();
        go
    }

    #[test]
    fn find_go_binaries_returns_empty_for_empty_path() {
        assert!(find_go_binaries("", ':', "go").is_empty());
    }

    #[test]
    fn find_go_binaries_finds_executables_in_order() {
        // Use a separator that never collides with a Windows drive-letter
        // colon (e.g. `C:\...`) so the test is meaningful on every platform.
        let sep = '|';
        let root = tempdir().unwrap();
        let dir_a = root.path().join("a");
        let dir_b = root.path().join("b");
        let dir_c = root.path().join("c"); // no go binary here
        let go_a = make_go(&dir_a, "go");
        let go_b = make_go(&dir_b, "go");
        std::fs::create_dir_all(&dir_c).unwrap();

        let path_var = format!(
            "{}{sep}{}{sep}{}",
            dir_a.display(),
            dir_c.display(),
            dir_b.display()
        );
        let found = find_go_binaries(&path_var, sep, "go");
        assert_eq!(found, vec![go_a, go_b]);
    }

    #[test]
    fn find_go_binaries_ignores_directories_named_like_the_binary() {
        let root = tempdir().unwrap();
        let dir = root.path().join("bin");
        // Create a directory named "go" instead of a file - should not count.
        std::fs::create_dir_all(dir.join("go")).unwrap();

        let found = find_go_binaries(&dir.display().to_string(), '|', "go");
        assert!(found.is_empty());
    }

    #[test]
    fn classify_empty_is_none() {
        let versions_dir = std::path::Path::new("/gvm/versions");
        let current_dir = std::path::Path::new("/gvm/current");
        assert_eq!(
            classify_go_paths(&[], versions_dir, current_dir),
            GoPathStatus::None
        );
    }

    #[test]
    fn classify_single_managed_via_versions_dir() {
        let versions_dir = std::path::Path::new("/gvm/versions");
        let current_dir = std::path::Path::new("/gvm/current");
        let go = versions_dir.join("go1.22.4/bin/go");
        assert_eq!(
            classify_go_paths(std::slice::from_ref(&go), versions_dir, current_dir),
            GoPathStatus::SingleManaged(go)
        );
    }

    #[test]
    fn classify_single_managed_via_current_dir() {
        let versions_dir = std::path::Path::new("/gvm/versions");
        let current_dir = std::path::Path::new("/gvm/current");
        let go = current_dir.join("bin/go");
        assert_eq!(
            classify_go_paths(std::slice::from_ref(&go), versions_dir, current_dir),
            GoPathStatus::SingleManaged(go)
        );
    }

    #[test]
    fn classify_single_unmanaged() {
        let versions_dir = std::path::Path::new("/gvm/versions");
        let current_dir = std::path::Path::new("/gvm/current");
        let go = std::path::PathBuf::from("/usr/local/go/bin/go");
        assert_eq!(
            classify_go_paths(std::slice::from_ref(&go), versions_dir, current_dir),
            GoPathStatus::SingleUnmanaged(go)
        );
    }

    #[test]
    fn classify_multiple_all_managed_is_harmless() {
        let versions_dir = std::path::Path::new("/gvm/versions");
        let current_dir = std::path::Path::new("/gvm/current");
        let go1 = current_dir.join("bin/go");
        let go2 = versions_dir.join("go1.22.4/bin/go");
        assert_eq!(
            classify_go_paths(&[go1.clone(), go2], versions_dir, current_dir),
            GoPathStatus::MultipleAllManaged(go1)
        );
    }

    #[test]
    fn classify_multiple_shadowed_non_gvm() {
        let versions_dir = std::path::Path::new("/gvm/versions");
        let current_dir = std::path::Path::new("/gvm/current");
        let active = current_dir.join("bin/go");
        let shadowed = std::path::PathBuf::from("/usr/local/go/bin/go");
        assert_eq!(
            classify_go_paths(
                &[active.clone(), shadowed.clone()],
                versions_dir,
                current_dir
            ),
            GoPathStatus::MultipleShadowedNonGvm {
                active,
                shadowed: vec![shadowed],
            }
        );
    }

    #[test]
    fn classify_multiple_active_not_managed() {
        let versions_dir = std::path::Path::new("/gvm/versions");
        let current_dir = std::path::Path::new("/gvm/current");
        let system_go = std::path::PathBuf::from("/usr/local/go/bin/go");
        let managed_go = versions_dir.join("go1.22.4/bin/go");
        let all = vec![system_go, managed_go];
        assert_eq!(
            classify_go_paths(&all, versions_dir, current_dir),
            GoPathStatus::MultipleActiveNotManaged { all }
        );
    }
}
