//! `gvm prune` - remove unreferenced installed Go versions.
//!
//! A version is considered "referenced" when it is either:
//! - The current global default (`~/.gvm/version`), or
//! - Named in a `.go-version` file found by walking up from the current
//!   working directory, or
//! - Named in a `.go-version` file found anywhere inside `--scan-dir`
//!   (up to 5 levels deep, skipping hidden directories and `node_modules`).
//!
//! Every installed version that does not appear in the referenced set is
//! offered for removal after a confirmation prompt (skipped with `--force`).
//! Passing `--dry-run` prints the plan without deleting anything.

use anyhow::{bail, Result};
use colored::Colorize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::{config::Config, lock, prompt, toolchain, version::GoVersion};

/// Finds and removes unreferenced installed Go versions.
///
/// # Errors
///
/// Returns an error if the versions directory cannot be read, if stdin cannot
/// be read for the confirmation prompt, or if a directory cannot be removed.
pub fn run(config: &Config, force: bool, dry_run: bool, scan_dir: Option<&str>) -> Result<()> {
    let installed = toolchain::list_installed(config)?;

    if installed.is_empty() {
        println!("No Go versions installed.");
        return Ok(());
    }

    // ── Collect referenced version tags ───────────────────────────────────────

    let mut referenced: HashSet<String> = HashSet::new();

    // Global default version
    if let Ok(v) = toolchain::global_version(config) {
        referenced.insert(v.tag());
    }

    // Walk up from the current working directory for a local .go-version
    if let Ok(mut dir) = std::env::current_dir() {
        let mut depth = 0u8;
        loop {
            collect_go_version_file(&dir, &mut referenced);
            if depth >= 20 || !dir.pop() {
                break;
            }
            depth += 1;
        }
    }

    // Optional recursive scan directory
    if let Some(s) = scan_dir {
        let scan_path = PathBuf::from(s);
        if !scan_path.is_dir() {
            bail!("--scan-dir '{}' is not a directory", s);
        }
        scan_directory(&scan_path, 0, 5, &mut referenced);
    }

    // ── Classify installed versions ───────────────────────────────────────────

    let to_remove: Vec<&GoVersion> = installed
        .iter()
        .filter(|v| !referenced.contains(&v.tag()))
        .collect();

    // ── Print summary ─────────────────────────────────────────────────────────

    println!();
    let kept: Vec<_> = installed
        .iter()
        .filter(|v| referenced.contains(&v.tag()))
        .collect();

    if !kept.is_empty() {
        print!("  {} Referenced:", "→".cyan());
        for v in &kept {
            print!("  {}", v.tag().bold());
        }
        println!();
    }

    if to_remove.is_empty() {
        println!();
        println!(
            "{} All installed versions are referenced. Nothing to prune.",
            "✓".green()
        );
        return Ok(());
    }

    println!();
    println!("  Versions to remove:");
    let mut total_mb = 0.0f64;
    for v in &to_remove {
        let size = dir_size_mb(&config.version_dir(&v.tag()));
        total_mb += size;
        println!("    {} {}  ({:.1} MB)", "–".red(), v.tag(), size);
    }
    println!();
    println!(
        "  Total: {} version{}, ~{:.1} MB freed",
        to_remove.len(),
        if to_remove.len() == 1 { "" } else { "s" },
        total_mb
    );
    println!();

    if dry_run {
        println!(
            "{}  Dry run - nothing removed. Run without {} to prune.",
            "ℹ".cyan(),
            "--dry-run".cyan()
        );
        return Ok(());
    }

    // ── Confirmation ──────────────────────────────────────────────────────────

    if !force {
        let message = format!(
            "  Remove {} version{}? [y/N] ",
            to_remove.len(),
            if to_remove.len() == 1 { "" } else { "s" }
        );
        if !prompt::confirm(&message)? {
            bail!("Aborted.");
        }
        println!();
    }

    // ── Remove ────────────────────────────────────────────────────────────────

    let lock_path = config.root.join(".lock");
    for v in &to_remove {
        let dir = config.version_dir(&v.tag());
        lock::with_lock(&lock_path, || Ok(std::fs::remove_dir_all(&dir)?))?;
        println!("  {} Removed {}", "✓".green(), v.tag());
    }

    println!();
    println!(
        "{} Pruned {} version{}.",
        "✓".green(),
        to_remove.len(),
        if to_remove.len() == 1 { "" } else { "s" }
    );
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// If `dir` contains a `.go-version` file with a valid version, inserts the
/// tag into `referenced`.
fn collect_go_version_file(dir: &Path, referenced: &mut HashSet<String>) {
    let vf = dir.join(".go-version");
    if let Ok(raw) = std::fs::read_to_string(&vf) {
        if let Ok(v) = GoVersion::parse(raw.trim()) {
            referenced.insert(v.tag());
        }
    }
}

/// Recursively scans `dir` (up to `max_depth` levels) for `.go-version` files.
///
/// Hidden directories (name starts with `.`), `node_modules`, `target`, and
/// `vendor` are skipped to keep the scan fast and avoid false positives from
/// build artefacts.
pub fn scan_directory(dir: &Path, depth: u8, max_depth: u8, referenced: &mut HashSet<String>) {
    if depth > max_depth {
        return;
    }
    collect_go_version_file(dir, referenced);

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name();
        let n = name.to_string_lossy();
        // Skip directories that are never project roots.
        if n.starts_with('.') || n == "node_modules" || n == "target" || n == "vendor" {
            continue;
        }
        scan_directory(&entry.path(), depth + 1, max_depth, referenced);
    }
}

/// Returns the approximate on-disk size of `dir` in megabytes.
///
/// Symlinks are counted by their own inode size only (not the target content).
fn dir_size_mb(dir: &Path) -> f64 {
    fn walk(d: &Path) -> u64 {
        let Ok(rd) = std::fs::read_dir(d) else {
            return 0;
        };
        rd.filter_map(|e| e.ok())
            .fold(0u64, |acc, entry| match entry.metadata() {
                Ok(m) if m.is_dir() => acc + walk(&entry.path()),
                Ok(m) => acc + m.len(),
                Err(_) => acc,
            })
    }
    if !dir.exists() {
        return 0.0;
    }
    walk(dir) as f64 / (1024.0 * 1024.0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigMut;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn collect_valid_go_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".go-version"), "go1.21.13").unwrap();
        let mut set = HashSet::new();
        collect_go_version_file(dir.path(), &mut set);
        assert!(set.contains("go1.21.13"));
    }

    #[test]
    fn collect_ignores_invalid_content() {
        let dir = tmp();
        fs::write(dir.path().join(".go-version"), "not-a-version").unwrap();
        let mut set = HashSet::new();
        collect_go_version_file(dir.path(), &mut set);
        assert!(set.is_empty());
    }

    #[test]
    fn collect_missing_file_is_noop() {
        let dir = tmp();
        let mut set = HashSet::new();
        collect_go_version_file(dir.path(), &mut set);
        assert!(set.is_empty());
    }

    #[test]
    fn scan_finds_nested_go_versions() {
        let root = tmp();
        // root/.go-version
        fs::write(root.path().join(".go-version"), "go1.21.13").unwrap();
        // root/project-a/.go-version
        let proj_a = root.path().join("project-a");
        fs::create_dir(&proj_a).unwrap();
        fs::write(proj_a.join(".go-version"), "go1.22.4").unwrap();
        // root/project-b/sub/.go-version (depth 2)
        let proj_b_sub = root.path().join("project-b").join("sub");
        fs::create_dir_all(&proj_b_sub).unwrap();
        fs::write(proj_b_sub.join(".go-version"), "go1.23.0").unwrap();

        let mut referenced = HashSet::new();
        scan_directory(root.path(), 0, 5, &mut referenced);

        assert!(referenced.contains("go1.21.13"));
        assert!(referenced.contains("go1.22.4"));
        assert!(referenced.contains("go1.23"));
    }

    #[test]
    fn scan_skips_hidden_directories() {
        let root = tmp();
        // .git should be skipped
        let git_dir = root.path().join(".git");
        fs::create_dir(&git_dir).unwrap();
        fs::write(git_dir.join(".go-version"), "go1.19.1").unwrap();

        let mut referenced = HashSet::new();
        scan_directory(root.path(), 0, 5, &mut referenced);

        assert!(
            !referenced.contains("go1.19.1"),
            "Hidden directories should be skipped"
        );
    }

    #[test]
    fn scan_skips_target_and_vendor() {
        let root = tmp();
        for skip_dir in &["target", "vendor", "node_modules"] {
            let d = root.path().join(skip_dir);
            fs::create_dir(&d).unwrap();
            fs::write(d.join(".go-version"), "go1.18.1").unwrap();
        }

        let mut referenced = HashSet::new();
        scan_directory(root.path(), 0, 5, &mut referenced);

        assert!(
            !referenced.contains("go1.18.1"),
            "Build artifact directories should be skipped"
        );
    }

    #[test]
    fn scan_respects_max_depth() {
        let root = tmp();
        // Create a .go-version at depth 6 - should not be found at max_depth=5
        let deep = root
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e")
            .join("f");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join(".go-version"), "go1.17.1").unwrap();

        let mut referenced = HashSet::new();
        scan_directory(root.path(), 0, 5, &mut referenced);

        assert!(
            !referenced.contains("go1.17.1"),
            "Version beyond max depth should not be found"
        );
    }

    #[test]
    fn dir_size_of_nonexistent_dir_is_zero() {
        let dir = PathBuf::from("/nonexistent/directory/1234567890");
        assert_eq!(dir_size_mb(&dir), 0.0);
    }

    fn test_config(root: &Path) -> Config {
        Config {
            root: root.to_path_buf(),
        }
    }

    fn fake_install(config: &Config, tag: &str) {
        fs::create_dir_all(config.version_dir(tag)).unwrap();
        fs::write(config.version_dir(tag).join("marker.txt"), "x").unwrap();
    }

    #[test]
    fn run_prints_message_when_nothing_installed() {
        let dir = tmp();
        let config = test_config(dir.path());
        config.ensure_dirs().unwrap();

        // Should return Ok without touching the filesystem further.
        run(&config, true, false, None).unwrap();
    }

    #[test]
    fn run_dry_run_does_not_remove_anything() {
        let dir = tmp();
        let config = test_config(dir.path());
        config.ensure_dirs().unwrap();
        fake_install(&config, "go1.10.1");
        fake_install(&config, "go1.11.1");
        fs::write(config.version_file(), "go1.10.1").unwrap();

        run(&config, false, true, None).unwrap();

        assert!(config.version_dir("go1.10.1").exists());
        assert!(config.version_dir("go1.11.1").exists());
    }

    #[test]
    fn run_force_removes_unreferenced_versions() {
        let dir = tmp();
        let config = test_config(dir.path());
        config.ensure_dirs().unwrap();
        fake_install(&config, "go1.12.1");
        fake_install(&config, "go1.13.1");
        fs::write(config.version_file(), "go1.12.1").unwrap();

        run(&config, true, false, None).unwrap();

        assert!(
            config.version_dir("go1.12.1").exists(),
            "referenced version must survive"
        );
        assert!(
            !config.version_dir("go1.13.1").exists(),
            "unreferenced version must be removed"
        );
    }

    #[test]
    fn run_reports_nothing_to_prune_when_all_referenced() {
        let dir = tmp();
        let config = test_config(dir.path());
        config.ensure_dirs().unwrap();
        fake_install(&config, "go1.14.1");
        fs::write(config.version_file(), "go1.14.1").unwrap();

        run(&config, true, false, None).unwrap();

        assert!(config.version_dir("go1.14.1").exists());
    }

    #[test]
    fn run_errors_when_scan_dir_is_not_a_directory() {
        let dir = tmp();
        let config = test_config(dir.path());
        config.ensure_dirs().unwrap();
        fake_install(&config, "go1.15.1");

        let not_a_dir = dir.path().join("not-a-directory.txt");
        fs::write(&not_a_dir, "x").unwrap();

        let err = run(&config, true, false, Some(not_a_dir.to_str().unwrap())).unwrap_err();
        assert!(err.to_string().contains("is not a directory"));
    }
}
