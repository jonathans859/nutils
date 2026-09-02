//! In-process auto-hide: inject `nutils_hook.dll` into a **designated** app so its
//! new windows are made transparent before they are ever shown (zero flash).
//!
//! Mechanism: `SetWindowsHookEx(WH_CBT, …, threadId)` scoped to the target app's
//! GUI thread. This is the *polite*, documented form of injection — the same one
//! screen readers use — not `CreateRemoteThread`/`WriteProcessMemory`. Windows
//! maps the DLL into the target and runs the hook there; we only ever target the
//! specific threads of apps the user has designated as managed.
//!
//! If the DLL can't be found or the hook can't be set (e.g. an architecture
//! mismatch, or an elevated target), we simply skip — the cross-process
//! `winevent` watcher remains as the fallback.

use std::collections::HashMap;
use std::os::windows::ffi::OsStrExt;
use std::sync::{Mutex, OnceLock};
use windows::core::{s, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, HOOKPROC,
    WH_CALLWNDPROC,
};

/// Append a diagnostic line to `%USERPROFILE%\nutils-inject-log.txt`, but only when
/// the `NUTILS_INJECT_LOG` environment variable is set — off by default so a normal
/// run writes nothing. Set it to troubleshoot injection.
fn log(msg: &str) {
    use std::io::Write;
    if std::env::var_os("NUTILS_INJECT_LOG").is_none() {
        return;
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{home}\\nutils-inject-log.txt"))
        {
            let _ = writeln!(f, "{msg}");
        }
    }
}

struct State {
    /// Whether we've attempted to load the DLL yet (load lazily, once).
    tried: bool,
    /// `HMODULE` of the loaded DLL as an integer (0 == not available).
    dll: isize,
    /// The exported `cbt_proc` from the DLL (function pointers are Send + Sync).
    proc: HOOKPROC,
    /// thread id -> (installed `HHOOK` as an integer, owning process id). A hook of
    /// 0 means injection was attempted for this thread and failed; kept so we don't
    /// retry it on every window. The pid lets us un-inject a whole app.
    hooks: HashMap<u32, (isize, u32)>,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        Mutex::new(State {
            tried: false,
            dll: 0,
            proc: None,
            hooks: HashMap::new(),
        })
    })
}

/// Wide, NUL-terminated path to `nutils_hook.dll` next to the running exe.
fn dll_path_wide() -> Option<Vec<u16>> {
    let p = std::env::current_exe()
        .ok()?
        .parent()?
        .join("nutils_hook.dll");
    Some(
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect(),
    )
}

/// Load the DLL and resolve `cbt_proc`, once. Leaves `dll`/`proc` at defaults on
/// failure so callers just skip injection.
fn load(s: &mut State) {
    s.tried = true;
    let Some(wide) = dll_path_wide() else {
        return;
    };
    unsafe {
        let Ok(hmod) = LoadLibraryW(PCWSTR(wide.as_ptr())) else {
            return;
        };
        let farproc = GetProcAddress(hmod, s!("call_wnd_proc"));
        if farproc.is_none() {
            log("load: DLL loaded but call_wnd_proc export not found");
            return;
        }
        s.dll = hmod.0 as isize;
        s.proc = std::mem::transmute::<
            windows::Win32::Foundation::FARPROC,
            HOOKPROC,
        >(farproc);
    }
}

/// Ensure the DLL is injected into the process that owns `hwnd`, so that app's
/// future windows are hidden in-process. Idempotent per GUI thread; cheap to call
/// on every managed-window sighting.
pub fn ensure(hwnd: HWND) {
    let mut pid: u32 = 0;
    let tid = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if tid == 0 {
        return;
    }
    let Ok(mut s) = state().lock() else {
        return;
    };
    if s.hooks.contains_key(&tid) {
        return; // already hooked (or already failed) for this thread
    }
    if !s.tried {
        load(&mut s);
    }
    if s.dll == 0 {
        log("ensure: DLL not loaded (nutils_hook.dll missing next to exe?)");
        return;
    }
    if s.proc.is_none() {
        return;
    }
    let hinst = HINSTANCE(s.dll as *mut core::ffi::c_void);
    let proc = s.proc;
    match unsafe { SetWindowsHookExW(WH_CALLWNDPROC, proc, Some(hinst), tid) } {
        Ok(h) => {
            log(&format!("ensure: hooked thread {tid} OK (hook=0x{:X})", h.0 as isize));
            s.hooks.insert(tid, (h.0 as isize, pid));
        }
        Err(e) => {
            log(&format!("ensure: SetWindowsHookEx on thread {tid} FAILED: {e}"));
            s.hooks.insert(tid, (0, pid)); // remember the failure; don't spam retries
        }
    }
}

/// Stop injecting into the process that owns `hwnd`: unhook every thread of that
/// process (so the helper DLL unloads and stops transparenting its new windows).
pub fn uninstall_owner(hwnd: HWND) {
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return;
    }
    let Ok(mut s) = state().lock() else {
        return;
    };
    let tids: Vec<u32> = s
        .hooks
        .iter()
        .filter(|(_, (_, p))| *p == pid)
        .map(|(t, _)| *t)
        .collect();
    for tid in tids {
        if let Some((h, _)) = s.hooks.remove(&tid) {
            if h != 0 {
                unsafe {
                    let _ = UnhookWindowsHookEx(HHOOK(h as *mut core::ffi::c_void));
                }
            }
        }
    }
}

/// Remove every hook we installed (called at shutdown).
pub fn uninstall_all() {
    let Ok(mut s) = state().lock() else {
        return;
    };
    for (_tid, (h, _pid)) in s.hooks.drain() {
        if h != 0 {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(h as *mut core::ffi::c_void));
            }
        }
    }
}
