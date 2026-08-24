//! Bootstrap compiler resolution for building Go from source.

use crate::{
    archive::{download, extract},
    config::Config,
    http::HttpClient,
    remote::{index, release::Release},
    tempdir::TempDir,
    toolchain,
    user_version::VersionSpec,
    version::GoVersion,
};
use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use std::path::PathBuf;

/// Represents a resolved bootstrap compiler.
#[derive(Debug)]
pub struct Bootstrap {
    /// Directory passed as `GOROOT_BOOTSTRAP` to `make.bash`.
    pub path: PathBuf,
    /// If set, this temporary directory is removed after compilation (temp download).
    _cleanup: Option<TempDir>,
    pub label: String,
}

/// Resolves the bootstrap Go compiler to use.
///
/// Priority:
/// 1. `--bootstrap VERSION` - must already be installed via gvsn.
/// 2. Previous version (patch-1, or latest of minor-1 when patch==0) if installed locally.
/// 3. Download that same previous version temporarily; removed after compilation.
pub fn resolve_bootstrap(
    config: &Config,
    client: &HttpClient,
    target: &GoVersion,
    bootstrap_spec: Option<&str>,
    releases: &[Release],
) -> Result<Bootstrap> {
    // Explicit --bootstrap flag.
    if let Some(spec_str) = bootstrap_spec {
        let spec = VersionSpec::parse(spec_str)?;
        let bv = toolchain::resolve_installed(config, &spec).map_err(|_| {
            anyhow!(
                "Bootstrap version '{}' is not installed. Run 'gvsn install {}' first.",
                spec_str,
                spec_str
            )
        })?;
        return Ok(Bootstrap {
            path: config.version_dir(&bv.tag()),
            _cleanup: None,
            label: bv.tag(),
        });
    }

    // Compute the closest older version spec:
    //   patch > 0 → exact previous patch (e.g. 1.25.10 for target 1.25.11)
    //   patch == 0 → latest patch of previous minor (e.g. 1.24.x for target 1.25.0)
    let prev_spec = if target.patch > 0 {
        VersionSpec::Exact {
            major: target.major,
            minor: target.minor,
            patch: target.patch - 1,
        }
    } else {
        VersionSpec::Partial {
            major: target.major,
            minor: target.minor.saturating_sub(1),
        }
    };

    // Check if the previous version is already installed - use it without downloading.
    if let Ok(bv) = toolchain::resolve_installed(config, &prev_spec) {
        return Ok(Bootstrap {
            path: config.version_dir(&bv.tag()),
            _cleanup: None,
            label: bv.tag(),
        });
    }

    // Not installed locally - download that specific version as a temporary bootstrap.
    let b_release = index::resolve(&prev_spec, releases).with_context(|| {
        format!(
            "Could not find a bootstrap release for {}. \
             Install a Go version first with 'gvsn install latest', \
             or specify one with --bootstrap.",
            prev_spec
        )
    })?;

    let b_version = b_release
        .go_version()
        .ok_or_else(|| anyhow!("Cannot parse bootstrap version tag"))?;

    let b_archive = b_release
        .archive_for(index::host_os(), index::host_arch())
        .ok_or_else(|| {
            anyhow!(
                "No bootstrap binary available for {}/{}.",
                index::host_os(),
                index::host_arch()
            )
        })?;

    // Create a TempDir for the bootstrap staging - it will auto-cleanup on drop
    let staging_dir = TempDir::new_in(config.tmp_dir(), format!("bootstrap-{}", b_version.tag()))?;

    println!(
        "{} Downloading bootstrap {} (temporary, removed after build)...",
        "->".cyan(),
        b_version.tag().bold()
    );

    let archive_path = config.tmp_dir().join(&b_archive.filename);
    if let Err(e) = download::fetch(
        client,
        &index::download_url(&b_archive.filename),
        &archive_path,
    ) {
        let _ = std::fs::remove_file(&archive_path);
        return Err(e).context("Failed to download bootstrap compiler");
    }
    if !b_archive.sha256.is_empty() {
        if let Err(e) = download::verify_sha256(&archive_path, &b_archive.sha256) {
            let _ = std::fs::remove_file(&archive_path);
            return Err(e);
        }
    }

    extract::unpack(&archive_path, staging_dir.path())
        .context("Failed to extract bootstrap compiler")?;
    let _ = std::fs::remove_file(&archive_path);

    // Bootstrap archive also extracts to a `go/` subdirectory.
    let bootstrap_root = staging_dir.path().join("go");
    if !bootstrap_root.exists() {
        anyhow::bail!("Bootstrap archive had an unexpected layout");
    }

    // Keep the TempDir so it cleans up when Bootstrap is dropped
    Ok(Bootstrap {
        path: bootstrap_root,
        _cleanup: Some(staging_dir),
        label: format!("{} (downloaded temporarily)", b_version.tag()),
    })
}

/// No explicit cleanup needed - TempDir cleans up automatically on drop.
#[allow(dead_code)]
pub fn cleanup_bootstrap(_b: &Bootstrap) {
    // TempDir handles cleanup automatically when Bootstrap is dropped.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpClient;
    use tempfile::tempdir;

    /// Creates a fake installed version directory under `config.versions_dir()`
    /// so `toolchain::resolve_installed` can find it without touching the network.
    fn fake_install(config: &Config, tag: &str) {
        std::fs::create_dir_all(config.version_dir(tag)).unwrap();
    }

    fn test_config(root: &std::path::Path) -> Config {
        Config {
            root: root.to_path_buf(),
        }
    }

    #[test]
    fn explicit_bootstrap_flag_uses_installed_version() {
        let dir = tempdir().unwrap();
        let config = test_config(dir.path());
        fake_install(&config, "go1.24.3");

        let client = HttpClient::new(false, 0).unwrap();
        let target = GoVersion::parse("1.25.0").unwrap();

        let b = resolve_bootstrap(&config, &client, &target, Some("1.24.3"), &[]).unwrap();

        assert_eq!(b.path, config.version_dir("go1.24.3"));
        assert_eq!(b.label, "go1.24.3");
    }

    #[test]
    fn explicit_bootstrap_flag_errors_when_not_installed() {
        let dir = tempdir().unwrap();
        let config = test_config(dir.path());
        let client = HttpClient::new(false, 0).unwrap();
        let target = GoVersion::parse("1.25.0").unwrap();

        let err = resolve_bootstrap(&config, &client, &target, Some("1.24.0"), &[]).unwrap_err();
        assert!(err.to_string().contains("not installed"));
    }

    #[test]
    fn previous_patch_version_used_when_installed_locally() {
        let dir = tempdir().unwrap();
        let config = test_config(dir.path());
        // Target is 1.25.11 (patch > 0) so the previous patch is 1.25.10.
        fake_install(&config, "go1.25.10");

        let client = HttpClient::new(false, 0).unwrap();
        let target = GoVersion::parse("1.25.11").unwrap();

        let b = resolve_bootstrap(&config, &client, &target, None, &[]).unwrap();

        assert_eq!(b.path, config.version_dir("go1.25.10"));
        assert_eq!(b.label, "go1.25.10");
    }

    #[test]
    fn previous_minor_latest_patch_used_when_target_patch_is_zero() {
        let dir = tempdir().unwrap();
        let config = test_config(dir.path());
        // Target is 1.25.0 (patch == 0) so the previous minor's latest patch
        // (1.24.3, chosen over 1.24.1) should be used.
        fake_install(&config, "go1.24.1");
        fake_install(&config, "go1.24.3");

        let client = HttpClient::new(false, 0).unwrap();
        let target = GoVersion::parse("1.25.0").unwrap();

        let b = resolve_bootstrap(&config, &client, &target, None, &[]).unwrap();

        assert_eq!(b.path, config.version_dir("go1.24.3"));
        assert_eq!(b.label, "go1.24.3");
    }

    #[test]
    fn falls_through_to_download_lookup_when_nothing_installed_locally() {
        let dir = tempdir().unwrap();
        let config = test_config(dir.path());
        let client = HttpClient::new(false, 0).unwrap();
        let target = GoVersion::parse("1.25.11").unwrap();

        // No local installs and no releases supplied - must fail with the
        // "could not find a bootstrap release" error rather than panicking or
        // attempting a network call.
        let err = resolve_bootstrap(&config, &client, &target, None, &[]).unwrap_err();
        assert!(err
            .to_string()
            .contains("Could not find a bootstrap release"));
    }
}
