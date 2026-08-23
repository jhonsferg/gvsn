//! `gvm list-remote` - list Go versions available from go.dev.
//!
//! By default shows only the latest patch release per minor version (compact
//! view). Passing `--all` shows every patch release in the index.
//! Already-installed versions are marked so the user can see what is new
//! at a glance.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use crate::{
    config::Config,
    http::HttpClient,
    remote::{index, release::Release},
    toolchain,
};

/// One available release as reported by `--json`.
#[derive(Serialize)]
struct RemoteVersion {
    version: String,
    installed: bool,
}

/// Fetches the go.dev release index and prints available stable versions.
///
/// When `all` is `false` (the default), only the highest patch release for
/// each minor version is shown, keeping the list concise. When `all` is
/// `true`, every stable patch release is listed.
///
/// Versions already installed locally are prefixed with `✓` to distinguish
/// them from versions available for download.
///
/// When `json` is `true`, prints a JSON array of `{"version", "installed"}`
/// objects instead (still respecting `all`), with the "Fetching..." progress
/// line sent to stderr so stdout stays valid JSON.
///
/// # Errors
///
/// Returns an error if the remote release index cannot be fetched, or
/// (`json` only) if the result cannot be serialised.
pub fn run(config: &Config, client: &HttpClient, all: bool, json: bool) -> Result<()> {
    if json {
        eprintln!("{} Fetching available Go versions...", "->".cyan());
    } else {
        println!("{} Fetching available Go versions...", "->".cyan());
    }

    let releases = index::fetch_releases(client, &config.root)?;
    let installed = toolchain::list_installed(config)?;

    let to_show = releases_to_show(&releases, all);

    if json {
        let entries: Vec<RemoteVersion> = to_show
            .iter()
            .map(|r| RemoteVersion {
                version: r.version.clone(),
                installed: installed.iter().any(|v| v.tag() == r.version),
            })
            .collect();
        println!("{}", serde_json::to_string(&entries)?);
        return Ok(());
    }

    println!(
        "Available Go versions{}:",
        if all {
            " (all patches)"
        } else {
            " (latest per minor)"
        }
    );

    for r in &to_show {
        let is_installed = installed.iter().any(|v| v.tag() == r.version);
        let mark = if is_installed {
            format!("{}", "✓".green())
        } else {
            " ".to_string()
        };
        println!("  {mark} {}", r.version);
    }

    if !all {
        println!(
            "\n  Use {} to see all patch releases.",
            "gvm list-remote --all".cyan()
        );
    }
    Ok(())
}

/// Filters the release list down to what should be printed.
///
/// When `all` is `true`, every stable release is kept. Otherwise the list is
/// deduplicated to only the newest (first, since go.dev returns newest-first)
/// release per `major.minor` pair.
fn releases_to_show(releases: &[Release], all: bool) -> Vec<&Release> {
    let stable: Vec<_> = releases.iter().filter(|r| r.stable).collect();

    if all {
        return stable;
    }

    let mut seen = std::collections::HashSet::new();
    stable
        .into_iter()
        .filter(|r| {
            r.go_version()
                .is_some_and(|v| seen.insert((v.major, v.minor)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::release::ReleaseFile;

    fn release(version: &str, stable: bool) -> Release {
        Release {
            version: version.to_string(),
            stable,
            files: vec![ReleaseFile {
                filename: format!("{version}.linux-amd64.tar.gz"),
                os: "linux".to_string(),
                arch: "amd64".to_string(),
                sha256: String::new(),
                size: 0,
                kind: "archive".to_string(),
            }],
        }
    }

    #[test]
    fn releases_to_show_all_keeps_every_stable_release() {
        let releases = vec![
            release("go1.22.4", true),
            release("go1.22.3", true),
            release("go1.21.0", true),
            release("go1.23.0rc1", false),
        ];
        let shown = releases_to_show(&releases, true);
        assert_eq!(shown.len(), 3);
    }

    #[test]
    fn releases_to_show_compact_keeps_newest_patch_per_minor() {
        let releases = vec![
            release("go1.22.4", true),
            release("go1.22.3", true),
            release("go1.21.5", true),
            release("go1.21.4", true),
        ];
        let shown = releases_to_show(&releases, false);
        let versions: Vec<&str> = shown.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, vec!["go1.22.4", "go1.21.5"]);
    }

    #[test]
    fn releases_to_show_excludes_unstable_releases() {
        let releases = vec![release("go1.23.0rc1", false), release("go1.22.4", true)];
        let shown = releases_to_show(&releases, false);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].version, "go1.22.4");
    }

    #[test]
    fn releases_to_show_empty_input_yields_empty_output() {
        assert!(releases_to_show(&[], false).is_empty());
        assert!(releases_to_show(&[], true).is_empty());
    }
}
