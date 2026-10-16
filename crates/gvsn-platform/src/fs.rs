//! File-system utilities.
//!
//! Supplements the standard library with helpers needed for cross-platform
//! toolchain management. The primary concern is safe directory moves across
//! different drives or mount points, which `std::fs::rename` does not support.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Creates or replaces the `~/.gvsn/current` junction/symlink so it points to
/// `target`.
///
/// On Windows an NTFS junction (reparse point) is used: no elevation and no
/// Developer Mode is required. On Unix a directory symbolic link is used.
///
/// The new link is created at a temporary sibling path first and only swapped
/// into place once creation succeeds. This matters because junction creation
/// on Windows shells out to `mklink` and can be blocked by antivirus
/// software: if it failed after the existing link had already been removed,
/// `current` would be left pointing at nothing until the next successful
/// `gvsn use`. Building the replacement first means a failure here leaves the
/// existing link untouched.
pub fn set_version_link(link: &Path, target: &Path) -> Result<()> {
    let tmp_link = tmp_link_path(link);
    // Clean up any leftover temp link from a previous failed attempt.
    let _ = remove_link_if_exists(&tmp_link);

    create_link(&tmp_link, target)?;

    if let Err(e) = remove_link_if_exists(link) {
        let _ = remove_link_if_exists(&tmp_link);
        return Err(e);
    }

    std::fs::rename(&tmp_link, link).with_context(|| {
        format!(
            "Failed to activate new link at {} (staged at {})",
            link.display(),
            tmp_link.display()
        )
    })
}

/// Returns a temporary sibling path used to stage a new link before it is
/// swapped into place at `link`.
fn tmp_link_path(link: &Path) -> PathBuf {
    let file_name = link.file_name().unwrap_or_default().to_string_lossy();
    link.with_file_name(format!("{file_name}.tmp-{}", std::process::id()))
}

#[cfg(windows)]
fn remove_link_if_exists(link: &Path) -> Result<()> {
    match std::fs::symlink_metadata(link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            Err(anyhow::Error::from(e)).with_context(|| format!("Cannot stat {}", link.display()))
        }
        Ok(_) => std::fs::remove_dir(link)
            .with_context(|| format!("Failed to remove {}", link.display())),
    }
}

#[cfg(not(windows))]
fn remove_link_if_exists(link: &Path) -> Result<()> {
    match std::fs::symlink_metadata(link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            Err(anyhow::Error::from(e)).with_context(|| format!("Cannot stat {}", link.display()))
        }
        Ok(_) => std::fs::remove_file(link)
            .with_context(|| format!("Failed to remove {}", link.display())),
    }
}

#[cfg(windows)]
fn create_link(link: &Path, target: &Path) -> Result<()> {
    // mklink is a cmd.exe builtin, not a real executable: cmd re-scans its
    // whole command line for "/" followed by a letter as a switch, even
    // mid-argument. A forward-slash path (e.g. GVSN_DIR="C:/Users/x/.gvsn",
    // valid everywhere else on Windows, including PowerShell) makes cmd
    // choke on "/Users" as if it were an unknown switch. Normalizing to
    // backslashes first sidesteps the ambiguity entirely.
    let link_arg = windows_backslash_path(link);
    let target_arg = windows_backslash_path(target);

    // mklink /J creates an NTFS junction - no elevation or Developer Mode needed.
    let out = std::process::Command::new("cmd")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(&link_arg)
        .arg(&target_arg)
        .output()
        .context("Failed to run cmd for mklink")?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow::anyhow!(
            "Failed to create junction {} -> {}: {}",
            link.display(),
            target.display(),
            msg.trim()
        ));
    }
    Ok(())
}

/// Renders `path` as a string using `\` separators, regardless of whether it
/// was built with `/`. Only meaningful on Windows, where both separators are
/// normally interchangeable except inside cmd.exe builtins like `mklink`.
#[cfg(windows)]
fn windows_backslash_path(path: &Path) -> String {
    path.display().to_string().replace('/', "\\")
}

#[cfg(not(windows))]
fn create_link(link: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            link.display(),
            target.display()
        )
    })
}

/// Moves the directory at `src` to `dst`.
///
/// Attempts an atomic rename first. If the rename fails - which happens when
/// `src` and `dst` reside on different file-system volumes (common on Windows
/// when the temp directory is on a different drive than `~/.gvsn`) - it falls
/// back to a recursive copy followed by removal of the source.
///
/// # Errors
///
/// Returns an error if:
/// - Both the rename and the copy-then-delete fallback fail.
/// - Any file in `src` cannot be copied to `dst`.
/// - `src` cannot be removed after a successful copy.
pub fn move_dir(src: &Path, dst: &Path) -> Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-device move: copy every entry then delete the source tree.
            copy_dir_all(src, dst).with_context(|| {
                format!("Failed to copy {} to {}", src.display(), dst.display())
            })?;
            std::fs::remove_dir_all(src)
                .with_context(|| format!("Failed to remove {}", src.display()))
        }
    }
}

/// Recursively copies the directory tree rooted at `src` into `dst`.
///
/// `dst` is created if it does not exist. Files are copied individually;
/// symbolic links are not followed - they are copied as regular files
/// pointing to the same content.
///
/// # Errors
///
/// Returns an error if any entry cannot be read, created, or copied.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn move_dir_relocates_contents_via_rename() {
        let root = tempdir().unwrap();
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("file.txt"), b"hello").unwrap();
        std::fs::write(src.join("nested").join("inner.txt"), b"world").unwrap();

        move_dir(&src, &dst).unwrap();

        assert!(!src.exists());
        assert_eq!(
            std::fs::read_to_string(dst.join("file.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("nested").join("inner.txt")).unwrap(),
            "world"
        );
    }

    #[test]
    fn move_dir_errors_when_source_missing() {
        let root = tempdir().unwrap();
        let src = root.path().join("does-not-exist");
        let dst = root.path().join("dst");
        assert!(move_dir(&src, &dst).is_err());
    }

    #[test]
    fn copy_dir_all_recreates_full_tree() {
        let root = tempdir().unwrap();
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        std::fs::create_dir_all(src.join("a").join("b")).unwrap();
        std::fs::write(src.join("top.txt"), b"top").unwrap();
        std::fs::write(src.join("a").join("b").join("deep.txt"), b"deep").unwrap();

        copy_dir_all(&src, &dst).unwrap();

        // Original tree must remain untouched.
        assert!(src.exists());
        assert_eq!(std::fs::read_to_string(dst.join("top.txt")).unwrap(), "top");
        assert_eq!(
            std::fs::read_to_string(dst.join("a").join("b").join("deep.txt")).unwrap(),
            "deep"
        );
    }

    #[test]
    fn set_version_link_creates_and_replaces() {
        let root = tempdir().unwrap();
        let target_a = root.path().join("version-a");
        let target_b = root.path().join("version-b");
        std::fs::create_dir_all(&target_a).unwrap();
        std::fs::create_dir_all(&target_b).unwrap();
        std::fs::write(target_a.join("marker.txt"), b"a").unwrap();
        std::fs::write(target_b.join("marker.txt"), b"b").unwrap();

        let link = root.path().join("current");

        set_version_link(&link, &target_a).unwrap();
        assert_eq!(
            std::fs::read_to_string(link.join("marker.txt")).unwrap(),
            "a"
        );

        // Re-pointing the link should atomically replace it, not fail because
        // it already exists.
        set_version_link(&link, &target_b).unwrap();
        assert_eq!(
            std::fs::read_to_string(link.join("marker.txt")).unwrap(),
            "b"
        );
    }

    #[test]
    fn remove_link_if_exists_is_noop_when_absent() {
        let root = tempdir().unwrap();
        let link = root.path().join("nonexistent-link");
        assert!(remove_link_if_exists(&link).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_backslash_path_converts_forward_slashes() {
        let path = Path::new("C:/Users/x/.gvsn/current");
        assert_eq!(windows_backslash_path(path), r"C:\Users\x\.gvsn\current");
    }

    // Regression test for a real failure found while testing on Windows:
    // GVSN_DIR set with forward slashes (valid everywhere on Windows,
    // including PowerShell) made `cmd /c mklink` choke on "/Users" as if it
    // were an unrecognised switch, because mklink is a cmd.exe builtin that
    // rescans the whole command line for "/<letter>" tokens.
    #[cfg(windows)]
    #[test]
    fn set_version_link_succeeds_with_forward_slash_paths() {
        let root = tempdir().unwrap();
        let target = root.path().join("version-a");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("marker.txt"), b"a").unwrap();

        let link = root.path().join("current");

        // Rebuild both paths using forward slashes, as a user-set GVSN_DIR
        // containing "/" would produce.
        let link_fwd = Path::new(&link.display().to_string().replace('\\', "/")).to_path_buf();
        let target_fwd = Path::new(&target.display().to_string().replace('\\', "/")).to_path_buf();

        set_version_link(&link_fwd, &target_fwd).unwrap();
        assert_eq!(
            std::fs::read_to_string(link.join("marker.txt")).unwrap(),
            "a"
        );
    }
}
