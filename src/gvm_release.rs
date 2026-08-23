//! Shared lookup of gvm's own latest GitHub release.
//!
//! Used by both `gvm upgrade` (which needs to know whether - and what - to
//! download) and the background update notifier (which only needs the
//! version string to compare against). Kept independent of [`crate::http::HttpClient`]
//! since neither caller needs its verbose-logging/retry machinery here -
//! this is a single, short-lived, best-effort request.

use std::time::Duration;

use anyhow::{Context, Result};

/// GitHub repository slug used to build the Releases API URL.
pub const REPO: &str = "jhonsferg/gvm-rs";

/// Returns the GitHub API base URL, overridable via `GVM_TEST_API_BASE` for
/// local testing without a real GitHub release.
pub fn api_base() -> String {
    std::env::var("GVM_TEST_API_BASE").unwrap_or_else(|_| "https://api.github.com".to_owned())
}

/// Minimal shape of the GitHub Releases API response.
///
/// Only `tag_name` is needed; the rest of the payload is ignored.
#[derive(serde::Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
}

/// Fetches the tag name of the latest `jhonsferg/gvm-rs` release, with the
/// leading `v` stripped.
///
/// `timeout` bounds the entire request (connect + response); callers doing a
/// best-effort background check should keep this short.
///
/// # Errors
///
/// Returns an error if the API cannot be reached within `timeout`, returns a
/// non-2xx status, or the response body cannot be parsed.
pub fn fetch_latest_version(timeout: Duration) -> Result<String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(format!("gvm/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent();

    let url = format!("{}/repos/{REPO}/releases/latest", api_base());
    let mut response = agent
        .get(&url)
        .call()
        .with_context(|| format!("Failed to reach {url}"))?;

    let release: GithubRelease = response
        .body_mut()
        .read_json()
        .context("Failed to parse GitHub release response")?;

    Ok(release.tag_name.trim_start_matches('v').to_string())
}

/// Parses a `"MAJOR.MINOR.PATCH"` string into a comparable tuple.
///
/// Returns `None` when the string does not have exactly three dot-separated
/// components or any of them is not a valid unsigned integer. Components are
/// validated strictly (not skipped) so a stray pre-release suffix like
/// `"1.2.3-beta.1"` is rejected instead of silently mis-parsed as `(1, 2, 1)`.
pub fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = s.split('.').collect();
    match parts.as_slice() {
        [major, minor, patch] => Some((
            major.parse().ok()?,
            minor.parse().ok()?,
            patch.parse().ok()?,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parses_correctly() {
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("10.0.0"), Some((10, 0, 0)));
        assert_eq!(parse_semver("bad"), None);
        assert_eq!(parse_semver("1.2"), None);
    }

    #[test]
    fn semver_rejects_invalid_middle_segment_instead_of_skipping_it() {
        // A stray pre-release-style segment must invalidate the whole parse,
        // not be silently dropped and shift the remaining numbers into the
        // wrong positions.
        assert_eq!(parse_semver("1.2.3-beta.1"), None);
        assert_eq!(parse_semver("1.x.2.3"), None);
    }

    #[test]
    fn semver_ordering() {
        assert!(parse_semver("0.2.0") > parse_semver("0.1.9"));
        assert!(parse_semver("1.0.0") > parse_semver("0.99.99"));
        assert_eq!(parse_semver("1.0.0"), parse_semver("1.0.0"));
    }
}
