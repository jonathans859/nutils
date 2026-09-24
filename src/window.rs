//! Thin, safe-ish wrappers around the Win32 window/process operations NUtils needs.
//! All of these must be called from the GUI thread that owns the message loop.

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, COLORREF, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE,
};
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
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

/// Marks a window NUtils has hidden; the value is its slot plus one (0 would
/// read as "absent"). Kept on the window itself, so a hidden window can be found
/// again even if `state.toml` is lost: see [`hidden_slot`].
const HIDDEN_PROP: PCWSTR = w!("NUtils.HiddenSlot");

/// Hide a window into `slot`, marking it with the slot.
pub fn hide(hwnd: HWND, slot: usize) {
    unsafe {
        let _ = SetPropW(hwnd, HIDDEN_PROP, Some(HANDLE((slot + 1) as *mut core::ffi::c_void)));
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// The slot NUtils hid this window into, if it carries the mark.
pub fn hidden_slot(hwnd: HWND) -> Option<usize> {
    let v = unsafe { GetPropW(hwnd, HIDDEN_PROP).0 as usize };
    v.checked_sub(1)
}

/// Remove the hidden-window mark (the window was shown, here or elsewhere).
pub fn clear_hidden_mark(hwnd: HWND) {
    unsafe {
        let _ = RemovePropW(hwnd, HIDDEN_PROP);
    }
}

pub fn show(hwnd: HWND) {
    clear_hidden_mark(hwnd);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        // If it was minimized while hidden, bring it back to its real size.
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        force_foreground(hwnd);
    }
}

/// Bring `hwnd` to the foreground, leaving keyboard focus to the app.
///
/// Focus is deliberately *not* set here. Activating a window makes the app put
/// focus back on the control that had it (wxWidgets, dialogs and most frameworks
/// remember it); a `SetFocus` on the top-level window afterwards would move focus
/// off that control onto the bare frame, where a screen reader finds nothing —
/// which is what an earlier version did. Apps that don't restore focus get
/// Windows' default: focus on the window itself.
///
/// NUtils has just received the user's hotkey (or tray click), so Windows
/// normally allows it to change the foreground window and a plain
/// `SetForegroundWindow` is enough. Only if that is refused do we escalate:
///   * temporarily zero the system **foreground-lock timeout**;
///   * briefly attach our input thread to the foreground window's thread (and
///     the target's), so activation is permitted;
///   * as a last resort, a synthetic ALT tap, which Windows treats as real user
///     input.
///
/// Everything is restored/detached afterwards.
fn force_foreground(hwnd: HWND) {
    unsafe {
        let _ = BringWindowToTop(hwnd);
        if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            return;
        }

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

        // Escalation for windows that still resist: a synthetic ALT tap makes
        // Windows treat this as real user input, which lifts the restriction.
        if GetForegroundWindow() != hwnd {
            synth_alt_tap();
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
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

// ---- transparency -----------------------------------------------------------
//
// A transparent window is a layered window (`WS_EX_LAYERED`) at alpha 0: still
// present, focusable and readable by a screen reader, just not drawn.
//
// Before the first change NUtils records the window's original layered state in
// a window property, so making it solid puts back exactly what was there. That
// matters for apps that draw themselves with `UpdateLayeredWindow` (per-pixel
// alpha: WPF, Qt, Java translucent windows): once `SetLayeredWindowAttributes`
// has been used on such a window, the app's own `UpdateLayeredWindow` calls fail
// until the layered style is cleared and set again — so "alpha 255" alone would
// leave it unable to draw. The property lives on the window itself, so it
// survives a NUtils restart, and `nutils_hook.dll` records it the same way
// (keep the two encodings in step).
//
// NOTE: DWM cloaking (`DwmSetWindowAttribute` / `DWMWA_CLOAK`) would be a nicer,
// repaint-free hide — but it is rejected cross-process: calling it on a window we
// do not own returns E_ACCESSDENIED (0x80070005). Verified empirically.

/// Present on a window whose layered state NUtils changed; the value says how to
/// put it back (the `ORIG_*` bits, alpha in bits 8..16, LWA flags in 16..24).
const ORIG_PROP: PCWSTR = w!("NUtils.Original");
/// The original colour key plus one (a property value of 0 reads as absent).
const ORIG_KEY_PROP: PCWSTR = w!("NUtils.OriginalKey");
const ORIG_SET: usize = 1; // always set, so the value is never 0
const ORIG_LAYERED: usize = 2; // the window already had WS_EX_LAYERED
const ORIG_ATTRS: usize = 4; // ...set via SetLayeredWindowAttributes (else UpdateLayeredWindow)

fn layered_attrs(hwnd: HWND) -> Option<(COLORREF, u8, LAYERED_WINDOW_ATTRIBUTES_FLAGS)> {
    let (mut key, mut alpha, mut flags) = (COLORREF(0), 0u8, LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
    unsafe { GetLayeredWindowAttributes(hwnd, Some(&mut key), Some(&mut alpha), Some(&mut flags)) }
        .ok()
        .map(|_| (key, alpha, flags))
}

fn is_layered(hwnd: HWND) -> bool {
    unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_LAYERED.0 as isize != 0 }
}

fn set_layered(hwnd: HWND, on: bool) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered = WS_EX_LAYERED.0 as isize;
        let new = if on { ex | layered } else { ex & !layered };
        if new != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
        }
    }
}

/// Record the window's layered state, unless it is already recorded.
fn record_original(hwnd: HWND) {
    unsafe {
        if !GetPropW(hwnd, ORIG_PROP).0.is_null() {
            return;
        }
        // Already at alpha 0 with no record means another path (the hook DLL, the
        // event watcher) got there a moment earlier; don't record our own change
        // as the original. Restoring then means "not layered", i.e. drawn normally.
        let mut value = ORIG_SET;
        if is_layered(hwnd) && !is_transparent(hwnd) {
            value |= ORIG_LAYERED;
            if let Some((key, alpha, flags)) = layered_attrs(hwnd) {
                value |= ORIG_ATTRS | (alpha as usize) << 8 | (flags.0 as usize) << 16;
                let key = HANDLE((key.0 as usize + 1) as *mut core::ffi::c_void);
                let _ = SetPropW(hwnd, ORIG_KEY_PROP, Some(key));
            }
        }
        let _ = SetPropW(hwnd, ORIG_PROP, Some(HANDLE(value as *mut core::ffi::c_void)));
    }
}

/// Whether the window really is transparent right now: layered, with an alpha of
/// 0 in effect. Reads the window's actual state, not what NUtils last asked for.
pub fn is_transparent(hwnd: HWND) -> bool {
    is_layered(hwnd)
        && layered_attrs(hwnd).is_some_and(|(_, alpha, flags)| flags.contains(LWA_ALPHA) && alpha == 0)
}

/// Make a window transparent, then read its state back to confirm it took.
/// Returns false when Windows refused — typically a window running with higher
/// privileges than NUtils — in which case any partial change is undone.
pub fn make_transparent(hwnd: HWND) -> bool {
    record_original(hwnd);
    set_layered(hwnd, true);
    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
    }
    if is_transparent(hwnd) {
        return true;
    }
    make_solid(hwnd);
    false
}

/// Undo [`make_transparent`], restoring the layered state the window had before.
/// A window NUtils never changed is left alone. Returns false if the window is
/// still transparent afterwards.
pub fn make_solid(hwnd: HWND) -> bool {
    let orig = unsafe { GetPropW(hwnd, ORIG_PROP).0 as usize };
    if orig == 0 {
        // Not recorded: made transparent by a NUtils version that kept no record.
        // Its original state is unknown, so just turn the alpha back up.
        if is_transparent(hwnd) {
            unsafe {
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            }
        }
        return !is_transparent(hwnd);
    }
    unsafe {
        let key = GetPropW(hwnd, ORIG_KEY_PROP).0 as usize;
        if orig & ORIG_LAYERED == 0 {
            set_layered(hwnd, false);
        } else if orig & ORIG_ATTRS != 0 {
            let key = COLORREF(key.saturating_sub(1) as u32);
            let flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(((orig >> 16) & 0xff) as u32);
            let _ = SetLayeredWindowAttributes(hwnd, key, ((orig >> 8) & 0xff) as u8, flags);
        } else {
            // An UpdateLayeredWindow app: clearing and re-setting the style is what
            // lets its own UpdateLayeredWindow calls work again.
            set_layered(hwnd, false);
            set_layered(hwnd, true);
        }
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_ERASE | RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
        );
        let _ = RemovePropW(hwnd, ORIG_PROP);
        let _ = RemovePropW(hwnd, ORIG_KEY_PROP);
    }
    !is_transparent(hwnd)
}

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

#[cfg(test)]
mod transparency {
    use super::*;
    use windows::Win32::Foundation::{POINT, SIZE};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    /// A hidden (never shown, so it never takes focus) top-level test window.
    fn test_window(ex: WINDOW_EX_STYLE) -> HWND {
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            CreateWindowExW(
                ex, w!("STATIC"), PCWSTR::null(), WS_OVERLAPPEDWINDOW,
                0, 0, 100, 100, None, None, Some(hinst.into()), None,
            )
            .unwrap()
        }
    }

    /// Draw the window the way per-pixel-alpha apps do; false if Windows refuses.
    fn update_layered(hwnd: HWND) -> bool {
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, 100, 100);
            let old = SelectObject(mem, bmp.into());
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let ok = UpdateLayeredWindow(
                hwnd, Some(screen), None, Some(&SIZE { cx: 100, cy: 100 }), Some(mem),
                Some(&POINT::default()), COLORREF(0), Some(&blend), ULW_ALPHA,
            )
            .is_ok();
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            ok
        }
    }

    #[test]
    fn plain_window_round_trips_to_not_layered() {
        let h = test_window(WINDOW_EX_STYLE(0));
        assert!(!is_transparent(h));
        assert!(make_transparent(h));
        assert!(is_transparent(h));
        assert!(make_transparent(h), "making it transparent twice is harmless");
        assert!(make_solid(h));
        assert!(!is_layered(h), "solid removes the layered style NUtils added");
        assert!(unsafe { GetPropW(h, ORIG_PROP).0.is_null() });
        unsafe { DestroyWindow(h).unwrap() };
    }

    #[test]
    fn own_alpha_and_colour_key_are_restored() {
        let h = test_window(WS_EX_LAYERED);
        let flags = LWA_ALPHA | LWA_COLORKEY;
        unsafe { SetLayeredWindowAttributes(h, COLORREF(0x00ABCDEF), 200, flags).unwrap() };
        assert!(make_transparent(h));
        assert!(make_solid(h));
        assert_eq!(layered_attrs(h), Some((COLORREF(0x00ABCDEF), 200, flags)));
        unsafe { DestroyWindow(h).unwrap() };
    }

    #[test]
    fn per_pixel_alpha_app_can_draw_again_after_solid() {
        let h = test_window(WS_EX_LAYERED);
        assert!(update_layered(h));
        assert!(make_transparent(h));
        assert!(!update_layered(h), "while transparent the app's own drawing is refused");
        assert!(make_solid(h));
        assert!(update_layered(h), "solid must give the app its own drawing back");
        unsafe { DestroyWindow(h).unwrap() };
    }

    #[test]
    fn unrecorded_window_is_left_alone() {
        let h = test_window(WINDOW_EX_STYLE(0));
        assert!(make_solid(h));
        assert!(!is_layered(h), "solid must not add a layered style to an untouched window");
        unsafe { DestroyWindow(h).unwrap() };
    }

    #[test]
    fn late_recorder_does_not_save_transparent_as_original() {
        // Another path (hook DLL / watcher) made it transparent without a record.
        let h = test_window(WS_EX_LAYERED);
        unsafe { SetLayeredWindowAttributes(h, COLORREF(0), 0, LWA_ALPHA).unwrap() };
        assert!(make_transparent(h));
        assert!(make_solid(h));
        assert!(!is_transparent(h));
        unsafe { DestroyWindow(h).unwrap() };
    }
}

#[cfg(test)]
mod focus_restore {
    use super::*;
    use std::time::Duration;

    /// The focused control of `hwnd`'s GUI thread, as "class (hwnd)".
    fn focused(hwnd: HWND) -> String {
        unsafe {
            let tid = GetWindowThreadProcessId(hwnd, None);
            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            let _ = GetGUIThreadInfo(tid, &mut info);
            let f = info.hwndFocus;
            if f.0.is_null() {
                "(nothing)".into()
            } else if f == hwnd {
                format!("the top-level window itself ({})", class_name(f))
            } else {
                format!("{} ({:?})", class_name(f), f.0)
            }
        }
    }

    /// Hides and unhides a fresh app window (Notepad by default) with NUtils' real hide/show and reports
    /// which control has keyboard focus before and after. Opens Notepad briefly.
    ///   cargo test --release focus_restore -- --ignored --nocapture
    #[test]
    #[ignore]
    fn focus_returns_after_unhide() {
        // NUTILS_FOCUS_APP / NUTILS_FOCUS_CLASS test another app instead.
        let app = std::env::var("NUTILS_FOCUS_APP").unwrap_or("notepad.exe".into());
        let class = std::env::var("NUTILS_FOCUS_CLASS").unwrap_or("Notepad".into());
        let before: Vec<HWND> = enum_top_windows();
        let mut child = std::process::Command::new(&app).spawn().unwrap();
        let mut np = None;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            np = enum_top_windows()
                .into_iter()
                .find(|&h| !before.contains(&h) && is_visible(h) && class_name(h) == class);
            if np.is_some() {
                break;
            }
        }
        let h = np.expect("a new window of the app");
        std::thread::sleep(Duration::from_millis(2000));
        eprintln!("before hide: foreground={} focus={}", foreground() == h, focused(h));

        hide(h, 0);
        std::thread::sleep(Duration::from_millis(500));
        show(h);
        std::thread::sleep(Duration::from_millis(800));
        eprintln!("after show:  foreground={} focus={}", foreground() == h, focused(h));

        unsafe {
            let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        std::thread::sleep(Duration::from_millis(500));
        let _ = child.kill();
    }
}
