//! The auto-transparency watcher (the new feature).
//!
//! A system-wide `SetWinEventHook` with `WINEVENT_OUTOFCONTEXT` — which delivers
//! callbacks WITHOUT injecting any code into other processes — notices when a
//! window belonging to a "managed app" appears and immediately makes it
//! transparent.
//!
//! Two things keep the delay minimal:
//!   * We tag the window at `EVENT_OBJECT_CREATE` (fired *before* it is shown), so
//!     a window that is created hidden and then shown is made transparent before
//!     its first paint — no flash.
//!   * The hook lives on its **own dedicated thread** whose only job is to pump
//!     these events, so nothing the main thread does (hotkeys, the tray menu, the
//!     1-second sweep) can ever delay a callback.
//!
//! Measured OUTOFCONTEXT delivery latency on this machine is ~1ms (min 0.9 /
//! median 1.1 / max 1.4ms over 18 samples — see the `hook_latency` test in
//! `window.rs`). That is a small fraction of a 60Hz compositor frame (16.7ms), so
//! for a normally-built dialog — created hidden, shown, painted at the next vsync
//! — we set alpha 0 well before the first frame is drawn: no flash.
//!
//! The one case that can still flash for a single frame is an app that *force-
//! paints synchronously* at show time (`ShowWindow` immediately followed by
//! `UpdateWindow`, microseconds apart): that paint lands before our ~1ms callback.
//! Beating an in-process synchronous paint would require running inside that
//! process — i.e. injecting a DLL into every program — which we deliberately do
//! NOT do (it is exactly what makes such tools heavy, permission-hungry, and prone
//! to antivirus flags). DWM cloaking, which would hide without a repaint, is
//! rejected cross-process (E_ACCESSDENIED). So this is as tight as it gets from
//! our own process; the residual is at most one frame, only for force-painting apps.

use crate::config::MatchKind;
use crate::state::ManagedApp;
use crate::inject;
use crate::window;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage, EVENT_OBJECT_CREATE,
    EVENT_OBJECT_SHOW, MSG, WINEVENT_OUTOFCONTEXT, WM_QUIT,
};

#[derive(Clone)]
struct Matcher {
    kind: MatchKind,
    value: String, // lowercased
}

struct State {
    matchers: Vec<Matcher>,
    has_exe: bool,
    /// pid -> owning exe name, so we don't `OpenProcess` on every CREATE event.
    pid_exe: HashMap<u32, Option<String>>,
}

// Shared with the hook thread; both the main thread (set_managed) and the hook
// thread (the callback) touch it, so it must be a real global, not thread-local.
static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        Mutex::new(State {
            matchers: Vec::new(),
            has_exe: false,
            pid_exe: HashMap::new(),
        })
    })
}

/// Replace the set of managed-app matchers the hook consults.
pub fn set_managed(apps: &[ManagedApp]) {
    let matchers: Vec<Matcher> = apps
        .iter()
        .map(|a| Matcher {
            kind: a.match_kind,
            value: a.value.to_ascii_lowercase(),
        })
        .collect();
    if let Ok(mut s) = state().lock() {
        s.has_exe = matchers.iter().any(|m| m.kind == MatchKind::Exe);
        s.matchers = matchers;
        s.pid_exe.clear();
    }
}

fn is_managed(hwnd: HWND) -> bool {
    let Ok(mut s) = state().lock() else {
        return false;
    };
    if s.matchers.is_empty() {
        return false;
    }
    let exe = if s.has_exe {
        let pid = window::owner_pid(hwnd);
        if pid == 0 {
            None
        } else if let Some(v) = s.pid_exe.get(&pid) {
            v.clone()
        } else {
            let exe = window::owner_exe(hwnd);
            if s.pid_exe.len() > 512 {
                s.pid_exe.clear();
            }
            s.pid_exe.insert(pid, exe.clone());
            exe
        }
    } else {
        None
    };
    s.matchers.iter().any(|mt| match mt.kind {
        MatchKind::Exe => exe.as_deref() == Some(mt.value.as_str()),
        MatchKind::Title => window::get_title(hwnd)
            .to_ascii_lowercase()
            .contains(&mt.value),
        MatchKind::Class => window::class_name(hwnd).to_ascii_lowercase() == mt.value,
    })
}

unsafe extern "system" fn proc(
    _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // The window object itself (not a child control) being created or shown.
    if id_object != 0 || id_child != 0 {
        return;
    }
    if event != EVENT_OBJECT_CREATE && event != EVENT_OBJECT_SHOW {
        return; // the range also delivers EVENT_OBJECT_DESTROY, which we ignore
    }
    if hwnd.0.is_null() || !window::is_top_level(hwnd) {
        return;
    }
    if is_managed(hwnd) {
        // Make it transparent now, while it may still be hidden. This is the
        // cross-process fallback and may still let a single frame through for an
        // app that force-paints on show.
        // A refusal is reported by the timer check in main.rs, not here.
        window::make_transparent(hwnd);
        // Ensure the in-process helper is injected into this app, so its *next*
        // windows are born transparent with no flash at all.
        inject::ensure(hwnd);
    }
}

/// Whether `hwnd` belongs to a currently-managed app (used at startup to inject
/// into already-running managed apps).
pub fn is_managed_window(hwnd: HWND) -> bool {
    is_managed(hwnd)
}

/// A running hook, owning its dedicated thread. Keep it alive; drop via [`uninstall`].
pub struct HookThread {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

/// Start the hook on its own thread.
pub fn install() -> HookThread {
    let (tx, rx) = mpsc::channel::<u32>();
    let join = std::thread::spawn(move || unsafe {
        let hook = SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_SHOW,
            None,
            Some(proc),
            0, // all processes
            0, // all threads
            WINEVENT_OUTOFCONTEXT,
        );
        let _ = tx.send(windows::Win32::System::Threading::GetCurrentThreadId());
        // This thread does nothing but deliver hook callbacks as fast as possible.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !hook.is_invalid() {
            let _ = UnhookWinEvent(hook);
        }
    });
    let thread_id = rx.recv().unwrap_or(0);
    HookThread {
        thread_id,
        join: Some(join),
    }
}

/// Stop the hook thread and unhook.
pub fn uninstall(mut h: HookThread) {
    if h.thread_id != 0 {
        unsafe {
            let _ = PostThreadMessageW(h.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
    if let Some(join) = h.join.take() {
        let _ = join.join();
    }
}
