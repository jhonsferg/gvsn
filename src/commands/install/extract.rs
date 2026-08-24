//! Archive extraction for `gvsn install`.

use anyhow::{Context, Result};

use crate::{archive::extract, config::Config, tempdir::TempDir};

/// Extracts the downloaded archive to a staging directory.
pub fn extract_archive(
    archive_path: &std::path::Path,
    config: &Config,
    version_tag: &str,
) -> Result<std::path::PathBuf> {
    // Use TempDir for auto-cleanup on error
    let staging_dir = TempDir::new_in(config.tmp_dir(), format!("src-{}", version_tag))?;

    extract::unpack(archive_path, staging_dir.path()).context("Failed to extract archive")?;

    // Verify expected layout: archive extracts to a "go/" subdirectory
    let source_root = staging_dir.path().join("go");
    if !source_root.exists() {
        anyhow::bail!(
            "Unexpected archive layout: expected 'go/' inside {}",
            staging_dir.path().display()
        );
    }

    // Prevent auto-cleanup since caller will handle the extracted source
    staging_dir.keep();
    Ok(source_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigMut;
    use tempfile::tempdir;

    /// Builds a real `.tar.gz` fixture with a single file at `go/bin/go` so
    /// `unpack`'s tar.gz path is exercised for real (not mocked).
    fn write_go_archive(dest: &std::path::Path) {
        let file = std::fs::File::create(dest).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut ar = tar::Builder::new(enc);
        let content = b"#!/bin/sh\necho fake go\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        ar.append_data(&mut header, "go/bin/go", &content[..])
            .unwrap();
        ar.finish().unwrap();
    }

    fn write_broken_layout_archive(dest: &std::path::Path) {
        let file = std::fs::File::create(dest).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut ar = tar::Builder::new(enc);
        let content = b"nope";
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        // No "go/" prefix - simulates an unexpected archive layout.
        ar.append_data(&mut header, "bin/go", &content[..]).unwrap();
        ar.finish().unwrap();
    }

    #[test]
    fn extract_archive_extracts_go_subdirectory() {
        let root = tempdir().unwrap();
        let config = Config {
            root: root.path().to_path_buf(),
        };
        config.ensure_dirs().unwrap();

        let archive_path = config.tmp_dir().join("go1.22.4.tar.gz");
        write_go_archive(&archive_path);

        let source_root = extract_archive(&archive_path, &config, "go1.22.4").unwrap();

        assert!(source_root.ends_with("go"));
        assert!(source_root.join("bin/go").exists());
    }

    #[test]
    fn extract_archive_errors_on_unexpected_layout() {
        let root = tempdir().unwrap();
        let config = Config {
            root: root.path().to_path_buf(),
        };
        config.ensure_dirs().unwrap();

        let archive_path = config.tmp_dir().join("broken.tar.gz");
        write_broken_layout_archive(&archive_path);

        let err = extract_archive(&archive_path, &config, "go1.22.4").unwrap_err();
        assert!(err.to_string().contains("Unexpected archive layout"));
    }
}
