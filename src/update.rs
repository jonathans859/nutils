//! Update checks, handed to `nutils-settings.exe`, which carries the updater
//! (ship-shape, built on wxWidgets, which the core deliberately doesn't link).
//!
//! At startup the core runs `nutils-settings.exe --check` on a worker thread and
//! reads the one line it prints; a newer release is posted back to the main
//! window as [`WM_UPDATE_FOUND`], which puts it in the tray. Updating itself —
//! release notes, download, signature check, install — is the settings program's
//! `--update` mode, which also closes and restarts this program.

use crate::config::exe_dir;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Posted to the main window when the startup check found a newer release.
pub const WM_UPDATE_FOUND: u32 = WM_APP + 2;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The newer version the last check found, collected by [`take_found`].
static FOUND: Mutex<Option<String>> = Mutex::new(None);

fn settings_exe() -> std::path::PathBuf {
    exe_dir().join("nutils-settings.exe")
}

/// The version in a `--check` result line, if it reports an update.
fn parse_check(output: &str) -> Option<String> {
    let version = output.lines().next()?.strip_prefix("update ")?.trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// Check for a newer release in the background. Silent unless one is found:
/// no network, no release yet, or a missing settings program just mean no
/// update is shown.
pub fn check_in_background(hwnd: HWND) {
    let hwnd = hwnd.0 as isize; // HWND isn't Send
    std::thread::spawn(move || {
        let Ok(out) = Command::new(settings_exe())
            .arg("--check")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        else {
            return;
        };
        if let Some(version) = parse_check(&String::from_utf8_lossy(&out.stdout)) {
            if let Ok(mut found) = FOUND.lock() {
                *found = Some(version);
            }
            unsafe {
                let hwnd = HWND(hwnd as *mut core::ffi::c_void);
                let _ = PostMessageW(Some(hwnd), WM_UPDATE_FOUND, WPARAM(0), LPARAM(0));
            }
        }
    });
}

/// The version found by the background check, once.
pub fn take_found() -> Option<String> {
    FOUND.lock().ok()?.take()
}

/// Start the update flow (check, release notes, download, install). Returns
/// false if the settings program couldn't be started.
pub fn launch() -> bool {
    Command::new(settings_exe()).arg("--update").spawn().is_ok()
}

/// Delete the files an update moved aside (`*.old`). One still loaded somewhere
/// (the hook DLL in an app that hasn't let go yet) stays until a later start.
pub fn clean_up_old_files() {
    let Ok(entries) = std::fs::read_dir(exe_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("old")) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_of_an_update() {
        assert_eq!(parse_check("update v4.1.0\r\n").as_deref(), Some("v4.1.0"));
    }

    #[test]
    fn anything_else_is_no_update() {
        assert_eq!(parse_check("current v4.0.0\n"), None);
        assert_eq!(parse_check("error HTTP error: 404\n"), None);
        assert_eq!(parse_check(""), None);
        assert_eq!(parse_check("update \n"), None);
    }
}
