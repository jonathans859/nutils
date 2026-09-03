//! Native Win32 dialogs, kept deliberately simple and screen-reader friendly:
//! the change-title input box and a message box.
//!
//! Each dialog builds standard common controls, runs its own modal message pump
//! (with `IsDialogMessageW` for Tab/Enter navigation), and returns a plain value.
//! The caller must NOT hold any global borrow while a dialog is open, because the
//! pump dispatches other messages to the main window.

use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::EM_SETSEL;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn pump_modal(dlg: HWND) {
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        if !IsWindow(Some(dlg)).as_bool() {
            break;
        }
        if IsDialogMessageW(dlg, &msg).as_bool() {
            continue;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None).unwrap_or_default().into() }
}

fn create_control(
    class: PCWSTR,
    text: PCWSTR,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    parent: HWND,
    id: isize,
) -> HWND {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            text,
            style,
            x,
            y,
            w,
            h,
            Some(parent),
            Some(HMENU(id as *mut _)),
            Some(instance()),
            None,
        )
        .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Change-title input box
// ---------------------------------------------------------------------------

thread_local! {
    static INPUT_RESULT: RefCell<Option<String>> = const { RefCell::new(None) };
    static INPUT_EDIT: RefCell<HWND> = const { RefCell::new(HWND(std::ptr::null_mut())) };
}

const ID_OK: isize = 1;
const ID_CANCEL: isize = 2;
const ID_EDIT: isize = 3;

unsafe extern "system" fn input_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as isize;
            if id == ID_OK {
                let edit = INPUT_EDIT.with(|e| *e.borrow());
                let len = GetWindowTextLengthW(edit);
                let mut buf = vec![0u16; len as usize + 1];
                let n = GetWindowTextW(edit, &mut buf);
                let text = String::from_utf16_lossy(&buf[..n as usize]);
                INPUT_RESULT.with(|r| *r.borrow_mut() = Some(text));
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            } else if id == ID_CANCEL {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

fn register_class(name: PCWSTR, proc: WNDPROC) {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: proc,
            hInstance: instance().into(),
            lpszClassName: name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            // Classic trick: system-color index + 1 used directly as the brush.
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(16 as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc);
    }
}

/// Prompt for a new window title, pre-filled with `initial`.
pub fn input_box(title: &str, prompt: &str, initial: &str) -> Option<String> {
    INPUT_RESULT.with(|r| *r.borrow_mut() = None);
    register_class(w!("NUtilsInput"), Some(input_proc));
    unsafe {
        let dlg = CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT | WS_EX_TOPMOST,
            w!("NUtilsInput"),
            PCWSTR(wide(title).as_ptr()),
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            420,
            170,
            None,
            None,
            Some(instance()),
            None,
        )
        .ok()?;

        create_control(
            w!("STATIC"),
            PCWSTR(wide(prompt).as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0),
            15,
            15,
            390,
            40,
            dlg,
            0,
        );
        let edit = create_control(
            w!("EDIT"),
            PCWSTR(wide(initial).as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            15,
            60,
            390,
            26,
            dlg,
            ID_EDIT,
        );
        INPUT_EDIT.with(|e| *e.borrow_mut() = edit);
        create_control(
            w!("BUTTON"),
            w!("OK"),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            215,
            100,
            90,
            32,
            dlg,
            ID_OK,
        );
        create_control(
            w!("BUTTON"),
            PCWSTR(wide("Cancel").as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            315,
            100,
            90,
            32,
            dlg,
            ID_CANCEL,
        );

        // Select all text and focus the edit.
        let _ = SetFocus(Some(edit));
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        let _ = ShowWindow(dlg, SW_SHOW);
        pump_modal(dlg);
    }
    INPUT_RESULT.with(|r| r.borrow_mut().take())
}


/// Simple modal message box wrapper (used for About / update results).
pub fn message_box(title: &str, text: &str, icon: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(wide(text).as_ptr()),
            PCWSTR(wide(title).as_ptr()),
            icon,
        )
    }
}
