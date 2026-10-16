//! Data structures for the go.dev release JSON API.
//!
//! The structs in this module are deserialised directly from the JSON response
//! of `https://go.dev/dl/?mode=json&include=all`. Only the fields required by
//! `gvsn` are used; the rest are retained to avoid deserialisation errors if
//! the API adds new fields.

use crate::version::GoVersion;
use serde::{Deserialize, Serialize};

/// A single Go release as returned by the go.dev download API.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Release {
    /// Canonical release tag, e.g. `"go1.22.4"`.
    pub version: String,

    /// `true` for stable releases; `false` for release candidates and betas.
    pub stable: bool,

    /// All downloadable files associated with this release.
    pub files: Vec<ReleaseFile>,
}

/// A single downloadable file within a [`Release`].
#[derive(Deserialize, Serialize, Debug, Clone)]
#[allow(dead_code)]
pub struct ReleaseFile {
    /// Archive filename, e.g. `"go1.22.4.linux-amd64.tar.gz"`.
    pub filename: String,

    /// Target operating system reported by go.dev (`"linux"`, `"darwin"`,
    /// `"windows"`).
    pub os: String,

    /// Target architecture reported by go.dev (`"amd64"`, `"arm64"`, `"386"`).
    pub arch: String,

    /// Expected SHA-256 checksum of the file as a lowercase hex string.
    pub sha256: String,

    /// File size in bytes.
    pub size: u64,

    /// File kind. Only `"archive"` files are used by `gvsn`; others
    /// (`"source"`, `"installer"`) are ignored.
    pub kind: String,
}

impl Release {
    /// Parses [`Release::version`] into a [`GoVersion`].
    ///
    /// Returns `None` if the version string cannot be parsed (e.g. for
    /// pre-release tags with non-standard suffixes).
    pub fn go_version(&self) -> Option<GoVersion> {
        GoVersion::parse(&self.version).ok()
    }

    /// Finds the archive file for the given operating system and architecture.
    ///
    /// Returns the first [`ReleaseFile`] whose `os` and `arch` fields match
    /// the supplied values and whose `kind` is `"archive"`.
    ///
    /// Returns `None` if no matching file exists in this release.
    pub fn archive_for(&self, os: &str, arch: &str) -> Option<&ReleaseFile> {
        self.files
            .iter()
            .find(|f| f.os == os && f.arch == arch && f.kind == "archive")
    }

    /// Returns the source tarball file for this release, if available.
    ///
    /// Source files have `kind == "source"` and carry empty `os` and `arch`
    /// fields because they are platform-independent.
    pub fn source_file(&self) -> Option<&ReleaseFile> {
        self.files.iter().find(|f| f.kind == "source")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> Release {
        Release {
            version: "go1.22.4".to_string(),
            stable: true,
            files: vec![
                ReleaseFile {
                    filename: "go1.22.4.linux-amd64.tar.gz".to_string(),
                    os: "linux".to_string(),
                    arch: "amd64".to_string(),
                    sha256: "abc123".to_string(),
                    size: 100,
                    kind: "archive".to_string(),
                },
                ReleaseFile {
                    filename: "go1.22.4.windows-amd64.zip".to_string(),
                    os: "windows".to_string(),
                    arch: "amd64".to_string(),
                    sha256: "def456".to_string(),
                    size: 200,
                    kind: "archive".to_string(),
                },
                ReleaseFile {
                    filename: "go1.22.4.src.tar.gz".to_string(),
                    os: String::new(),
                    arch: String::new(),
                    sha256: "ghi789".to_string(),
                    size: 300,
                    kind: "source".to_string(),
                },
                ReleaseFile {
                    filename: "go1.22.4.linux-amd64.msi".to_string(),
                    os: "linux".to_string(),
                    arch: "amd64".to_string(),
                    sha256: "jkl012".to_string(),
                    size: 400,
                    kind: "installer".to_string(),
                },
            ],
        }
    }

    #[test]
    fn go_version_parses_the_version_field() {
        let r = release();
        let v = r.go_version().unwrap();
        assert_eq!(v.tag(), "go1.22.4");
    }

    #[test]
    fn go_version_returns_none_for_unparsable_version() {
        let mut r = release();
        r.version = "go1.22.4rc1-weird".to_string();
        assert!(r.go_version().is_none());
    }

    #[test]
    fn archive_for_finds_matching_os_and_arch() {
        let r = release();
        let file = r.archive_for("linux", "amd64").unwrap();
        assert_eq!(file.filename, "go1.22.4.linux-amd64.tar.gz");
    }

    #[test]
    fn archive_for_ignores_non_archive_kind() {
        let r = release();
        // The linux/amd64 installer (.msi) must not be returned as an archive.
        let file = r.archive_for("linux", "amd64").unwrap();
        assert_eq!(file.kind, "archive");
    }

    #[test]
    fn archive_for_returns_none_when_no_match() {
        let r = release();
        assert!(r.archive_for("plan9", "amd64").is_none());
    }

    #[test]
    fn source_file_finds_the_source_kind() {
        let r = release();
        let file = r.source_file().unwrap();
        assert_eq!(file.filename, "go1.22.4.src.tar.gz");
    }

    #[test]
    fn source_file_returns_none_when_absent() {
        let mut r = release();
        r.files.retain(|f| f.kind != "source");
        assert!(r.source_file().is_none());
    }
}
