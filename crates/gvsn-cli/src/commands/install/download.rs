//! Download and verification logic for `gvsn install`.

use anyhow::{anyhow, Context, Result};
use colored::Colorize;

use gvsn_core::{
    checksum,
    config::Config,
    remote::{index, release::Release},
    version::GoVersion,
};
use gvsn_net::{download, http::HttpClient};

/// Downloads the archive for the specified version.
pub fn download_archive(
    client: &HttpClient,
    config: &Config,
    release: &Release,
    _version: &GoVersion,
) -> Result<ReleaseArchive> {
    let file = release
        .archive_for(index::host_os(), index::host_arch())
        .ok_or_else(|| {
            anyhow!(
                "No binary found for {}/{}",
                index::host_os(),
                index::host_arch()
            )
        })?;

    let url = index::download_url(&file.filename);
    let archive_path = config.tmp_dir().join(&file.filename);

    println!("{} Downloading {}...", "->".cyan(), file.filename.bold());

    download::fetch(client, &url, &archive_path)
        .with_context(|| format!("Failed to download {}", file.filename))?;

    println!("{} Verifying checksum...", "->".cyan());
    checksum::verify_sha256(&archive_path, &file.sha256).context("Archive checksum mismatch")?;

    Ok(ReleaseArchive { path: archive_path })
}

/// Archive information returned after download.
pub struct ReleaseArchive {
    pub path: std::path::PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gvsn_core::remote::release::ReleaseFile;
    use tempfile::tempdir;

    #[test]
    fn download_archive_errors_before_any_network_call_when_no_binary_matches() {
        // A release whose only file targets a platform that can never match
        // `index::host_os()`/`index::host_arch()` simultaneously should fail
        // fast in `archive_for` lookup, before any HTTP request is attempted.
        let release = Release {
            version: "go1.22.4".to_string(),
            stable: true,
            files: vec![ReleaseFile {
                filename: "go1.22.4.plan9-amd64.tar.gz".to_string(),
                os: "plan9".to_string(),
                arch: "amd64".to_string(),
                sha256: String::new(),
                size: 0,
                kind: "archive".to_string(),
            }],
        };

        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        let client = HttpClient::new(false, 0).unwrap();
        let version = GoVersion::parse("1.22.4").unwrap();

        match download_archive(&client, &config, &release, &version) {
            Err(e) => assert!(e.to_string().contains("No binary found")),
            Ok(_) => panic!("expected download_archive to fail without a matching binary"),
        }
    }
}
