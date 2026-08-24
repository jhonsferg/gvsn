//! `gvsn upgrade` - self-update the gvsn binary.
//!
//! Queries the GitHub Releases API for the latest `jhonsferg/gvsn` release and
//! compares the published tag with the version embedded in this binary at
//! compile time. When a newer version is available the correct platform archive
//! is downloaded, the `gvsn` binary is extracted from it, and the running
//! executable is replaced in-place.
//!
//! # Replacement strategy
//!
//! - **Unix**: the archive is downloaded to a temp file, `gvsn` is extracted,
//!   staged next to the target executable, and then atomically moved over the
//!   current executable with `rename(2)`.
//! - **Windows**: the archive is downloaded to a temp `.zip`, `gvsn.exe` is
//!   extracted, copied into the install directory (a `rename` from the temp
//!   directory can fail if it's on a different volume), the running binary is
//!   renamed (freeing the name while it stays in use), then the staged binary
//!   is moved to the original path.

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;

use crate::{
    archive::download,
    config::Config,
    gvsn_release::{api_base, parse_semver, GithubRelease, REPO},
    http::HttpClient,
    lock,
};

/// Returns the GitHub download base URL, overridable via `GVSN_TEST_DL_BASE`.
fn dl_base() -> String {
    std::env::var("GVSN_TEST_DL_BASE").unwrap_or_else(|_| "https://github.com".to_owned())
}

/// Checks for a newer gvsn release and replaces the binary if one is found.
///
/// When `force` is `true` the version comparison is skipped and the latest
/// binary is always downloaded and installed.
///
/// # Errors
///
/// Returns an error if:
/// - The GitHub API cannot be reached or returns an unexpected response.
/// - The archive cannot be downloaded or extracted.
/// - The in-place replacement fails (e.g. permission denied).
pub fn run(config: &Config, client: &HttpClient, force: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    println!("{} Checking for updates...", "->".cyan());

    let api_url = format!("{}/repos/{REPO}/releases/latest", api_base());
    crate::http::log_request(client, "GET", &api_url);

    let mut response = match client.agent().get(&api_url).call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => anyhow::bail!(
            "No releases found for {REPO}. \
             The project may not have published a release yet."
        ),
        Err(e) => {
            return Err(anyhow::anyhow!(e))
                .context("Failed to reach GitHub API - check your internet connection")
        }
    };

    crate::http::log_response(
        client,
        response.status().as_u16(),
        response.status().canonical_reason().unwrap_or(""),
        response.headers(),
    );

    let release: GithubRelease = response
        .body_mut()
        .read_json()
        .context("Failed to parse GitHub release response")?;

    let latest_tag = release.tag_name.trim_start_matches('v');

    let latest = parse_semver(latest_tag)
        .ok_or_else(|| anyhow::anyhow!("Cannot parse version tag '{}'", release.tag_name))?;
    let current_parsed = parse_semver(current)
        .ok_or_else(|| anyhow::anyhow!("Cannot parse current version '{current}'"))?;

    println!("  Current: {}", format!("v{current}").bold());
    println!("  Latest:  {}", format!("v{latest_tag}").bold());

    if !force && current_parsed >= latest {
        println!();
        println!("{} gvsn is already up to date.", "✓".green());
        return Ok(());
    }

    let archive_name = release_archive_name();
    let url = format!(
        "{}/{REPO}/releases/download/{}/{archive_name}",
        dl_base(),
        release.tag_name,
    );

    println!("{} Downloading {}...", "->".cyan(), archive_name.bold());

    let tmp_archive = tmp_archive_path()?;
    if let Err(e) = download::fetch(client, &url, &tmp_archive) {
        let _ = std::fs::remove_file(&tmp_archive);
        return Err(e);
    }

    let tmp_bin = extract_upgrade_binary(&tmp_archive);
    let _ = std::fs::remove_file(&tmp_archive);
    let tmp_bin = tmp_bin?;

    // Replace the running binary under a lock so two concurrent `gvsn upgrade`
    // invocations cannot interleave their rename steps against the same
    // executable path.
    let lock_path = config.root.join(".lock");
    if let Err(e) = lock::with_lock(&lock_path, || replace_binary(&tmp_bin)) {
        let _ = std::fs::remove_file(&tmp_bin);
        return Err(e);
    }

    println!();
    println!(
        "{} gvsn upgraded to {}.",
        "✓".green(),
        format!("v{latest_tag}").bold(),
    );
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the release archive name for the current platform and architecture.
///
/// Convention: `gvsn_{os}_{arch}.tar.gz` on Unix, `gvsn_{os}_{arch}.zip` on
/// Windows. Arch names follow Go's `GOARCH` naming except that Linux/Darwin
/// use `aarch64` for ARM64 to match the binary names in the release matrix.
/// Windows ARM64 uses `arm64` to match Go's own naming.
fn release_archive_name() -> String {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "linux"
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        if cfg!(target_os = "windows") {
            "arm64"
        } else {
            "aarch64"
        }
    } else if cfg!(target_arch = "arm") {
        "armv7"
    } else if cfg!(target_arch = "x86") {
        "386"
    } else if cfg!(target_arch = "riscv64") {
        "riscv64"
    } else if cfg!(target_arch = "s390x") {
        "s390x"
    } else if cfg!(target_arch = "powerpc64") {
        "ppc64le"
    } else {
        "x86_64"
    };

    let ext = if cfg!(windows) { ".zip" } else { ".tar.gz" };

    format!("gvsn_{os}_{arch}{ext}")
}

/// Returns a temp path for the downloaded archive file.
fn tmp_archive_path() -> Result<std::path::PathBuf> {
    let ext = if cfg!(windows) { ".zip" } else { ".tar.gz" };
    Ok(std::env::temp_dir().join(format!("gvsn-upgrade-{}{ext}", std::process::id())))
}

/// Extracts the `gvsn` (or `gvsn.exe`) binary from the downloaded archive into
/// a new temporary file and returns its path.
///
/// The extraction is self-contained: no system `tar`, `unzip`, or similar
/// tool is required. The `flate2`, `tar`, and `zip` crates do the work.
fn extract_upgrade_binary(archive: &Path) -> Result<std::path::PathBuf> {
    let out = std::env::temp_dir().join(format!("gvsn-upgrade-bin-{}", std::process::id()));

    #[cfg(not(windows))]
    {
        let binary_filename = "gvsn";

        let file = std::fs::File::open(archive)
            .with_context(|| format!("Cannot open archive {}", archive.display()))?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut ar = tar::Archive::new(gz);

        for entry in ar.entries().context("Failed to read tar entries")? {
            let mut entry = entry.context("Failed to read tar entry")?;
            let name = entry
                .path()
                .context("Invalid tar entry path")?
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_owned();

            if name == binary_filename {
                entry
                    .unpack(&out)
                    .context("Failed to extract gvsn binary from archive")?;

                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755))
                    .context("Cannot set execute permission on extracted binary")?;

                return Ok(out);
            }
        }

        anyhow::bail!("Archive did not contain a file named '{binary_filename}'");
    }

    #[cfg(windows)]
    {
        let binary_filename = "gvsn.exe";

        let file = std::fs::File::open(archive)
            .with_context(|| format!("Cannot open archive {}", archive.display()))?;
        let mut zip = zip::ZipArchive::new(file).context("Failed to read zip archive")?;

        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).context("Failed to read zip entry")?;
            if entry.name() == binary_filename {
                let mut dest =
                    std::fs::File::create(&out).context("Cannot create temp binary file")?;
                std::io::copy(&mut entry, &mut dest)
                    .context("Failed to extract gvsn.exe from zip")?;
                return Ok(out);
            }
        }

        anyhow::bail!("Archive did not contain a file named '{binary_filename}'");
    }
}

/// Replaces the current gvsn executable with the file at `new_binary`.
///
/// On Unix the new binary is first copied to a hidden temp file inside the
/// same directory as the target executable (so that the final `rename(2)` is
/// guaranteed to be same-filesystem and therefore atomic). `/tmp` is often a
/// separate `tmpfs`, which would cause `rename(2)` to fail with `EXDEV` if
/// used directly. On Windows the running binary is renamed first (freeing the
/// name while keeping the file in use) and then the new binary takes the
/// original name.
fn replace_binary(new_binary: &Path) -> Result<()> {
    let exe = std::env::current_exe().context("Cannot determine gvsn binary location")?;
    replace_binary_at(new_binary, &exe)
}

/// Does the actual replacement work for [`replace_binary`], taking the
/// target executable path as a parameter so it can be exercised in tests
/// against a fake "installed" binary instead of the real running process.
fn replace_binary_at(new_binary: &Path, exe: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let staged = exe.with_file_name(format!(".gvsn-upgrade-{}", std::process::id()));

        if let Err(e) = std::fs::copy(new_binary, &staged) {
            let _ = std::fs::remove_file(new_binary);
            return Err(e).context("Cannot stage new binary in install directory");
        }
        let _ = std::fs::remove_file(new_binary);

        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .context("Cannot set execute permission on new binary")?;

        if let Err(e) = std::fs::rename(&staged, exe) {
            let _ = std::fs::remove_file(&staged);
            return Err(e).with_context(|| format!("Cannot replace {}", exe.display()));
        }
    }

    #[cfg(windows)]
    {
        // `new_binary` lives under `std::env::temp_dir()`, which may be on a
        // different volume than the install directory (e.g. TEMP redirected
        // to another drive, or a portable install on removable media).
        // `rename` fails across volumes, so stage the file into the install
        // directory with `copy` first - same pattern already used on Unix
        // and in `fs::move_dir` - so the swap below is always a same-volume
        // rename.
        let staged = exe.with_file_name(format!("gvsn-upgrade-{}.exe", std::process::id()));
        if let Err(e) = std::fs::copy(new_binary, &staged) {
            let _ = std::fs::remove_file(new_binary);
            return Err(e).context("Cannot stage new binary in install directory");
        }
        let _ = std::fs::remove_file(new_binary);

        let old = exe.with_file_name("gvsn.exe.old");
        if let Err(e) = std::fs::rename(exe, &old) {
            let _ = std::fs::remove_file(&staged);
            return Err(e)
                .with_context(|| format!("Cannot rename current binary {}", exe.display()));
        }

        if let Err(e) = std::fs::rename(&staged, exe) {
            let _ = std::fs::rename(&old, exe);
            let _ = std::fs::remove_file(&staged);
            return Err(e).context("Cannot place new binary - rolled back to previous version");
        }

        let _ = std::fs::remove_file(&old);
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{extract_upgrade_binary, release_archive_name, replace_binary_at};
    use std::path::Path;
    use tempfile::tempdir;

    /// Binary filename expected inside the release archive on this platform.
    #[cfg(not(windows))]
    const BIN_NAME: &str = "gvsn";
    #[cfg(windows)]
    const BIN_NAME: &str = "gvsn.exe";

    /// Builds a fixture archive (tar.gz on Unix, zip on Windows) containing a
    /// single file named `BIN_NAME` with the given content, and writes it to
    /// `dest`.
    #[cfg(not(windows))]
    fn write_fixture_archive(dest: &Path, content: &[u8]) {
        let file = std::fs::File::create(dest).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut ar = tar::Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        ar.append_data(&mut header, BIN_NAME, content).unwrap();
        ar.finish().unwrap();
    }

    #[cfg(windows)]
    fn write_fixture_archive(dest: &Path, content: &[u8]) {
        let file = std::fs::File::create(dest).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file::<_, ()>(BIN_NAME, zip::write::FileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, content).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn extract_upgrade_binary_finds_and_extracts_entry() {
        let dir = tempdir().unwrap();
        let archive_ext = if cfg!(windows) { "zip" } else { "tar.gz" };
        let archive_path = dir.path().join(format!("gvsn.{archive_ext}"));
        write_fixture_archive(&archive_path, b"fake binary contents");

        let extracted = extract_upgrade_binary(&archive_path).unwrap();
        let contents = std::fs::read(&extracted).unwrap();
        assert_eq!(contents, b"fake binary contents");

        let _ = std::fs::remove_file(&extracted);
    }

    #[test]
    fn extract_upgrade_binary_errors_when_entry_missing() {
        let dir = tempdir().unwrap();
        let archive_ext = if cfg!(windows) { "zip" } else { "tar.gz" };
        let archive_path = dir.path().join(format!("empty.{archive_ext}"));

        #[cfg(not(windows))]
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let ar = tar::Builder::new(enc);
            ar.into_inner().unwrap().finish().unwrap();
        }
        #[cfg(windows)]
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let zip = zip::ZipWriter::new(file);
            zip.finish().unwrap();
        }

        let err = extract_upgrade_binary(&archive_path).unwrap_err();
        assert!(err.to_string().contains("did not contain"));
    }

    #[test]
    fn replace_binary_at_swaps_target_with_new_binary() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("gvsn-fake-exe");
        std::fs::write(&target, b"old contents").unwrap();

        let new_binary = dir.path().join("staged-new-binary");
        std::fs::write(&new_binary, b"new contents").unwrap();

        replace_binary_at(&new_binary, &target).unwrap();

        let final_contents = std::fs::read(&target).unwrap();
        assert_eq!(final_contents, b"new contents");
        assert!(!new_binary.exists(), "staged file must be consumed");
    }

    #[test]
    fn archive_name_has_correct_format() {
        let name = release_archive_name();
        assert!(name.starts_with("gvsn_"));
        #[cfg(windows)]
        assert!(name.ends_with(".zip"));
        #[cfg(not(windows))]
        assert!(name.ends_with(".tar.gz"));
    }
}
