//! Archive extraction for `.tar.gz` and `.zip` files.
//!
//! Go releases use `.tar.gz` on Linux and macOS and `.zip` on Windows.
//! [`unpack`] dispatches to the appropriate implementation based on the file
//! extension and displays a spinner while extraction is in progress.

use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;

/// Extracts `archive` into the `dest` directory.
///
/// The archive format is determined from the file extension:
/// - `.tar.gz` - extracted with `flate2` + `tar`.
/// - `.zip` - extracted with the `zip` crate.
///
/// A spinner is shown for the duration of the extraction and cleared
/// afterwards, regardless of whether extraction succeeds or fails.
///
/// # Errors
///
/// Returns an error if:
/// - The archive file extension is not `.tar.gz` or `.zip`.
/// - The archive cannot be opened or is malformed.
/// - Any entry cannot be written to `dest`.
pub fn unpack(archive: &Path, dest: &Path) -> Result<()> {
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("  {spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message(format!("Extracting {name}..."));
    pb.enable_steady_tick(std::time::Duration::from_millis(80));

    let result = if name.ends_with(".tar.gz") {
        unpack_tar_gz(archive, dest)
    } else if name.ends_with(".zip") {
        unpack_zip(archive, dest)
    } else {
        bail!("Unsupported archive format: {name}")
    };

    pb.finish_and_clear();
    result
}

/// Extracts a `.tar.gz` archive into `dest`.
///
/// The gzip stream is decoded on the fly; no temporary uncompressed file is
/// written to disk.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or the archive is malformed.
fn unpack_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let gz = flate2::read::GzDecoder::new(file);
    tar::Archive::new(gz)
        .unpack(dest)
        .context("Failed to extract tar.gz")
}

/// Extracts a `.zip` archive into `dest`.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, the central directory
/// cannot be read, or any entry cannot be extracted.
fn unpack_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    zip::ZipArchive::new(file)
        .context("Failed to read zip archive")?
        .extract(dest)
        .context("Failed to extract zip")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn make_tar_gz(path: &Path, entry_name: &str, contents: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, entry_name, contents)
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();
    }

    fn make_zip(path: &Path, entry_name: &str, contents: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(entry_name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
        writer.finish().unwrap();
    }

    #[test]
    fn unpack_tar_gz_extracts_files() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("go.tar.gz");
        make_tar_gz(&archive, "go/bin/hello.txt", b"hello from tar");

        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        unpack(&archive, &dest).unwrap();

        let extracted = dest.join("go").join("bin").join("hello.txt");
        assert_eq!(
            std::fs::read_to_string(extracted).unwrap(),
            "hello from tar"
        );
    }

    #[test]
    fn unpack_zip_extracts_files() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("go.zip");
        make_zip(&archive, "go/bin/hello.txt", b"hello from zip");

        let dest = dir.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        unpack(&archive, &dest).unwrap();

        let extracted = dest.join("go").join("bin").join("hello.txt");
        assert_eq!(
            std::fs::read_to_string(extracted).unwrap(),
            "hello from zip"
        );
    }

    #[test]
    fn unpack_rejects_unsupported_extension() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("go.rar");
        std::fs::write(&archive, b"not a real archive").unwrap();

        let dest = dir.path().join("dest");
        let err = unpack(&archive, &dest).unwrap_err();
        assert!(err.to_string().contains("Unsupported archive format"));
    }

    #[test]
    fn unpack_tar_gz_errors_on_corrupt_archive() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("corrupt.tar.gz");
        std::fs::write(&archive, b"not actually gzip data").unwrap();

        let dest = dir.path().join("dest");
        assert!(unpack(&archive, &dest).is_err());
    }
}
