//! Native Win32 dialogs, kept deliberately simple and screen-reader friendly:
//! the unhide tree and the change-title input box.
//!
//! Each dialog builds standard common controls, runs its own modal message pump
//! (with `IsDialogMessageW` for Tab/Enter navigation), and returns a plain value.
//! The caller must NOT hold any global borrow while a dialog is open, because the
//! pump dispatches other messages to the main window.

use crate::stacks::{slot_to_human, Stacks, STACK_SIZE};
use crate::window;
use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
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

// ---------------------------------------------------------------------------
// Unhide tree dialog
// ---------------------------------------------------------------------------

pub enum UnhideChoice {
    Slot(usize),
    Stack(usize),
}

thread_local! {
    static TREE_RESULT: RefCell<Option<UnhideChoice>> = const { RefCell::new(None) };
    static TREE_HWND: RefCell<HWND> = const { RefCell::new(HWND(std::ptr::null_mut())) };
}

const ID_TREE: isize = 10;
const ID_UNHIDE: isize = 11;

/// Encode a tree item's payload into its `lParam`: window slots as `slot+1`,
/// stack headers as `-(stack+1)`.
fn decode_item(lparam: isize) -> Option<UnhideChoice> {
    if lparam > 0 {
        Some(UnhideChoice::Slot((lparam - 1) as usize))
    } else if lparam < 0 {
        Some(UnhideChoice::Stack((-lparam - 1) as usize))
    } else {
        None
    }
}

unsafe fn tree_selected_param(tree: HWND) -> isize {
    let sel = SendMessageW(tree, TVM_GETNEXTITEM, Some(WPARAM(TVGN_CARET as usize)), Some(LPARAM(0)));
    if sel.0 == 0 {
        return 0;
    }
    let mut item = TVITEMW {
        mask: TVIF_PARAM | TVIF_HANDLE,
        hItem: HTREEITEM(sel.0),
        ..Default::default()
    };
    SendMessageW(
        tree,
        TVM_GETITEMW,
        Some(WPARAM(0)),
        Some(LPARAM(&mut item as *mut _ as isize)),
    );
    item.lParam.0
}

unsafe extern "system" fn tree_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as isize;
            if id == ID_UNHIDE {
                let tree = TREE_HWND.with(|t| *t.borrow());
                let param = tree_selected_param(tree);
                TREE_RESULT.with(|r| *r.borrow_mut() = decode_item(param));
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            } else if id == ID_CANCEL {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
        }
        WM_NOTIFY => {
            let hdr = &*(lp.0 as *const NMHDR);
            // Double-click / Enter on a tree item acts as Unhide.
            if hdr.idFrom == ID_TREE as usize && hdr.code == NM_DBLCLK {
                let tree = TREE_HWND.with(|t| *t.borrow());
                let param = tree_selected_param(tree);
                if param != 0 {
                    TREE_RESULT.with(|r| *r.borrow_mut() = decode_item(param));
                    let _ = DestroyWindow(hwnd);
                    return LRESULT(0);
                }
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

unsafe fn tree_insert(
    tree: HWND,
    parent: HTREEITEM,
    text: &str,
    lparam: isize,
) -> HTREEITEM {
    let wtext = wide(text);
    let mut item = TVINSERTSTRUCTW {
        hParent: parent,
        hInsertAfter: TVI_LAST,
        ..Default::default()
    };
    item.Anonymous.item.mask = TVIF_TEXT | TVIF_PARAM;
    item.Anonymous.item.pszText = windows::core::PWSTR(wtext.as_ptr() as *mut _);
    item.Anonymous.item.lParam = LPARAM(lparam);
    let res = SendMessageW(
        tree,
        TVM_INSERTITEMW,
        Some(WPARAM(0)),
        Some(LPARAM(&item as *const _ as isize)),
    );
    HTREEITEM(res.0)
}

fn init_common_controls() {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_TREEVIEW_CLASSES | ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
    };
    unsafe {
        let _ = InitCommonControlsEx(&icc);
    }
}

/// Show the tree of hidden windows and return the user's unhide choice.
pub fn unhide_dialog(stacks: &Stacks) -> Option<UnhideChoice> {
    TREE_RESULT.with(|r| *r.borrow_mut() = None);
    init_common_controls();
    register_class(w!("NUtilsTree"), Some(tree_proc));

    unsafe {
        let dlg = CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT | WS_EX_TOPMOST,
            w!("NUtilsTree"),
            PCWSTR(wide("NUtils - Unhide Window").as_ptr()),
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            300,
            460,
            None,
            None,
            Some(instance()),
            None,
        )
        .ok()?;

        create_control(
            w!("STATIC"),
            PCWSTR(wide("Select the window you wish to unhide and then click \"Unhide\"").as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0),
            15,
            10,
            260,
            40,
            dlg,
            0,
        );
        let tree = create_control(
            WC_TREEVIEW,
            PCWSTR::null(),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WS_BORDER
                | WINDOW_STYLE(TVS_HASLINES | TVS_HASBUTTONS | TVS_LINESATROOT | TVS_SHOWSELALWAYS),
            15,
            55,
            255,
            300,
            dlg,
            ID_TREE,
        );
        TREE_HWND.with(|t| *t.borrow_mut() = tree);

        let btn_unhide = create_control(
            w!("BUTTON"),
            PCWSTR(wide("Unhide").as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            15,
            370,
            110,
            34,
            dlg,
            ID_UNHIDE,
        );
        create_control(
            w!("BUTTON"),
            PCWSTR(wide("Cancel").as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            160,
            370,
            110,
            34,
            dlg,
            ID_CANCEL,
        );

        // Populate: stack headers with their non-empty windows underneath.
        let mut any = false;
        let mut last_item = HTREEITEM(0);
        let stacks_n = stacks.stack_count();
        for s in 0..stacks_n {
            if stacks.is_stack_empty(s) {
                continue;
            }
            let header = tree_insert(
                tree,
                TVI_ROOT,
                &format!("Stack {}", s + 1),
                -((s as isize) + 1),
            );
            let start = s * STACK_SIZE;
            let end = (start + STACK_SIZE).min(stacks.slots.len());
            for i in start..end {
                let id = stacks.slots[i];
                if id == 0 {
                    continue;
                }
                any = true;
                let title = window::get_title(window::from_id(id));
                let label = format!("{}.{}: {}", s + 1, slot_to_human(i % STACK_SIZE), title);
                let item = tree_insert(tree, header, &label, (i as isize) + 1);
                if stacks.last_hidden() == Some(i) {
                    last_item = item;
                }
            }
            // Expand each populated stack.
            SendMessageW(
                tree,
                TVM_EXPAND,
                Some(WPARAM(TVE_EXPAND.0 as usize)),
                Some(LPARAM(header.0)),
            );
        }

        if !any {
            let _ = EnableWindow(btn_unhide, false);
        } else if last_item.0 != 0 {
            SendMessageW(
                tree,
                TVM_SELECTITEM,
                Some(WPARAM(TVGN_CARET as usize)),
                Some(LPARAM(last_item.0)),
            );
            let _ = SetFocus(Some(tree));
        } else {
            let _ = SetFocus(Some(tree));
        }

        let _ = ShowWindow(dlg, SW_SHOW);
        pump_modal(dlg);
    }
    TREE_RESULT.with(|r| r.borrow_mut().take())
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
