//! Updates from GitHub Releases, through ship-shape.
//!
//! The updater lives in the settings editor rather than in `nutils.exe`, because
//! ship-shape is built on wxDragon and the always-running core stays free of
//! wxWidgets. The core asks this program instead:
//!
//! - `nutils-settings.exe --check` prints the result of a check (no window), for
//!   the startup check that puts "update available" in the tray;
//! - `nutils-settings.exe --update` runs the whole flow (release notes, download,
//!   install), as does the "Check for updates now" button in Settings.
//!
//! ship-shape checks GitHub, picks `nutils.zip`, downloads it and verifies its
//! minisign signature, and shows the update dialog. The install step is our own:
//! ship-shape's would wait only for *this* process and restart it, but NUtils is
//! two programs, and the running `nutils.exe` (and `nutils_hook.dll`, loaded into
//! auto-transparent apps) must be stopped or moved aside before the zip can
//! replace them, and `nutils.exe` is the one to restart.

use ship_shape::{UpdateChannel, UpdateCheckOutcome, UpdateError, UpdaterConfig};
use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use wxdragon::prelude::*;

const REPO: &str = "jonathans859/nutils";
/// Releases are signed with the matching secret key (a GitHub Actions secret);
/// an update whose signature doesn't verify against this is refused.
const PUBLIC_KEY: &str = "RWTdMF3Xh6dZKD4XVJli0pOwD53QyhMIDairBCza5D0BVG7waRKWUFF+";

fn config() -> UpdaterConfig {
    UpdaterConfig::new(
        REPO,
        "nutils", // release asset: nutils.zip + nutils.zip.minisig
        "NUtils",
        PUBLIC_KEY,
        format!("nutils/{}", env!("CARGO_PKG_VERSION")),
    )
}

/// The running version. Debug builds can pretend to be older, to try the update
/// flow against a real release: `NUTILS_PRETEND_VERSION=0.1.0`.
fn current_version() -> String {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("NUTILS_PRETEND_VERSION") {
        return v;
    }
    env!("CARGO_PKG_VERSION").to_string()
}

fn check() -> Result<UpdateCheckOutcome, UpdateError> {
    // Stable channel: compares release tags, so no commit hash is needed.
    ship_shape::check_for_updates(&config(), &current_version(), "", false, UpdateChannel::Stable)
}

/// `--check`: print one line for the core to read — `update <version>`,
/// `current <version>` or `error <message>` — and return the exit code.
pub fn check_headless() -> i32 {
    match check() {
        Ok(UpdateCheckOutcome::UpdateAvailable(r)) => {
            println!("update {}", r.latest_version);
            0
        }
        Ok(UpdateCheckOutcome::UpToDate(v)) => {
            println!("current {v}");
            0
        }
        Err(e) => {
            println!("error {e}");
            1
        }
    }
}

/// A window handle that can cross threads; turned back into a dialog parent on
/// the UI thread.
#[derive(Clone, Copy)]
struct Parent(usize);

impl WxWidget for Parent {
    fn handle_ptr(&self) -> *mut wxdragon::ffi::wxd_Window_t {
        self.0 as *mut _
    }
}

thread_local! {
    static PROGRESS: RefCell<Option<ProgressDialog>> = const { RefCell::new(None) };
}

/// Guards against two flows at once (e.g. the button pressed twice).
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Run the whole update flow with dialogs parented to `parent`: check, show the
/// release notes, download and verify, install. Every outcome is reported, so
/// "no update" and errors are said too. With `exit_when_done` (the standalone
/// `--update` mode) the process ends when the flow does.
pub fn run(parent: &dyn WxWidget, exit_when_done: bool) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let parent = Parent(parent.handle_ptr() as usize);
    std::thread::spawn(move || {
        let outcome = check();
        wxdragon::call_after(Box::new(move || present(parent, outcome, exit_when_done)));
        wxdragon::wake_up_idle();
    });
}

fn finish(exit_when_done: bool) {
    RUNNING.store(false, Ordering::SeqCst);
    if exit_when_done {
        std::process::exit(0);
    }
}

fn message(parent: Parent, text: &str, title: &str, style: MessageDialogStyle) {
    MessageDialog::builder(&parent, text, title)
        .with_style(MessageDialogStyle::OK | style | MessageDialogStyle::Centre)
        .build()
        .show_modal();
}

fn present(parent: Parent, outcome: Result<UpdateCheckOutcome, UpdateError>, exit_when_done: bool) {
    let result = match outcome {
        Ok(UpdateCheckOutcome::UpdateAvailable(result)) => result,
        Ok(UpdateCheckOutcome::UpToDate(latest)) => {
            let text = format!(
                "NUtils is up to date. You have version {}; the latest release is {latest}.",
                env!("CARGO_PKG_VERSION")
            );
            message(parent, &text, "NUtils Update", MessageDialogStyle::IconInformation);
            return finish(exit_when_done);
        }
        Err(e) => {
            let text = format!("Could not check for updates.\n\n{e}");
            message(parent, &text, "NUtils Update", MessageDialogStyle::IconError);
            return finish(exit_when_done);
        }
    };
    let notes = ship_shape::ui::markdown_to_text(&result.release_notes);
    let notes = if notes.trim().is_empty() { "No release notes.".to_string() } else { notes };
    if !ship_shape::ui::show_update_dialog(&parent, &result.latest_version, &notes, "NUtils") {
        return finish(exit_when_done);
    }
    download(parent, result.download_url, result.signature_url, exit_when_done);
}

/// Download and verify on a worker thread, with a progress dialog on the UI
/// thread (updated five times a second) that can cancel it.
fn download(parent: Parent, url: String, signature_url: String, exit_when_done: bool) {
    let dialog = ProgressDialog::builder(&parent, "NUtils Update", "Downloading the update...", 100)
        .with_style(
            ProgressDialogStyle::AutoHide
                | ProgressDialogStyle::AppModal
                | ProgressDialogStyle::CanAbort,
        )
        .build();
    PROGRESS.with(|p| *p.borrow_mut() = Some(dialog));

    let done = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let (got, total) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));

    {
        let (done, cancelled, got, total) = (done.clone(), cancelled.clone(), got.clone(), total.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) && !cancelled.load(Ordering::Relaxed) {
                let (d, t) = (got.load(Ordering::Relaxed), total.load(Ordering::Relaxed));
                let cancelled = cancelled.clone();
                wxdragon::call_after(Box::new(move || {
                    PROGRESS.with(|p| {
                        let keep_going = match p.borrow().as_ref() {
                            Some(dialog) => match (d * 100).checked_div(t) {
                                Some(pct) => dialog.update(pct.min(100) as i32, None),
                                None => dialog.pulse(None),
                            },
                            None => return,
                        };
                        if !keep_going {
                            cancelled.store(true, Ordering::Relaxed);
                            *p.borrow_mut() = None;
                        }
                    });
                }));
                wxdragon::wake_up_idle();
                std::thread::sleep(Duration::from_millis(200));
            }
        });
    }

    std::thread::spawn(move || {
        let res = ship_shape::download_update_file(&config(), &url, &signature_url, &cancelled, |d, t| {
            got.store(d, Ordering::Relaxed);
            total.store(t, Ordering::Relaxed);
        });
        done.store(true, Ordering::Relaxed);
        wxdragon::call_after(Box::new(move || {
            PROGRESS.with(|p| *p.borrow_mut() = None);
            match res {
                Ok(zip) => match install(&zip) {
                    // The install script takes over: it waits for us to exit.
                    Ok(()) => std::process::exit(0),
                    Err(e) => {
                        let _ = std::fs::remove_file(&zip);
                        message(parent, &format!("Could not install the update.\n\n{e}"), "NUtils Update", MessageDialogStyle::IconError);
                    }
                },
                Err(UpdateError::Cancelled) => {}
                Err(UpdateError::VerificationError(e)) => message(
                    parent,
                    &format!("The download did not pass the signature check, so it was not installed. It may have been tampered with.\n\n{e}"),
                    "NUtils Update",
                    MessageDialogStyle::IconError,
                ),
                Err(e) => message(parent, &format!("Could not download the update.\n\n{e}"), "NUtils Update", MessageDialogStyle::IconError),
            }
            finish(exit_when_done);
        }));
        wxdragon::wake_up_idle();
    });
}

/// The files the release zip replaces that may be in use: NUtils itself, this
/// program, and the hook DLL (loaded into auto-transparent apps).
const IN_USE: [&str; 3] = ["nutils.exe", "nutils-settings.exe", "nutils_hook.dll"];

/// Close the NUtils running from this folder, then hand over to a PowerShell
/// script that waits for both programs to exit, moves the in-use files aside
/// (Windows allows renaming a running exe or loaded DLL, not replacing it),
/// unpacks the zip, and starts the new `nutils.exe`. If unpacking fails, the
/// error goes to `update-error.log` in the folder, the old files are put back and
/// the old NUtils restarted. NUtils deletes the moved-aside
/// `*.old` files at its next start (or later, once nothing holds them).
fn install(zip: &Path) -> Result<(), String> {
    let dir = zip.parent().ok_or("the download has no folder")?;
    let core_pid = core::close(dir);
    let script = install_script(std::process::id(), core_pid, zip, dir);
    use std::os::windows::process::CommandExt;
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|e| format!("Failed to start the install script: {e}"))?;
    Ok(())
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn install_script(our_pid: u32, core_pid: Option<u32>, zip: &Path, dir: &Path) -> String {
    let pids = match core_pid {
        Some(core) => format!("{our_pid},{core}"),
        None => our_pid.to_string(),
    };
    let files = IN_USE.iter().map(|f| ps_quote(f)).collect::<Vec<_>>().join(",");
    format!(
        "$ErrorActionPreference = 'Stop'
$dir = {dir}; $zip = {zip}
try {{ Wait-Process -Id {pids} -Timeout 30 -ErrorAction SilentlyContinue }} catch {{}}
$moved = @{{}}
try {{
    foreach ($f in @({files})) {{
        $p = Join-Path $dir $f
        if (Test-Path $p) {{
            $old = \"$p.$(Get-Random).old\"
            Move-Item -LiteralPath $p -Destination $old
            $moved[$p] = $old
        }}
    }}
    Expand-Archive -LiteralPath $zip -DestinationPath $dir -Force
}} catch {{
    $_ | Out-File -LiteralPath (Join-Path $dir 'update-error.log')
    foreach ($p in $moved.Keys) {{ Move-Item -LiteralPath $moved[$p] -Destination $p -Force -ErrorAction SilentlyContinue }}
}}
Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue
Start-Process -FilePath (Join-Path $dir 'nutils.exe') -WorkingDirectory $dir",
        dir = ps_quote(&dir.display().to_string()),
        zip = ps_quote(&zip.display().to_string()),
    )
}

/// Finding and closing the NUtils core that runs from a given folder.
mod core {
    use std::path::Path;
    use windows::core::{w, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };

    fn exe_of(pid: u32) -> Option<std::path::PathBuf> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(h);
            ok.ok()?;
            Some(String::from_utf16_lossy(&buf[..len as usize]).into())
        }
    }

    /// Ask the NUtils whose exe is in `dir` to exit (a copy elsewhere is left
    /// alone), returning its process id so the install script can wait for it.
    pub fn close(dir: &Path) -> Option<u32> {
        let mut after: Option<HWND> = None;
        loop {
            let hwnd = unsafe {
                FindWindowExW(None, after, w!("NUtilsMainWnd"), None).ok()?
            };
            after = Some(hwnd);
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            let same_dir = exe_of(pid)
                .and_then(|exe| exe.parent().map(|d| d.eq(dir)))
                .unwrap_or(false);
            if same_dir {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                return Some(pid);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_waits_for_both_programs_and_restarts_the_core() {
        let s = install_script(10, Some(20), Path::new(r"C:\N\nutils.zip"), Path::new(r"C:\N"));
        assert!(s.contains("Wait-Process -Id 10,20 "));
        assert!(s.contains(r"Expand-Archive -LiteralPath $zip -DestinationPath $dir -Force"));
        assert!(s.contains(r"$dir = 'C:\N'; $zip = 'C:\N\nutils.zip'"));
        assert!(s.contains("'nutils.exe','nutils-settings.exe','nutils_hook.dll'"));
        assert!(s.ends_with("Start-Process -FilePath (Join-Path $dir 'nutils.exe') -WorkingDirectory $dir"));
    }

    /// Runs the real script on a scratch folder: old files (one a DLL held
    /// loaded, as nutils_hook.dll is by auto-transparent apps) must be moved
    /// aside, the zip unpacked over them and deleted.
    ///   cargo test --release -p nutils-settings install_script_replaces -- --ignored
    #[test]
    #[ignore]
    fn install_script_replaces_files_even_when_in_use() {
        use std::os::windows::ffi::OsStrExt;
        let dir = std::env::temp_dir().join(format!("nutils-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let new = dir.join("new");
        std::fs::create_dir_all(&new).unwrap();
        // "Installed" files, including a real DLL we keep loaded.
        let dll = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("nutils_hook.dll");
        std::fs::copy(&dll, dir.join("nutils_hook.dll")).expect("build nutils-hook first");
        std::fs::write(dir.join("nutils.exe"), "old").unwrap();
        std::fs::write(dir.join("nutils-settings.exe"), "old").unwrap();
        std::fs::write(dir.join("config.toml"), "mine").unwrap();
        let wide: Vec<u16> = dir.join("nutils_hook.dll").as_os_str().encode_wide().chain([0]).collect();
        let held = unsafe {
            windows::Win32::System::LibraryLoader::LoadLibraryW(windows::core::PCWSTR(wide.as_ptr()))
        }
        .expect("load the DLL");
        // The release zip.
        for f in IN_USE {
            std::fs::write(new.join(f), "new").unwrap();
        }
        let zip = dir.join("nutils.zip");
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command"])
            .arg(format!("Compress-Archive -Path '{}' -DestinationPath '{}'", new.join("*").display(), zip.display()))
            .status()
            .unwrap();
        assert!(status.success());

        // Our pid is running, so wait on one that has finished instead.
        let mut done = std::process::Command::new("cmd.exe").args(["/c", "exit"]).spawn().unwrap();
        let finished = done.id();
        done.wait().unwrap();
        let script = install_script(finished, None, &zip, &dir);
        let _ = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .status()
            .unwrap();

        for f in IN_USE {
            assert_eq!(std::fs::read_to_string(dir.join(f)).unwrap(), "new", "{f} replaced");
        }
        assert_eq!(std::fs::read_to_string(dir.join("config.toml")).unwrap(), "mine", "settings kept");
        assert!(!zip.exists(), "zip removed");
        let old = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "old"))
            .count();
        assert_eq!(old, 3, "the replaced files were moved aside as *.old");
        assert!(!dir.join("update-error.log").exists(), "no error logged");
        unsafe {
            let _ = windows::Win32::Foundation::FreeLibrary(held);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn script_quotes_paths_and_works_without_a_running_core() {
        let s = install_script(10, None, Path::new(r"C:\O'Brien\nutils.zip"), Path::new(r"C:\O'Brien"));
        assert!(s.contains("Wait-Process -Id 10 "));
        assert!(s.contains(r"$dir = 'C:\O''Brien'"));
    }
}
