//! Background "a new gvm version is available" notifier.
//!
//! Modeled on the update notices shown by tools like npm: after any regular
//! command finishes, if a newer `gvm` release exists on GitHub, a short
//! colored note is printed to stderr suggesting `gvm upgrade`.
//!
//! # Design
//!
//! - **Cached**: the result is cached in `<GVM_DIR>/update-check.json` for
//!   24 hours, so this never makes a network request on every invocation.
//! - **Non-blocking**: [`spawn`] starts the check on a background thread as
//!   early as possible (before the command's own work), so on a cache miss
//!   the HTTP round trip normally overlaps with - and is hidden by - the
//!   command's own work. [`print_if_ready`] only waits for whatever time
//!   budget is left by the time the command finishes; if the check hasn't
//!   finished by then, it is dropped silently rather than adding a delay.
//! - **Best-effort**: any failure (network error, timeout, unwritable
//!   cache) is swallowed. This feature must never break, delay, or clutter
//!   a command's own output.
//! - **Quiet where it would be intrusive**: not spawned at all for commands
//!   whose stdout is consumed programmatically or that run on every shell
//!   prompt/`cd` via the shell hook (`env`, `path`, `shell`), for
//!   `completions` (piped straight into a completion file), or for
//!   `upgrade` itself (redundant). Also skipped entirely when
//!   `GVM_NO_UPDATE_CHECK` or `CI` is set.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use colored::Colorize;

use crate::cli::Command;
use crate::config::Config;
use crate::gvm_release;

/// Cache file name, relative to the gvm root directory (`GVM_DIR`).
const CACHE_FILE: &str = "update-check.json";

/// How long a cached "latest version" result is trusted before a fresh
/// network check is made again.
const TTL_SECS: u64 = 24 * 60 * 60;

/// Timeout for the background version check. Short and bounded: this is a
/// best-effort background lookup, not a user-initiated action.
const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// Total time [`print_if_ready`] is willing to wait, measured from when
/// [`spawn`] was called - not from when it is itself invoked. Most commands
/// take longer than this to run their own work, in which case the wait is
/// zero.
const TOTAL_BUDGET: Duration = Duration::from_millis(1500);

#[derive(serde::Serialize, serde::Deserialize)]
struct UpdateCache {
    last_checked_unix: u64,
    latest_version: String,
}

/// A newer version found by the background check: `(current, latest)`.
type NewerVersion = (String, String);

/// Handle returned by [`spawn`]: the channel the background thread will
/// eventually send its result on, plus when it was started (used to compute
/// the remaining wait budget in [`print_if_ready`]).
type PendingCheck = (Receiver<Option<NewerVersion>>, Instant);

/// Starts the background version check, unless suppressed.
///
/// Returns `None` immediately (no thread spawned) when the check should not
/// run for `command`, or when `GVM_NO_UPDATE_CHECK`/`CI` is set. Otherwise
/// returns a receiver that will eventually carry `Some((current, latest))`
/// if a newer version was found, or `None` if not (or the check failed).
pub fn spawn(config: &Config, command: &Command) -> Option<PendingCheck> {
    if !should_run_for(command) {
        return None;
    }

    let root = config.root.clone();
    let current = env!("CARGO_PKG_VERSION").to_string();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = check(&root, &current);
        let _ = tx.send(result);
    });

    Some((rx, Instant::now()))
}

/// Waits for the background check (if any) to finish, bounded by the
/// remaining share of [`TOTAL_BUDGET`] since `spawned_at`, and prints the
/// notice if a newer version was found in time.
pub fn print_if_ready(check: Option<PendingCheck>) {
    let Some((rx, spawned_at)) = check else {
        return;
    };
    let remaining = TOTAL_BUDGET.saturating_sub(spawned_at.elapsed());
    if let Ok(Some((current, latest))) = rx.recv_timeout(remaining) {
        eprint!("{}", format_notice(&current, &latest));
    }
}

/// Returns `true` if the notifier should run at all for `command`.
fn should_run_for(command: &Command) -> bool {
    should_run_for_env(
        command,
        std::env::var_os("GVM_NO_UPDATE_CHECK").is_some(),
        std::env::var_os("CI").is_some(),
    )
}

/// Pure logic behind [`should_run_for`], taking the two suppressing env
/// vars as explicit parameters so it can be unit-tested without mutating
/// real process environment state - both to avoid ambient `CI=true`
/// already being set on every real CI runner, and to avoid races between
/// tests that would otherwise need to mutate the same global env vars.
fn should_run_for_env(command: &Command, no_update_check: bool, ci: bool) -> bool {
    if no_update_check || ci {
        return false;
    }
    !matches!(
        command,
        Command::Env { .. }
            | Command::Path { .. }
            | Command::Shell { .. }
            | Command::Completions { .. }
            | Command::Upgrade { .. }
    )
}

/// Performs the actual cached lookup. Returns `Some((current, latest))` when
/// `latest` is newer than `current`, `None` otherwise (up to date, parse
/// failure, or the network check failed).
fn check(root: &Path, current: &str) -> Option<NewerVersion> {
    let cache_path = root.join(CACHE_FILE);
    let now = now_unix();

    let latest = match load_cache(&cache_path) {
        Some(cache) if !is_stale(cache.last_checked_unix, now) => cache.latest_version,
        _ => {
            let version = gvm_release::fetch_latest_version(CHECK_TIMEOUT).ok()?;
            let _ = save_cache(
                &cache_path,
                &UpdateCache {
                    last_checked_unix: now,
                    latest_version: version.clone(),
                },
            );
            version
        }
    };

    let latest_parsed = gvm_release::parse_semver(&latest)?;
    let current_parsed = gvm_release::parse_semver(current)?;

    if latest_parsed > current_parsed {
        Some((current.to_string(), latest))
    } else {
        None
    }
}

fn load_cache(path: &Path) -> Option<UpdateCache> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_cache(path: &Path, cache: &UpdateCache) -> std::io::Result<()> {
    let content = serde_json::to_string(cache).unwrap_or_default();
    std::fs::write(path, content)
}

/// Returns `true` when a cached check performed at `last_checked_unix` is
/// old enough (relative to `now_unix`) that a fresh one should be made.
fn is_stale(last_checked_unix: u64, now_unix: u64) -> bool {
    now_unix.saturating_sub(last_checked_unix) >= TTL_SECS
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Formats the colored, multi-line notice printed to stderr.
fn format_notice(current: &str, latest: &str) -> String {
    format!(
        "\n  {} {} {} {}\n    Run {} to update.\n",
        "↑ Update available:".yellow().bold(),
        format!("v{current}").dimmed(),
        "→".dimmed(),
        format!("v{latest}").bold().green(),
        "gvm upgrade".cyan()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn is_stale_before_ttl_is_false() {
        assert!(!is_stale(1000, 1000 + TTL_SECS - 1));
    }

    #[test]
    fn is_stale_at_or_after_ttl_is_true() {
        assert!(is_stale(1000, 1000 + TTL_SECS));
        assert!(is_stale(1000, 1000 + TTL_SECS + 1));
    }

    #[test]
    fn is_stale_handles_clock_going_backwards() {
        // now < last_checked (e.g. system clock adjusted) must not panic or
        // report stale via underflow.
        assert!(!is_stale(1000, 500));
    }

    #[test]
    fn cache_round_trips_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CACHE_FILE);

        let cache = UpdateCache {
            last_checked_unix: 12345,
            latest_version: "9.9.9".to_string(),
        };
        save_cache(&path, &cache).unwrap();

        let loaded = load_cache(&path).unwrap();
        assert_eq!(loaded.last_checked_unix, 12345);
        assert_eq!(loaded.latest_version, "9.9.9");
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
    fn should_run_for_excludes_hook_and_scripted_commands() {
        assert!(!should_run_for_env(
            &Command::Env { shell: None },
            false,
            false
        ));
        assert!(!should_run_for_env(
            &Command::Path { version: None },
            false,
            false
        ));
        assert!(!should_run_for_env(
            &Command::Shell {
                version: None,
                unset: true,
                shell: None
            },
            false,
            false
        ));
        assert!(!should_run_for_env(
            &Command::Completions {
                shell: "bash".to_string()
            },
            false,
            false
        ));
        assert!(!should_run_for_env(
            &Command::Upgrade {
                force: false,
                download: crate::cli::DownloadArgs { retries: 3 }
            },
            false,
            false
        ));
    }

    #[test]
    fn should_run_for_allows_regular_commands() {
        assert!(should_run_for_env(
            &Command::List { json: false },
            false,
            false
        ));
        assert!(should_run_for_env(&Command::Current, false, false));
        assert!(should_run_for_env(
            &Command::Doctor {
                shell: None,
                fix: false
            },
            false,
            false
        ));
    }

    #[test]
    fn should_run_for_respects_no_update_check_env() {
        assert!(!should_run_for_env(
            &Command::List { json: false },
            true,
            false
        ));
    }

    #[test]
    fn should_run_for_respects_ci_env() {
        assert!(!should_run_for_env(
            &Command::List { json: false },
            false,
            true
        ));
    }

    #[test]
    fn format_notice_mentions_both_versions_and_the_upgrade_command() {
        let msg = format_notice("1.9.2", "1.10.0");
        assert!(msg.contains("1.9.2"));
        assert!(msg.contains("1.10.0"));
        assert!(msg.contains("gvm upgrade"));
    }
}
