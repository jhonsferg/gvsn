//! Archive extraction.
//!
//! [`extract`] unpacks `.tar.gz` (Linux / macOS) and `.zip` (Windows)
//! archives with a spinner to indicate progress. Downloading the archive is
//! the job of `gvsn-net`, and verifying it the job of [`crate::checksum`].

pub mod extract;
