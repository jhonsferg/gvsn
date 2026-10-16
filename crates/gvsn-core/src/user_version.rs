//! User-facing version specification.
//!
//! [`VersionSpec`] represents the version input as typed by the user on the
//! command line. It is deliberately decoupled from [`crate::version::GoVersion`]
//! so that resolution - querying the remote index or the local store - stays in
//! the caller rather than leaking into this type.
//!
//! The three variants cover every form accepted by `gvsn`:
//!
//! | User input | Variant                                   |
//! |------------|-------------------------------------------|
//! | `latest`   | `Latest`                                  |
//! | `1.22`     | `Partial { major: 1, minor: 22 }`         |
//! | `1.22.4`   | `Exact { major: 1, minor: 22, patch: 4 }` |

use anyhow::{bail, Result};
use std::fmt;

use crate::version::GoVersion;

/// A version specification as typed by the user.
///
/// See the [module documentation](self) for the mapping from input strings to
/// variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSpec {
    /// The latest stable release available.
    Latest,

    /// The latest patch release within a specific minor line (e.g. `1.22`).
    Partial { major: u32, minor: u32 },

    /// A fully-qualified release (e.g. `1.22.4`).
    Exact { major: u32, minor: u32, patch: u32 },
}

impl VersionSpec {
    /// Parses a user-supplied string into a [`VersionSpec`].
    ///
    /// The `go` prefix is accepted and stripped before parsing, so both
    /// `1.22.4` and `go1.22.4` produce the same result.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is not `latest`, `X.Y`, or `X.Y.Z`, or
    /// if any numeric component cannot be parsed as a `u32`.
    pub fn parse(input: &str) -> Result<Self> {
        let s = input.trim();

        if s.eq_ignore_ascii_case("latest") {
            return Ok(Self::Latest);
        }

        let stripped = s.strip_prefix("go").unwrap_or(s);
        let parts: Vec<&str> = stripped.split('.').collect();

        match parts.as_slice() {
            [maj, min] => Ok(Self::Partial {
                major: maj.parse()?,
                minor: min.parse()?,
            }),
            [maj, min, pat] => Ok(Self::Exact {
                major: maj.parse()?,
                minor: min.parse()?,
                patch: pat.parse()?,
            }),
            _ => bail!(
                "Invalid version spec '{}'. Use 'latest', X.Y, or X.Y.Z.",
                input
            ),
        }
    }

    /// Returns `true` if the given [`GoVersion`] satisfies this spec.
    ///
    /// - `Latest` matches any version.
    /// - `Partial` matches when `major` and `minor` are equal, regardless of
    ///   `patch`.
    /// - `Exact` requires all three components to be equal.
    pub fn matches(&self, v: &GoVersion) -> bool {
        match self {
            Self::Latest => true,
            Self::Partial { major, minor } => v.major == *major && v.minor == *minor,
            Self::Exact {
                major,
                minor,
                patch,
            } => v.major == *major && v.minor == *minor && v.patch == *patch,
        }
    }
}

/// Displays the spec in the same form the user would type it.
impl fmt::Display for VersionSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Latest => write!(f, "latest"),
            Self::Partial { major, minor } => write!(f, "{major}.{minor}"),
            Self::Exact {
                major,
                minor,
                patch,
            } => write!(f, "{major}.{minor}.{patch}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_latest_case_insensitive() {
        assert_eq!(VersionSpec::parse("latest").unwrap(), VersionSpec::Latest);
        assert_eq!(VersionSpec::parse("LATEST").unwrap(), VersionSpec::Latest);
        assert_eq!(VersionSpec::parse(" Latest ").unwrap(), VersionSpec::Latest);
    }

    #[test]
    fn parse_partial_with_and_without_go_prefix() {
        let expected = VersionSpec::Partial {
            major: 1,
            minor: 22,
        };
        assert_eq!(VersionSpec::parse("1.22").unwrap(), expected);
        assert_eq!(VersionSpec::parse("go1.22").unwrap(), expected);
    }

    #[test]
    fn parse_exact_with_and_without_go_prefix() {
        let expected = VersionSpec::Exact {
            major: 1,
            minor: 22,
            patch: 4,
        };
        assert_eq!(VersionSpec::parse("1.22.4").unwrap(), expected);
        assert_eq!(VersionSpec::parse("go1.22.4").unwrap(), expected);
    }

    #[test]
    fn parse_rejects_malformed_input() {
        assert!(VersionSpec::parse("not-a-version").is_err());
        assert!(VersionSpec::parse("1").is_err());
        assert!(VersionSpec::parse("1.2.3.4").is_err());
        assert!(VersionSpec::parse("1.x").is_err());
    }

    #[test]
    fn matches_latest_matches_anything() {
        let v = GoVersion::parse("1.22.4").unwrap();
        assert!(VersionSpec::Latest.matches(&v));
    }

    #[test]
    fn matches_partial_ignores_patch() {
        let spec = VersionSpec::Partial {
            major: 1,
            minor: 22,
        };
        assert!(spec.matches(&GoVersion::parse("1.22.4").unwrap()));
        assert!(spec.matches(&GoVersion::parse("1.22.0").unwrap()));
        assert!(!spec.matches(&GoVersion::parse("1.21.4").unwrap()));
        assert!(!spec.matches(&GoVersion::parse("2.22.4").unwrap()));
    }

    #[test]
    fn matches_exact_requires_full_equality() {
        let spec = VersionSpec::Exact {
            major: 1,
            minor: 22,
            patch: 4,
        };
        assert!(spec.matches(&GoVersion::parse("1.22.4").unwrap()));
        assert!(!spec.matches(&GoVersion::parse("1.22.5").unwrap()));
    }

    #[test]
    fn display_formats_match_user_input_style() {
        assert_eq!(VersionSpec::Latest.to_string(), "latest");
        assert_eq!(
            VersionSpec::Partial {
                major: 1,
                minor: 22
            }
            .to_string(),
            "1.22"
        );
        assert_eq!(
            VersionSpec::Exact {
                major: 1,
                minor: 22,
                patch: 4
            }
            .to_string(),
            "1.22.4"
        );
    }
}
