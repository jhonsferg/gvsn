//! Canonical Go version representation.
//!
//! [`GoVersion`] holds a parsed `major.minor.patch` triple and provides
//! comparison, display, and the canonical release tag (`go1.22.4`).
//!
//! This type represents a *known, resolved* version. User-supplied input
//! that may be partial (`1.22`) or symbolic (`latest`) is handled by
//! [`crate::user_version::VersionSpec`] instead.

use anyhow::{bail, Result};
use std::fmt;

/// A fully resolved Go version with `major`, `minor`, and `patch` components.
///
/// Versions are ordered semantically: `go1.22.4 > go1.22.3 > go1.21.0`.
/// When `patch` is 0, the version represents the initial release of a minor
/// (e.g. `go1.22`), which is stored and displayed without the trailing `.0`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GoVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// Filename gvsn uses for its own per-project version pin, written by
/// `gvsn local` and checked first when resolving the active version.
pub const GO_VERSION_FILE: &str = ".go-version";

/// Filename asdf (and asdf-compatible tools) use for per-project version
/// pins. Checked as a fallback when [`GO_VERSION_FILE`] is absent, so a
/// project that already pins its Go version via asdf's `golang` plugin does
/// not need a separate gvsn-specific file.
pub const TOOL_VERSIONS_FILE: &str = ".tool-versions";

/// Extracts the version from the `golang` line of a `.tool-versions` file's
/// contents (asdf format: `<tool> <version> [<fallback-version> ...]`, one
/// tool per line, blank lines and `#` comments ignored).
///
/// Only the first version listed is returned - gvsn has no concept of a
/// fallback version list. Returns `None` when no `golang` line is present.
pub fn parse_tool_versions_golang_line(content: &str) -> Option<&str> {
    content.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let mut parts = line.split_whitespace();
        if parts.next()? != "golang" {
            return None;
        }
        parts.next()
    })
}

impl GoVersion {
    /// Parses a version string into a [`GoVersion`].
    ///
    /// Accepts both the bare numeric form (`1.22.4`, `1.22`) and the canonical
    /// Go tag form (`go1.22.4`, `go1.22`). The `go` prefix is stripped before
    /// parsing.
    ///
    /// # Errors
    ///
    /// Returns an error if the input does not match `X.Y` or `X.Y.Z` (after
    /// stripping the optional `go` prefix), or if any component is not a valid
    /// unsigned integer.
    pub fn parse(input: &str) -> Result<Self> {
        let s = input.trim().strip_prefix("go").unwrap_or(input.trim());
        let parts: Vec<&str> = s.split('.').collect();
        match parts.as_slice() {
            [major, minor] => Ok(Self {
                major: major.parse()?,
                minor: minor.parse()?,
                patch: 0,
            }),
            [major, minor, patch] => Ok(Self {
                major: major.parse()?,
                minor: minor.parse()?,
                patch: patch.parse()?,
            }),
            _ => bail!(
                "Invalid version '{}'. Use X.Y or X.Y.Z (e.g. 1.22 or 1.22.4)",
                input
            ),
        }
    }

    /// Returns the canonical Go release tag used by go.dev and the local store.
    ///
    /// When `patch` is 0 the tag omits the patch component (`go1.22`);
    /// otherwise all three components are included (`go1.22.4`).
    pub fn tag(&self) -> String {
        if self.patch == 0 {
            format!("go{}.{}", self.major, self.minor)
        } else {
            format!("go{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

/// Displays the version without the `go` prefix (`1.22.4` or `1.22`).
impl fmt::Display for GoVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.patch == 0 {
            write!(f, "{}.{}", self.major, self.minor)
        } else {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

impl PartialOrd for GoVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Compares versions by `major`, then `minor`, then `patch` (all ascending).
impl Ord for GoVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tool_versions_finds_the_golang_line() {
        let content = "nodejs 20.11.0\ngolang 1.22.4\npython 3.12.1\n";
        assert_eq!(parse_tool_versions_golang_line(content), Some("1.22.4"));
    }

    #[test]
    fn parse_tool_versions_takes_only_the_first_fallback_version() {
        let content = "golang 1.22.4 1.21.9\n";
        assert_eq!(parse_tool_versions_golang_line(content), Some("1.22.4"));
    }

    #[test]
    fn parse_tool_versions_ignores_blank_lines_and_comments() {
        let content = "\n# managed by asdf\n\ngolang 1.22.4\n";
        assert_eq!(parse_tool_versions_golang_line(content), Some("1.22.4"));
    }

    #[test]
    fn parse_tool_versions_ignores_tools_with_golang_as_a_prefix() {
        // "golangci-lint" must not be mistaken for the "golang" plugin line.
        let content = "golangci-lint 1.55.2\n";
        assert_eq!(parse_tool_versions_golang_line(content), None);
    }

    #[test]
    fn parse_tool_versions_returns_none_when_absent() {
        let content = "nodejs 20.11.0\npython 3.12.1\n";
        assert_eq!(parse_tool_versions_golang_line(content), None);
    }

    #[test]
    fn parse_tool_versions_returns_none_for_empty_content() {
        assert_eq!(parse_tool_versions_golang_line(""), None);
    }
}
