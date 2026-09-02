//! `nutils_hook.dll` — the in-process arm of NUtils' auto-hide.
//!
//! NUtils loads this DLL into a **designated** app (and no other) with
//! `SetWindowsHookEx(WH_CALLWNDPROC, …, threadId)` — the same sanctioned,
//! documented mechanism screen readers use. Nothing is written into another
//! process's memory and no remote thread is created; Windows itself maps this DLL
//! into the target and calls [`call_wnd_proc`] there.
//!
//! Its single job: catch each window **just before it is shown**
//! (`WM_WINDOWPOSCHANGING`, also `WM_CREATE`/`WM_SHOWWINDOW`) and make it
//! transparent (`WS_EX_LAYERED`, alpha 0) *synchronously, in the app's own thread,
//! before the first paint*. Doing it at the show moment — rather than at creation —
//! is what makes it flash-free even for toolkits (Qt, wx) that build the window
//! first and only style/show it later: our alpha is applied last, right before the
//! pixels would appear, so nothing is ever painted opaque.
//!
//! The window stays a real, fully screen-reader-readable window; it is only
//! visually transparent. Child *controls* (buttons, edits, …) are skipped.

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, CWPSTRUCT,
    GWL_EXSTYLE, GWL_STYLE, HC_ACTION, LWA_ALPHA, WM_CREATE, WM_SHOWWINDOW, WM_WINDOWPOSCHANGING,
    WS_CHILD, WS_EX_LAYERED,
};

/// Make a top-level window transparent (alpha 0). No-op for child controls.
///
/// # Safety
/// `hwnd` must be a valid window handle (it comes straight from the OS).
unsafe fn hide_if_window(hwnd: HWND) {
    if hwnd.0.is_null() {
        return;
    }
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    if (style & WS_CHILD.0 as isize) != 0 {
        return; // a child control (button/edit/…), not a window we hide
    }
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let layered = WS_EX_LAYERED.0 as isize;
    if ex & layered == 0 {
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | layered);
    }
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
}

/// The WH_CALLWNDPROC hook procedure, run inside the target app's process. It sees
/// messages *before* the target's window procedure does, so hiding here on
/// `WM_WINDOWPOSCHANGING` happens before the window is actually shown/painted.
///
/// # Safety
/// Called by the OS as a Windows hook; all pointers come from the OS.
#[no_mangle]
pub unsafe extern "system" fn call_wnd_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let cwp = lparam.0 as *const CWPSTRUCT;
        if !cwp.is_null() {
            match (*cwp).message {
                WM_WINDOWPOSCHANGING | WM_CREATE | WM_SHOWWINDOW => hide_if_window((*cwp).hwnd),
                _ => {}
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
