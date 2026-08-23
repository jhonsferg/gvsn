//! `gvm outdated` - check which installed Go versions have newer patch releases.
//!
//! Compares every locally installed version against the latest available patch
//! for that same major.minor line on go.dev and reports the status so the user
//! can decide which versions to update or remove.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use crate::{config::Config, http::HttpClient, remote::index, toolchain};

/// One installed version's update status as reported by `--json`.
#[derive(Serialize)]
struct OutdatedEntry {
    installed: String,
    /// `None` when no release for this major.minor line was found on go.dev.
    latest: Option<String>,
    status: OutdatedStatus,
    /// Only meaningful when `status` is `Behind`.
    patches_behind: Option<u32>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OutdatedStatus {
    UpToDate,
    Behind,
    Unknown,
}

/// Checks installed Go versions against go.dev and prints their update status.
///
/// For each installed version the function finds the newest available patch
/// release with the same major and minor version. If a newer patch exists the
/// version is reported as behind; otherwise it is reported as up to date.
///
/// When `json` is `true`, prints a JSON array of per-version status objects
/// instead, with all progress text sent to stderr so stdout stays valid JSON.
///
/// # Errors
///
/// Returns an error if the versions directory cannot be read or if the go.dev
/// release index cannot be fetched.
pub fn run(config: &Config, client: &HttpClient, json: bool) -> Result<()> {
    let installed = toolchain::list_installed(config)?;

    if installed.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No Go versions installed. Run 'gvm install latest'.");
        }
        return Ok(());
    }

    if json {
        eprintln!("{} Fetching available Go versions...", "->".cyan());
    } else {
        println!("{} Fetching available Go versions...", "->".cyan());
    }
    let releases = index::fetch_releases(client, &config.root)?;

    // Only consider stable releases; collect into owned Vec so we can compare.
    let stable: Vec<_> = releases
        .iter()
        .filter(|r| r.stable)
        .filter_map(|r| r.go_version())
        .collect();

    if json {
        let entries: Vec<OutdatedEntry> = installed
            .iter()
            .map(|v| build_outdated_entry(v, &stable))
            .collect();
        println!("{}", serde_json::to_string(&entries)?);
        return Ok(());
    }

    println!();
    println!(
        "  {:<14} {:<14} {}",
        "Installed".bold(),
        "Latest patch".bold(),
        "Status".bold()
    );
    println!("  {}", "─".repeat(50));

    let mut any_outdated = false;

    for v in &installed {
        // Highest patch release for the same major.minor on go.dev.
        let latest = stable
            .iter()
            .filter(|rv| rv.major == v.major && rv.minor == v.minor)
            .max()
            .cloned();

        match latest {
            None => {
                // Probably a very old minor no longer listed on go.dev.
                println!("  {:<14} {:<14} {}", v.tag(), "-", "?  no data".dimmed());
            }
            Some(ref latest) if v >= latest => {
                println!(
                    "  {:<14} {:<14} {}",
                    v.tag(),
                    latest.tag(),
                    format!("{}  up to date", "✓").green()
                );
            }
            Some(ref latest) => {
                let diff = latest.patch.saturating_sub(v.patch);
                println!(
                    "  {:<14} {:<14} {}",
                    v.tag(),
                    latest.tag(),
                    format!(
                        "⚠  {} patch{} behind",
                        diff,
                        if diff == 1 { "" } else { "es" }
                    )
                    .yellow()
                );
                any_outdated = true;
            }
        }
    }

    println!();
    if any_outdated {
        println!(
            "  Run {} to update a version, or {} to remove it.",
            "gvm install <version>".cyan(),
            "gvm uninstall <version>".cyan()
        );
    } else {
        println!("  {} All installed versions are up to date.", "✓".green());
    }

    Ok(())
}

/// Builds the `--json` status entry for one installed version, mirroring the
/// text-output logic above (highest patch release for the same major.minor).
fn build_outdated_entry(
    v: &crate::version::GoVersion,
    stable: &[crate::version::GoVersion],
) -> OutdatedEntry {
    let latest = stable
        .iter()
        .filter(|rv| rv.major == v.major && rv.minor == v.minor)
        .max();

    match latest {
        None => OutdatedEntry {
            installed: v.tag(),
            latest: None,
            status: OutdatedStatus::Unknown,
            patches_behind: None,
        },
        Some(latest) if v >= latest => OutdatedEntry {
            installed: v.tag(),
            latest: Some(latest.tag()),
            status: OutdatedStatus::UpToDate,
            patches_behind: None,
        },
        Some(latest) => OutdatedEntry {
            installed: v.tag(),
            latest: Some(latest.tag()),
            status: OutdatedStatus::Behind,
            patches_behind: Some(latest.patch.saturating_sub(v.patch)),
        },
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::GoVersion;
    use tempfile::tempdir;

    // Helper: parse a GoVersion, panicking if the string is invalid (test only).
    fn v(s: &str) -> GoVersion {
        GoVersion::parse(s).unwrap()
    }

    #[test]
    fn run_short_circuits_before_any_network_call_when_nothing_installed() {
        // With no installed versions, `run` must return early (printing a
        // hint) without ever calling `index::fetch_releases`, so this must
        // succeed even though no network is available in the test sandbox.
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        let client = HttpClient::new(false, 0).unwrap();
        run(&config, &client, false).unwrap();
    }

    #[test]
    fn run_json_short_circuits_before_any_network_call_when_nothing_installed() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        let client = HttpClient::new(false, 0).unwrap();
        run(&config, &client, true).unwrap();
    }

    #[test]
    fn build_outdated_entry_reports_up_to_date() {
        let entry = build_outdated_entry(&v("go1.23.4"), &[v("go1.23.4")]);
        assert_eq!(entry.status, OutdatedStatus::UpToDate);
        assert_eq!(entry.patches_behind, None);
    }

    #[test]
    fn build_outdated_entry_reports_patches_behind() {
        let entry = build_outdated_entry(&v("go1.22.4"), &[v("go1.22.12")]);
        assert_eq!(entry.status, OutdatedStatus::Behind);
        assert_eq!(entry.patches_behind, Some(8));
    }

    #[test]
    fn build_outdated_entry_reports_unknown_when_no_matching_minor() {
        let entry = build_outdated_entry(&v("go1.10.0"), &[v("go1.23.4")]);
        assert_eq!(entry.status, OutdatedStatus::Unknown);
        assert_eq!(entry.latest, None);
    }

    #[test]
    fn version_ordering_used_for_latest() {
        let mut versions = [v("go1.21.3"), v("go1.21.13"), v("go1.21.1")];
        versions.sort();
        assert_eq!(versions.last().unwrap(), &v("go1.21.13"));
    }

    #[test]
    fn patch_diff_calculation() {
        let installed = v("go1.21.0");
        let latest = v("go1.21.13");
        let diff = latest.patch.saturating_sub(installed.patch);
        assert_eq!(diff, 13);
    }

    #[test]
    fn up_to_date_detection() {
        let installed = v("go1.23.4");
        let latest = v("go1.23.4");
        assert!(installed >= latest);
    }

    #[test]
    fn outdated_detection() {
        let installed = v("go1.22.4");
        let latest = v("go1.22.12");
        assert!(installed < latest);
    }

    #[test]
    fn version_without_patch_compared_correctly() {
        // go1.22 (patch = 0) should be behind go1.22.1
        let installed = v("go1.22");
        let latest = v("go1.22.12");
        assert!(installed < latest);
        let diff = latest.patch.saturating_sub(installed.patch);
        assert_eq!(diff, 12);
    }
}
