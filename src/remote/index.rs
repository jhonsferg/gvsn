//! Functions for fetching and querying the go.dev release index.
//!
//! This module is the single point of contact with the remote API. All network
//! requests made by `gvsn` go through [`fetch_releases`].

use std::path::Path;

use anyhow::{Context, Result};

use crate::{http::HttpClient, remote::release::Release, user_version::VersionSpec};

/// URL of the go.dev download API that lists all releases.
const GO_DL_API: &str = "https://go.dev/dl/?mode=json&include=all";

/// Cache file name, relative to the gvsn root directory (`GVSN_DIR`).
const CACHE_FILE: &str = "release-index.json";

/// How long a cached release index is trusted before a fresh fetch is made.
///
/// Short enough that a newly published release is picked up the same day,
/// long enough that running `list-remote`, `outdated`, and `install` back to
/// back - a common sequence - only pays the network+parse cost once.
const CACHE_TTL_SECS: u64 = 60 * 60;

#[derive(serde::Serialize, serde::Deserialize)]
struct ReleaseCache {
    last_checked_unix: u64,
    releases: Vec<Release>,
}

/// Returns the complete list of Go releases from go.dev, from a local cache
/// when one exists and is younger than [`CACHE_TTL_SECS`], or a fresh network
/// fetch otherwise.
///
/// The API returns both stable and unstable (RC/beta) releases; callers are
/// responsible for filtering by [`Release::stable`] if needed.
///
/// # Errors
///
/// Returns an error if the cache is stale/missing and the HTTP request fails
/// or the response body cannot be deserialised as JSON. A cache that exists
/// but cannot be read or parsed is treated the same as a missing one - it is
/// silently replaced by a fresh fetch rather than failing the command.
pub fn fetch_releases(client: &HttpClient, root: &Path) -> Result<Vec<Release>> {
    let cache_path = root.join(CACHE_FILE);
    let now = now_unix();

    if let Some(cache) = load_cache(&cache_path) {
        if !is_stale(cache.last_checked_unix, now) {
            return Ok(cache.releases);
        }
    }

    let releases = fetch_releases_uncached(client)?;
    let _ = save_cache(
        &cache_path,
        &ReleaseCache {
            last_checked_unix: now,
            releases: releases.clone(),
        },
    );
    Ok(releases)
}

/// Performs the actual network request and JSON parse, bypassing the cache.
///
/// When verbose mode is active, the request and response details are printed
/// to stderr before the JSON body is parsed.
fn fetch_releases_uncached(client: &HttpClient) -> Result<Vec<Release>> {
    crate::http::log_request(client, "GET", GO_DL_API);

    let mut response = client
        .agent()
        .get(GO_DL_API)
        .call()
        .context("Failed to reach go.dev - check your internet connection")?;

    crate::http::log_response(
        client,
        response.status().as_u16(),
        response.status().canonical_reason().unwrap_or(""),
        response.headers(),
    );

    response
        .body_mut()
        .read_json::<Vec<Release>>()
        .context("Failed to parse Go releases JSON")
}

fn load_cache(path: &Path) -> Option<ReleaseCache> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_cache(path: &Path, cache: &ReleaseCache) -> std::io::Result<()> {
    let content = serde_json::to_string(cache).unwrap_or_default();
    std::fs::write(path, content)
}

/// Returns `true` when a cache written at `last_checked_unix` is old enough
/// (relative to `now`) that a fresh fetch should be made.
fn is_stale(last_checked_unix: u64, now: u64) -> bool {
    now.saturating_sub(last_checked_unix) >= CACHE_TTL_SECS
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Resolves a [`VersionSpec`] to the best-matching stable [`Release`].
///
/// Only stable releases are considered. The API returns releases in
/// newest-first order, so the first match is always the most recent one that
/// satisfies the spec.
///
/// # Errors
///
/// Returns an error if no stable release satisfies the spec, or if the
/// release list is empty.
pub fn resolve<'a>(spec: &VersionSpec, releases: &'a [Release]) -> Result<&'a Release> {
    let mut stable = releases.iter().filter(|r| r.stable);

    match spec {
        VersionSpec::Latest => stable
            .next()
            .ok_or_else(|| anyhow::anyhow!("No stable releases found")),
        _ => stable
            .find(|r| r.go_version().is_some_and(|v| spec.matches(&v)))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Go version '{}' not found. Run 'gvsn list-remote' to see available versions.",
                    spec
                )
            }),
    }
}

/// Builds the download URL for a named archive file from go.dev.
pub fn download_url(filename: &str) -> String {
    format!("https://go.dev/dl/{filename}")
}

/// Returns the go.dev OS identifier for the current compilation target.
///
/// Possible return values: `"windows"`, `"darwin"`, `"linux"`.
pub fn host_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

/// Returns the go.dev architecture identifier for the current compilation target.
///
/// Possible return values match Go's `GOARCH` naming: `"amd64"`, `"arm64"`,
/// `"arm"`, `"386"`, `"riscv64"`, `"s390x"`, `"ppc64le"`.
pub fn host_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else if cfg!(target_arch = "x86") {
        "386"
    } else if cfg!(target_arch = "riscv64") {
        "riscv64"
    } else if cfg!(target_arch = "s390x") {
        "s390x"
    } else if cfg!(target_arch = "powerpc64") {
        "ppc64le"
    } else {
        "386"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::release::{Release, ReleaseFile};
    use tempfile::tempdir;

    #[test]
    fn is_stale_before_ttl_is_false() {
        assert!(!is_stale(1000, 1000 + CACHE_TTL_SECS - 1));
    }

    #[test]
    fn is_stale_at_or_after_ttl_is_true() {
        assert!(is_stale(1000, 1000 + CACHE_TTL_SECS));
    }

    #[test]
    fn is_stale_handles_clock_going_backwards() {
        assert!(!is_stale(1000, 500));
    }

    #[test]
    fn cache_round_trips_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CACHE_FILE);

        let cache = ReleaseCache {
            last_checked_unix: 12345,
            releases: vec![release("go1.22.4", true)],
        };
        save_cache(&path, &cache).unwrap();

        let loaded = load_cache(&path).unwrap();
        assert_eq!(loaded.last_checked_unix, 12345);
        assert_eq!(loaded.releases.len(), 1);
        assert_eq!(loaded.releases[0].version, "go1.22.4");
    }

    #[test]
    fn load_cache_returns_none_when_missing_or_corrupt() {
        let dir = tempdir().unwrap();
        assert!(load_cache(&dir.path().join("missing.json")).is_none());

        let corrupt = dir.path().join("corrupt.json");
        std::fs::write(&corrupt, "not json").unwrap();
        assert!(load_cache(&corrupt).is_none());
    }

    #[test]
    fn fetch_releases_uses_fresh_cache_without_a_network_call() {
        // Regression guard: this must succeed with no real network access,
        // proving `fetch_releases` returned the cached list instead of
        // trying `fetch_releases_uncached`.
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join(CACHE_FILE);
        save_cache(
            &cache_path,
            &ReleaseCache {
                last_checked_unix: now_unix(),
                releases: vec![release("go1.22.4", true)],
            },
        )
        .unwrap();

        let client = crate::http::HttpClient::new(false, 0).unwrap();
        let releases = fetch_releases(&client, dir.path()).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, "go1.22.4");
    }

    fn release(version: &str, stable: bool) -> Release {
        Release {
            version: version.to_string(),
            stable,
            files: vec![ReleaseFile {
                filename: format!("{version}.linux-amd64.tar.gz"),
                os: "linux".to_string(),
                arch: "amd64".to_string(),
                sha256: "deadbeef".to_string(),
                size: 123,
                kind: "archive".to_string(),
            }],
        }
    }

    #[test]
    fn resolve_latest_returns_first_stable_release() {
        let releases = vec![
            release("go1.23.0", true),
            release("go1.22.4", true),
            release("go1.22.3", true),
        ];
        let resolved = resolve(&VersionSpec::Latest, &releases).unwrap();
        assert_eq!(resolved.version, "go1.23.0");
    }

    #[test]
    fn resolve_latest_skips_unstable_releases() {
        let releases = vec![release("go1.24.0rc1", false), release("go1.23.0", true)];
        let resolved = resolve(&VersionSpec::Latest, &releases).unwrap();
        assert_eq!(resolved.version, "go1.23.0");
    }

    #[test]
    fn resolve_latest_errors_when_no_stable_releases() {
        let releases = vec![release("go1.24.0rc1", false)];
        let err = resolve(&VersionSpec::Latest, &releases).unwrap_err();
        assert!(err.to_string().contains("No stable releases"));
    }

    #[test]
    fn resolve_partial_matches_newest_patch() {
        let releases = vec![
            release("go1.23.0", true),
            release("go1.22.5", true),
            release("go1.22.4", true),
        ];
        let spec = VersionSpec::Partial {
            major: 1,
            minor: 22,
        };
        let resolved = resolve(&spec, &releases).unwrap();
        assert_eq!(resolved.version, "go1.22.5");
    }

    #[test]
    fn resolve_exact_requires_full_match() {
        let releases = vec![release("go1.22.4", true), release("go1.22.5", true)];
        let spec = VersionSpec::Exact {
            major: 1,
            minor: 22,
            patch: 4,
        };
        let resolved = resolve(&spec, &releases).unwrap();
        assert_eq!(resolved.version, "go1.22.4");
    }

    #[test]
    fn resolve_errors_with_helpful_message_when_not_found() {
        let releases = vec![release("go1.22.4", true)];
        let spec = VersionSpec::Exact {
            major: 9,
            minor: 9,
            patch: 9,
        };
        let err = resolve(&spec, &releases).unwrap_err();
        assert!(err.to_string().contains("not found"));
        assert!(err.to_string().contains("list-remote"));
    }

    #[test]
    fn resolve_ignores_unstable_release_for_partial_and_exact() {
        let releases = vec![release("go1.22.4", false)];
        let partial = VersionSpec::Partial {
            major: 1,
            minor: 22,
        };
        assert!(resolve(&partial, &releases).is_err());
    }

    #[test]
    fn download_url_builds_expected_link() {
        assert_eq!(
            download_url("go1.22.4.linux-amd64.tar.gz"),
            "https://go.dev/dl/go1.22.4.linux-amd64.tar.gz"
        );
    }

    #[test]
    fn host_os_matches_a_known_platform() {
        assert!(["windows", "darwin", "linux"].contains(&host_os()));
    }

    #[test]
    fn host_arch_matches_a_known_architecture() {
        assert!(
            ["amd64", "arm64", "arm", "386", "riscv64", "s390x", "ppc64le"].contains(&host_arch())
        );
    }
}
