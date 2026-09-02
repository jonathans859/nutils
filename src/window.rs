//! Thin, safe-ish wrappers around the Win32 window/process operations NUtils needs.
//! All of these must be called from the GUI thread that owns the message loop.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, SetActiveWindow, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// A window handle stored as a plain integer so it can live in serializable state.
pub type WinId = isize;

pub fn to_id(hwnd: HWND) -> WinId {
    hwnd.0 as isize
}

pub fn from_id(id: WinId) -> HWND {
    HWND(id as *mut core::ffi::c_void)
}

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// The top-level (root) ancestor of a window; NUtils always acts on this.
pub fn root(hwnd: HWND) -> HWND {
    unsafe { GetAncestor(hwnd, GA_ROOT) }
}

pub fn exists(hwnd: HWND) -> bool {
    unsafe { IsWindow(Some(hwnd)).as_bool() }
}

pub fn is_visible(hwnd: HWND) -> bool {
    unsafe { IsWindowVisible(hwnd).as_bool() }
}

/// True for a real top-level application window (has a title bar area / is a root,
/// not a tooltip/menu). Used to filter WinEvent noise.
pub fn is_top_level(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    unsafe { GetAncestor(hwnd, GA_ROOT) == hwnd }
}

pub fn hide(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

pub fn show(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        // If it was minimized while hidden, bring it back to its real size.
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        force_foreground(hwnd);
    }
}

/// Bring `hwnd` to the foreground and give it focus.
///
/// A bare `SetForegroundWindow` from a background process is usually refused by
/// Windows' foreground-lock rules (the window just blinks in the taskbar). We use
/// two workarounds together, because some apps resist just one:
///   * temporarily zero the system **foreground-lock timeout**, which is the
///     setting that actually causes the refusal; and
///   * briefly attach our input thread to the current foreground window's thread
///     (and the target's), so activation and focus are permitted.
/// Both are restored/detached afterwards.
fn force_foreground(hwnd: HWND) {
    unsafe {
        // Zero the foreground-lock timeout for the duration (restored below).
        let mut old_timeout: u32 = 0;
        let _ = SystemParametersInfoW(
            SPI_GETFOREGROUNDLOCKTIMEOUT,
            0,
            Some(&mut old_timeout as *mut u32 as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let _ = SystemParametersInfoW(
            SPI_SETFOREGROUNDLOCKTIMEOUT,
            0,
            Some(std::ptr::null_mut()), // value 0
            SPIF_SENDCHANGE,
        );

        let fg = GetForegroundWindow();
        let our_tid = GetCurrentThreadId();
        let fg_tid = if fg.0.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, None)
        };
        let target_tid = GetWindowThreadProcessId(hwnd, None);

        let attach_fg = fg_tid != 0 && fg_tid != our_tid;
        let attach_tgt = target_tid != 0 && target_tid != our_tid && target_tid != fg_tid;
        if attach_fg {
            let _ = AttachThreadInput(our_tid, fg_tid, true);
        }
        if attach_tgt {
            let _ = AttachThreadInput(our_tid, target_tid, true);
        }

        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetActiveWindow(hwnd);
        let _ = SetFocus(Some(hwnd));

        // Escalation for apps that still resist (notably some wxWidgets windows):
        // a synthetic ALT tap makes Windows treat this as real user input, which
        // lifts the foreground restriction. Only done when the clean path failed,
        // so well-behaved apps never see the extra keystroke.
        if GetForegroundWindow() != hwnd {
            synth_alt_tap();
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetActiveWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
        }

        if attach_tgt {
            let _ = AttachThreadInput(our_tid, target_tid, false);
        }
        if attach_fg {
            let _ = AttachThreadInput(our_tid, fg_tid, false);
        }

        // Restore the user's foreground-lock timeout.
        let _ = SystemParametersInfoW(
            SPI_SETFOREGROUNDLOCKTIMEOUT,
            0,
            Some(old_timeout as usize as *mut core::ffi::c_void),
            SPIF_SENDCHANGE,
        );
    }
}

/// Send a single ALT press+release via `SendInput`. Used only to nudge Windows'
/// foreground rules for windows that refuse activation otherwise.
unsafe fn synth_alt_tap() {
    let make = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_MENU,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [make(KEYBD_EVENT_FLAGS(0)), make(KEYEVENTF_KEYUP)];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

/// Make a window transparent (`alpha` 0) or solid (`alpha` 255). A transparent
/// window is still present and readable by a screen reader.
///
/// This is the mechanism behind the manual "make transparent" hotkey. Note that
/// it adds `WS_EX_LAYERED` to the window, which forces a one-off opaque repaint if
/// the window is already on screen — fine for a deliberate keystroke, but it is
/// why the *auto-hide* path uses [`cloak`] instead (see below).
pub fn set_alpha(hwnd: HWND, alpha: u8) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered = WS_EX_LAYERED.0 as isize;
        if ex & layered == 0 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | layered);
        }
        let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), alpha, LWA_ALPHA);
    }
}

// NOTE: DWM cloaking (`DwmSetWindowAttribute` / `DWMWA_CLOAK`) would be a nicer,
// repaint-free hide — but it is rejected cross-process: calling it on a window we
// do not own returns E_ACCESSDENIED (0x80070005). Verified empirically. DWM only
// lets a process cloak its *own* windows, so it is unusable for auto-hiding other
// apps' popups. `set_alpha(hwnd, 0)` above is what we use instead.

pub fn get_title(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

pub fn set_title(hwnd: HWND, title: &str) {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
    }
}

pub fn class_name(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

pub fn owner_pid(hwnd: HWND) -> u32 {
    let mut pid: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    pid
}

/// The image file name (e.g. `wxdragon.exe`) of the process owning `hwnd`, lowercased.
pub fn owner_exe(hwnd: HWND) -> Option<String> {
    let pid = owner_pid(hwnd);
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let res = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        res.ok()?;
        let full = String::from_utf16_lossy(&buf[..size as usize]);
        let name = full.rsplit(['\\', '/']).next().unwrap_or(&full);
        Some(name.to_ascii_lowercase())
    }
}

/// Forcefully terminate the process owning `hwnd`.
pub fn kill_owner(hwnd: HWND) -> bool {
    let pid = owner_pid(hwnd);
    if pid == 0 {
        return false;
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) else {
            return false;
        };
        let ok = TerminateProcess(handle, 1).is_ok();
        let _ = CloseHandle(handle);
        ok
    }
}

/// Politely ask a window to close.
pub fn close(hwnd: HWND) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

/// The six process-priority levels bound to F3..F8, in that order.
pub const PRIORITY_CLASSES: [PROCESS_CREATION_FLAGS; 6] = [
    IDLE_PRIORITY_CLASS,
    BELOW_NORMAL_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS,
    ABOVE_NORMAL_PRIORITY_CLASS,
    HIGH_PRIORITY_CLASS,
    REALTIME_PRIORITY_CLASS,
];

/// Set the priority class (index 0..=5) of the process owning `hwnd`.
pub fn set_priority(hwnd: HWND, index: usize) -> bool {
    let Some(class) = PRIORITY_CLASSES.get(index).copied() else {
        return false;
    };
    let pid = owner_pid(hwnd);
    if pid == 0 {
        return false;
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_SET_INFORMATION, false, pid) else {
            return false;
        };
        let ok = SetPriorityClass(handle, class).is_ok();
        let _ = CloseHandle(handle);
        ok
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let out = &mut *(lparam.0 as *mut Vec<HWND>);
    out.push(hwnd);
    windows::core::BOOL(1)
}

/// All top-level windows currently in the system.
pub fn enum_top_windows() -> Vec<HWND> {
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

/// Whether a window is the desktop or the taskbar (which must never be hidden).
pub fn is_shell_window(hwnd: HWND) -> bool {
    let class = class_name(hwnd);
    class == "Progman" || class == "Shell_TrayWnd" || class == "WorkerW"
}

#[cfg(test)]
mod hook_latency {
    use super::*;
    use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::Instant;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, EVENT_OBJECT_SHOW, MSG, WINEVENT_OUTOFCONTEXT,
    };

    // A monotonic origin shared by both threads, the window we are timing, and the
    // nanoseconds-since-origin at which the hook observed SHOW (0 == not yet).
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    static TARGET: AtomicIsize = AtomicIsize::new(0);
    static SHOW_NS: AtomicU64 = AtomicU64::new(0);

    unsafe extern "system" fn proc(
        _h: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, id_child: i32, _t: u32, _tm: u32,
    ) {
        if event == EVENT_OBJECT_SHOW
            && id_object == 0
            && id_child == 0
            && hwnd.0 as isize == TARGET.load(Ordering::Relaxed)
            && SHOW_NS.load(Ordering::Relaxed) == 0
        {
            let ns = ORIGIN.get().unwrap().elapsed().as_nanos() as u64;
            SHOW_NS.store(ns, Ordering::Relaxed);
        }
    }

    /// Measures the real OUTOFCONTEXT `SetWinEventHook` delivery latency: the gap
    /// between an app calling `ShowWindow` and our hook callback observing it, both
    /// timed against the same monotonic clock. This is the irreducible floor of the
    /// auto-hide feature without injecting into the target process — and it is the
    /// evidence behind the ~1ms figure documented in `winevent.rs`.
    ///
    /// Run with:
    ///   cargo test --release hook_delivery_latency -- --ignored --nocapture
    #[test]
    #[ignore]
    fn hook_delivery_latency() {
        use std::{thread, time::Duration};
        use windows::core::{w, PCWSTR};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;

        ORIGIN.set(Instant::now()).ok();

        // Dedicated hook thread, exactly like production (winevent.rs).
        let _hook_thread = thread::spawn(|| unsafe {
            let hook = SetWinEventHook(
                EVENT_OBJECT_SHOW, EVENT_OBJECT_SHOW, None, Some(proc), 0, 0, WINEVENT_OUTOFCONTEXT,
            );
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
            if !hook.is_invalid() { let _ = UnhookWinEvent(hook); }
        });
        thread::sleep(Duration::from_millis(200)); // let the hook install

        let mut samples: Vec<f64> = Vec::new();
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            for i in 0..20 {
                SHOW_NS.store(0, Ordering::Relaxed);
                // Create a normal overlapped window, initially hidden.
                let hwnd = CreateWindowExW(
                    Default::default(),
                    w!("STATIC"),
                    PCWSTR::null(),
                    WS_OVERLAPPEDWINDOW,
                    100, 100, 200, 150,
                    None, None, Some(hinst.into()), None,
                ).unwrap();
                TARGET.store(hwnd.0 as isize, Ordering::Relaxed);

                // Time the exact ShowWindow call — the moment the pixels could appear.
                let t0 = ORIGIN.get().unwrap().elapsed().as_nanos() as u64;
                let _ = ShowWindow(hwnd, SW_SHOW);

                // Wait (pumping our own messages) for the hook to record the SHOW time.
                let mut got = 0u64;
                for _ in 0..2000 {
                    got = SHOW_NS.load(Ordering::Relaxed);
                    if got != 0 { break; }
                    let mut m = MSG::default();
                    while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                        DispatchMessageW(&m);
                    }
                    thread::sleep(Duration::from_micros(50));
                }
                if got != 0 && i >= 2 {
                    samples.push((got - t0) as f64 / 1_000_000.0); // ns -> ms; drop warm-up
                }
                let _ = DestroyWindow(hwnd);
                thread::sleep(Duration::from_millis(20));
            }
        }

        assert!(!samples.is_empty(), "captured no SHOW callbacks");
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = samples.len();
        let (min, median, max) = (samples[0], samples[n / 2], samples[n - 1]);
        let mean = samples.iter().sum::<f64>() / n as f64;
        println!(
            "OUTOFCONTEXT SHOW-delivery latency over {n} samples: min={min:.2}ms  median={median:.2}ms  mean={mean:.2}ms  max={max:.2}ms"
        );
        // A full 60Hz frame is 16.7ms; delivery must be a small fraction of that.
        assert!(median < 8.0, "hook delivery unexpectedly slow: {median:.2}ms median");
    }
}
