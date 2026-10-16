//! Integrity verification of downloaded archives.
//!
//! [`verify_sha256`] checks a completed file against the expected hex digest
//! published by go.dev. Downloading itself lives in `gvsn-net`, which does
//! not verify integrity.

use std::io::Read;
use std::path::Path;

use anyhow::{bail, Context, Result};
use colored::Colorize;
use sha2::{Digest, Sha256};

/// Verifies the SHA-256 digest of `file` against the `expected` hex string.
///
/// If `expected` is empty the check is skipped (some older go.dev entries do
/// not include a checksum). When the digests differ an informative error is
/// returned that includes both the expected and actual values.
///
/// # Errors
///
/// Returns an error if `file` cannot be read or the digest does not match.
pub fn verify_sha256(file: &Path, expected: &str) -> Result<()> {
    if expected.is_empty() {
        eprintln!(
            "  {} No published checksum for {} - integrity not verified.",
            "!".yellow(),
            file.display()
        );
        return Ok(());
    }

    let mut hasher = Sha256::new();
    let mut f = std::fs::File::open(file)
        .with_context(|| format!("Cannot open {} for checksum", file.display()))?;
    let mut buf = [0u8; 65_536];
    loop {
        let n = f
            .read(&mut buf)
            .context("Failed to read file for checksum")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    if actual != expected {
        bail!(
            "Checksum mismatch for {}!\n  expected: {expected}\n  got:      {actual}",
            file.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn verify_sha256_accepts_matching_digest() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("data.bin");
        std::fs::write(&file, b"hello world").unwrap();

        // sha256("hello world")
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        verify_sha256(&file, expected).unwrap();
    }

    #[test]
    fn verify_sha256_rejects_mismatched_digest() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("data.bin");
        std::fs::write(&file, b"hello world").unwrap();

        let err = verify_sha256(
            &file,
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap_err();
        assert!(err.to_string().contains("Checksum mismatch"));
    }

    #[test]
    fn verify_sha256_skips_check_when_expected_is_empty() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("data.bin");
        std::fs::write(&file, b"anything").unwrap();

        verify_sha256(&file, "").unwrap();
    }

    #[test]
    fn verify_sha256_errors_when_file_missing() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("missing.bin");
        let err = verify_sha256(&file, "deadbeef").unwrap_err();
        assert!(err.to_string().contains("Cannot open"));
    }
}
