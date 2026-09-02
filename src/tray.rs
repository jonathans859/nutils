//! System-tray icon and its right-click menu: a "Hidden" submenu listing every
//! hidden window (grouped into per-stack submenus when they span several stacks),
//! then Settings and Exit.

use crate::stacks::{slot_to_human, Stacks, STACK_SIZE};
use crate::window;
use std::collections::BTreeMap;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The window message the shell posts to us for tray-icon events.
pub const WM_TRAY: u32 = WM_APP + 1;

// Fixed menu command ids; hidden-window entries use SLOT_BASE + slot index.
const CMD_EXIT: usize = 1;
const CMD_SETTINGS: usize = 2;
const SLOT_BASE: usize = 1000;

pub enum MenuChoice {
    Exit,
    Settings,
    Unhide(usize),
}

pub struct Tray {
    data: NOTIFYICONDATAW,
}

fn set_tip(data: &mut NOTIFYICONDATAW, tip: &str) {
    let wide: Vec<u16> = tip.encode_utf16().take(127).collect();
    data.szTip[..wide.len()].copy_from_slice(&wide);
    data.szTip[wide.len()] = 0;
}

impl Tray {
    pub fn new(hwnd: HWND) -> Tray {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            ..Default::default()
        };
        data.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() };
        set_tip(&mut data, "NUtils");
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &data);
        }
        Tray { data }
    }

    pub fn remove(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn append(menu: HMENU, id: usize, text: &str) {
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(wide(text).as_ptr()));
    }
}

fn append_submenu(parent: HMENU, sub: HMENU, text: &str) {
    unsafe {
        let _ = AppendMenuW(parent, MF_POPUP, sub.0 as usize, PCWSTR(wide(text).as_ptr()));
    }
}

/// Build and display the tray context menu at the cursor, returning the choice.
pub fn show_menu(hwnd: HWND, stacks: &Stacks) -> Option<MenuChoice> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;

        // Group hidden windows by their stack (BTreeMap keeps stacks in order).
        let mut by_stack: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, &id) in stacks.slots.iter().enumerate() {
            if id != 0 {
                by_stack.entry(i / STACK_SIZE).or_default().push(i);
            }
        }

        // A "Hidden" submenu. If the hidden windows span more than one stack, nest
        // a submenu per stack inside it; otherwise list the windows directly.
        if !by_stack.is_empty() {
            let hidden = CreatePopupMenu().ok()?;
            let label = |i: usize| {
                format!(
                    "{}: {}",
                    slot_to_human(i % STACK_SIZE),
                    window::get_title(window::from_id(stacks.slots[i]))
                )
            };
            if by_stack.len() == 1 {
                for slots in by_stack.values() {
                    for &i in slots {
                        append(hidden, SLOT_BASE + i, &label(i));
                    }
                }
            } else {
                for (stack, slots) in &by_stack {
                    let sub = CreatePopupMenu().ok()?;
                    for &i in slots {
                        append(sub, SLOT_BASE + i, &label(i));
                    }
                    append_submenu(hidden, sub, &format!("Stack {}", stack + 1));
                }
            }
            append_submenu(menu, hidden, "Hidden");
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        }

        append(menu, CMD_SETTINGS, "Settings ...");
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append(menu, CMD_EXIT, "Exit");

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Required so the menu dismisses correctly when clicking elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            Some(0),
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu); // also destroys the attached submenus

        let id = cmd.0 as usize;
        if id == 0 {
            return None;
        }
        if id >= SLOT_BASE {
            return Some(MenuChoice::Unhide(id - SLOT_BASE));
        }
        Some(match id {
            CMD_EXIT => MenuChoice::Exit,
            CMD_SETTINGS => MenuChoice::Settings,
            _ => return None,
        })
    }
}
