//! winprobe — a diagnostic for the auto-hide flash.
//!
//! It installs the SAME out-of-context WinEvent hook NUtils uses and, for every
//! top-level window whose owning process matches a name you give, prints exactly
//! how that window comes into existence:
//!
//!   * whether it is created **already visible** (`ws_visible=Y` at CREATE) — the
//!     smoking gun for an unavoidable one-frame flash — or created hidden and
//!     shown later (`ws_visible=N`, then a SHOW), which we can pre-empt cleanly;
//!   * the gap between CREATE and SHOW for the same window (`Δcreate`). A gap
//!     comfortably above ~1ms means our CREATE handler sets alpha 0 *before* the
//!     window is shown → no flash. A near-zero gap (or created-visible) is the
//!     hard case.
//!
//! Usage (from a console):
//!   cargo run --release --example winprobe -- <exe-substring> [--hide]
//!
//! e.g.  cargo run --release --example winprobe -- wxdragon
//!       cargo run --release --example winprobe -- wxdragon --hide
//!
//! Run it, then open the app and trigger the dialogs/popups that flash. Each event
//! prints a line. `--hide` additionally replicates NUtils' transparency (alpha 0)
//! so you can confirm whether the flash still happens under instrumentation and
//! correlate it with the printed timing. Ctrl+C to stop, then paste the output.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

struct Probe {
    target: String,        // lowercased exe substring to match
    hide: bool,            // also apply alpha 0 like NUtils does
    origin: Instant,       // monotonic clock origin
    create_ns: HashMap<isize, u128>, // hwnd -> ns at its CREATE, to compute Δ
    pid_exe: HashMap<u32, Option<String>>,
    log: Option<File>,     // mirror every line to this file
}

/// Print a line to the console AND append it to the log file (flushed per line so
/// the file is complete even if the window is closed abruptly).
fn emit(p: &mut Probe, line: &str) {
    println!("{line}");
    if let Some(f) = p.log.as_mut() {
        let _ = writeln!(f, "{line}");
        let _ = f.flush();
    }
}

static PROBE: OnceLock<Mutex<Probe>> = OnceLock::new();

fn owner_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn owner_exe(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let res = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size);
        let _ = CloseHandle(handle);
        res.ok()?;
        let full = String::from_utf16_lossy(&buf[..size as usize]);
        let name = full.rsplit(['\\', '/']).next().unwrap_or(&full);
        Some(name.to_ascii_lowercase())
    }
}

fn class_of(hwnd: HWND) -> String {
    unsafe {
        let mut b = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut b);
        String::from_utf16_lossy(&b[..n as usize])
    }
}

fn title_of(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut b = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut b);
        String::from_utf16_lossy(&b[..n as usize])
    }
}

fn is_top_level(hwnd: HWND) -> bool {
    !hwnd.0.is_null() && unsafe { GetAncestor(hwnd, GA_ROOT) == hwnd }
}

fn set_alpha0(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered = WS_EX_LAYERED.0 as isize;
        if ex & layered == 0 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | layered);
        }
        let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), 0, LWA_ALPHA);
    }
}

unsafe extern "system" fn proc(
    _h: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if id_object != 0 || id_child != 0 {
        return; // a child control, not the window object
    }
    if event != EVENT_OBJECT_CREATE && event != EVENT_OBJECT_SHOW {
        return;
    }
    // Skip pure child *controls* (buttons, labels, edit fields) so the log isn't
    // drowned — but KEEP everything else, including owned popups and dialogs that
    // aren't "top-level" in the GA_ROOT sense. This is how we find a Qt dialog we
    // were previously filtering out. A real dialog is not a tiny child control.
    let style0 = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    let is_child = (style0 & WS_CHILD.0 as isize) != 0;
    let mut r0 = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r0);
    }
    let big = (r0.right - r0.left) >= 80 && (r0.bottom - r0.top) >= 40;
    if is_child && !big {
        return; // small child control — noise
    }
    let Some(cell) = PROBE.get() else { return };
    let Ok(mut p) = cell.lock() else { return };

    // Resolve owning exe (cached), match against target substring.
    let pid = owner_pid(hwnd);
    let exe = if let Some(v) = p.pid_exe.get(&pid) {
        v.clone()
    } else {
        let e = owner_exe(pid);
        p.pid_exe.insert(pid, e.clone());
        e
    };
    let Some(exe) = exe else { return };
    if !exe.contains(&p.target) {
        return;
    }

    let now = p.origin.elapsed().as_nanos();
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let ws_visible = (style & WS_VISIBLE.0 as isize) != 0;
    let layered = (exstyle & WS_EX_LAYERED.0 as isize) != 0;
    let visible = IsWindowVisible(hwnd).as_bool();
    let mut rect = windows::Win32::Foundation::RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;

    let id = hwnd.0 as isize;
    let (ev, delta) = match event {
        EVENT_OBJECT_CREATE => {
            p.create_ns.insert(id, now);
            ("CREATE", None)
        }
        _ => {
            let d = p.create_ns.get(&id).map(|c| (now - c) as f64 / 1_000_000.0);
            ("SHOW ", d)
        }
    };

    let top_level = is_top_level(hwnd);
    let is_child = (style & WS_CHILD.0 as isize) != 0;
    let is_popup = (style & WS_POPUP.0 as isize) != 0;
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) }
        .map(|h| h.0 as isize)
        .unwrap_or(0);

    let t_ms = now as f64 / 1_000_000.0;
    let delta_str = match delta {
        Some(d) => format!("  Δcreate={d:.2}ms"),
        None => String::new(),
    };
    let line = format!(
        "[{t_ms:9.2}ms] {ev} hwnd=0x{id:X} size={w}x{h} TL={} child={} popup={} owner=0x{owner:X} ws_visible={} onscreen={} layered={} class=\"{}\" title=\"{}\"{delta_str}",
        if top_level { "Y" } else { "N" },
        if is_child { "Y" } else { "N" },
        if is_popup { "Y" } else { "N" },
        if ws_visible { "Y" } else { "N" },
        if visible { "Y" } else { "N" },
        if layered { "Y" } else { "N" },
        class_of(hwnd),
        title_of(hwnd),
    );
    emit(&mut p, &line);

    // With --hide, try to hide any non-child window we caught (this is the very
    // fix we're testing: if widening the net lets us hide the Qt dialog, we've
    // solved Qt with no injection).
    if p.hide && !is_child {
        set_alpha0(hwnd);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let target = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let hide = args.iter().any(|a| a == "--hide");
    if target.is_empty() {
        eprintln!("usage: winprobe <exe-substring> [--hide]");
        eprintln!("  e.g. winprobe wxdragon          (observe only)");
        eprintln!("       winprobe wxdragon --hide    (also apply NUtils' alpha 0)");
        std::process::exit(2);
    }

    // Auto-log to a fixed, easy-to-find file so no shell redirection is needed.
    let log_path = {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        format!("{home}\\winprobe-log.txt")
    };
    let log = File::create(&log_path).ok();
    if log.is_none() {
        eprintln!("(warning: could not open log file {log_path}; console output only)");
    }

    PROBE
        .set(Mutex::new(Probe {
            target: target.clone(),
            hide,
            origin: Instant::now(),
            create_ns: HashMap::new(),
            pid_exe: HashMap::new(),
            log,
        }))
        .ok();

    let mut p = PROBE.get().unwrap().lock().unwrap();
    emit(&mut p, &format!(
        "winprobe: watching windows whose exe contains \"{target}\"{}.",
        if hide { " (applying alpha 0)" } else { " (observe only)" }
    ));
    emit(&mut p, &format!("Writing this log to: {log_path}"));
    emit(&mut p, "Open the app and trigger the popups that flash. Close this window when done.");
    emit(&mut p, "Key columns: ws_visible=Y at CREATE => created already-visible (hard case);");
    emit(&mut p, "             a SHOW with a large Δcreate => created hidden, we can pre-empt (no flash).");
    drop(p);

    unsafe {
        let hook = SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_SHOW,
            None,
            Some(proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !hook.is_invalid() {
            let _ = UnhookWinEvent(hook);
        }
    }
}
