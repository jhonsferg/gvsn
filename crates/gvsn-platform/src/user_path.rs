//! Windows user PATH (`HKCU\Environment`) management.
//!
//! `gvsn setup` adds the gvsn binary directory and `~\.gvsn\current\bin` to
//! the user PATH so every application sees them; `gvsn setup --reset` and
//! `gvsn implode` remove them again. This module only reads and writes the
//! registry value and notifies running processes; all user-facing output
//! stays in the CLI.

/// Returns `true` if `candidate` is already present in `entries`.
///
/// Windows paths are case-insensitive, so a PATH entry that differs from
/// `candidate` only in case (e.g. a different drive-letter casing from
/// another shell) still counts as present - comparing `Path` values directly
/// would miss this and insert a duplicate entry on every `gvsn setup` run.
///
/// Kept unconditional (not `#[cfg(windows)]`) so its logic is unit-tested on
/// every platform.
pub fn path_already_present(entries: &[String], candidate: &std::path::Path) -> bool {
    let candidate = candidate.to_string_lossy();
    entries
        .iter()
        .any(|e| e.eq_ignore_ascii_case(candidate.as_ref()))
}

/// Prepends each of `dirs` to the user PATH unless already present.
///
/// Directories are processed in order and each one is inserted at the front,
/// so the last directory ends up first. Returns, for every directory, whether
/// it was added (`true`) or was already present (`false`). The registry is
/// only written, and the change broadcast, when at least one entry is added.
///
/// # Errors
///
/// Returns an error if `HKCU\Environment` cannot be opened or written.
#[cfg(windows)]
pub fn prepend_user_path_entries(
    dirs: &[std::path::PathBuf],
) -> anyhow::Result<Vec<(std::path::PathBuf, bool)>> {
    use anyhow::Context;

    let env = open_environment_key()?;
    let current_path: String = env.get_value("PATH").unwrap_or_default();
    let mut entries: Vec<String> = current_path
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    let mut report = Vec::with_capacity(dirs.len());
    for dir in dirs {
        let added = !path_already_present(&entries, dir);
        if added {
            entries.insert(0, dir.to_string_lossy().to_string());
        }
        report.push((dir.clone(), added));
    }

    if report.iter().any(|(_, added)| *added) {
        env.set_value("PATH", &entries.join(";"))
            .context("Cannot write PATH to HKCU\\Environment")?;
        broadcast_environment_change();
    }

    Ok(report)
}

/// Removes every user PATH entry located inside `root`.
///
/// Returns `true` if the registry value changed. The change is broadcast
/// only in that case.
///
/// # Errors
///
/// Returns an error if `HKCU\Environment` cannot be opened or written.
#[cfg(windows)]
pub fn remove_user_path_under(root: &std::path::Path) -> anyhow::Result<bool> {
    use anyhow::Context;

    let env = open_environment_key()?;
    let current_path: String = env.get_value("PATH").unwrap_or_default();

    let filtered: Vec<&str> = current_path
        .split(';')
        .filter(|e| !std::path::Path::new(e).starts_with(root))
        .collect();

    let new_path = filtered.join(";");
    if new_path == current_path {
        return Ok(false);
    }
    env.set_value("PATH", &new_path)
        .context("Cannot write PATH to HKCU\\Environment")?;
    broadcast_environment_change();
    Ok(true)
}

/// Opens `HKCU\Environment` for reading and writing.
#[cfg(windows)]
fn open_environment_key() -> anyhow::Result<winreg::RegKey> {
    use anyhow::Context;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("Cannot open HKCU\\Environment registry key")
}

/// Broadcasts `WM_SETTINGCHANGE` so running processes - most importantly
/// Explorer - refresh their cached environment block immediately.
///
/// Writing `HKCU\Environment` alone is not enough: Explorer (and anything it
/// spawns - the Start Menu, the taskbar, double-clicking an `.exe`) keeps its
/// own copy of the environment from whenever it last started, and normally
/// only refreshes it on this broadcast, a logoff, or a restart. Without it, a
/// GUI application like VS Code or GoLand launched right after `gvsn setup`/
/// `gvsn use` still sees the *old* PATH and fails to find `go`, even though
/// any terminal opened afterward works fine (terminals inherit PATH fresh
/// from Explorer at spawn time once Explorer itself has the update).
///
/// This only affects newly spawned processes - a GUI app that was already
/// running keeps whatever environment it started with regardless, since
/// Windows has no way to inject new environment variables into a live
/// process. It still needs to be closed and reopened once.
///
/// Best-effort: failures are ignored. Worst case, the pre-existing "restart
/// your terminal or log out/in" guidance still applies.
#[cfg(windows)]
#[allow(unsafe_code)]
fn broadcast_environment_change() {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    let param: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result: usize = 0;
    // SAFETY: `param` is a NUL-terminated UTF-16 buffer that outlives the
    // call, and `result` is a valid out-pointer for its whole duration.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST as HWND,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Smoke test only: confirms the WM_SETTINGCHANGE broadcast itself is a
    // well-formed call that doesn't crash the process. It never touches the
    // registry, so it's safe to run in CI - it can't verify that Explorer
    // actually refreshes its cached environment, which requires a real
    // interactive desktop session to observe.
    #[cfg(windows)]
    #[test]
    fn broadcast_environment_change_does_not_panic() {
        broadcast_environment_change();
    }

    #[test]
    fn path_already_present_matches_case_insensitively() {
        let entries = vec![r"C:\Users\jhon\.gvsn\current\bin".to_string()];
        let candidate = std::path::Path::new(r"c:\users\jhon\.gvsn\current\bin");
        assert!(path_already_present(&entries, candidate));
    }

    #[test]
    fn path_already_present_is_false_for_unrelated_entry() {
        let entries = vec![r"C:\Users\jhon\.cargo\bin".to_string()];
        let candidate = std::path::Path::new(r"C:\Users\jhon\.gvsn\current\bin");
        assert!(!path_already_present(&entries, candidate));
    }
}
