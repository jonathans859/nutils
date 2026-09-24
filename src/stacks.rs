//! The stack/slot model: an array of hidden-window handles grouped into stacks of
//! ten, plus a hide-order history and persistence that is invalidated across reboots.

use crate::state::Hidden;
use crate::window::{self, to_id, WinId};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::SystemInformation::GetTickCount64;

pub const STACK_SIZE: usize = 10;

/// The runtime state: `slots[i] == 0` means slot `i` is empty. `shift` is the
/// current stack's base offset (a multiple of [`STACK_SIZE`]).
pub struct Stacks {
    pub slots: Vec<WinId>,
    pub shift: usize,
    /// Slots in the order they were hidden; used to focus the last-hidden window.
    pub history: Vec<usize>,
}

fn boot_epoch() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let uptime_secs = (unsafe { GetTickCount64() } / 1000) as i64;
    now - uptime_secs
}

impl Stacks {
    pub fn new() -> Self {
        Stacks {
            slots: vec![0; STACK_SIZE],
            shift: 0,
            history: Vec::new(),
        }
    }

    /// The hidden windows saved in `state.toml`, discarded if the machine has
    /// rebooted since.
    pub fn from_saved(saved: Option<&Hidden>) -> Self {
        let mut s = Stacks::new();
        if let Some(p) = saved.filter(|p| (p.boot_epoch - boot_epoch()).abs() <= 10) {
            s.slots = p.slots.clone();
            if s.slots.len() < STACK_SIZE {
                s.slots.resize(STACK_SIZE, 0);
            }
            for (i, &id) in s.slots.iter().enumerate() {
                if id != 0 {
                    s.history.push(i);
                }
            }
        }
        s
    }

    /// The hidden windows, for saving in `state.toml`.
    pub fn to_saved(&self) -> Hidden {
        Hidden {
            boot_epoch: boot_epoch(),
            slots: self.slots.clone(),
        }
    }

    pub fn get(&self, slot: usize) -> WinId {
        self.slots.get(slot).copied().unwrap_or(0)
    }

    /// Grow the slot array so `slot` is addressable.
    pub fn ensure(&mut self, slot: usize) {
        if slot >= self.slots.len() {
            self.slots.resize(slot + 1, 0);
        }
    }

    pub fn set(&mut self, slot: usize, id: WinId) {
        self.ensure(slot);
        self.slots[slot] = id;
        self.history_add(slot);
    }

    pub fn clear(&mut self, slot: usize) {
        if let Some(s) = self.slots.get_mut(slot) {
            *s = 0;
        }
        self.history_del(slot);
    }

    /// First empty slot at or after `start`, or `None` if the array is full.
    pub fn first_free(&self, start: usize) -> Option<usize> {
        (start..self.slots.len()).find(|&i| self.slots[i] == 0)
    }

    /// First empty slot from `start`, extending the array if necessary (never fails).
    pub fn first_free_or_grow(&mut self, start: usize) -> usize {
        if let Some(i) = self.first_free(start) {
            return i;
        }
        let i = self.slots.len();
        self.slots.push(0);
        i
    }

    pub fn history_add(&mut self, slot: usize) {
        self.history.retain(|&s| s != slot);
        self.history.push(slot);
    }

    pub fn history_del(&mut self, slot: usize) {
        self.history.retain(|&s| s != slot);
    }

    /// Every occupied slot as `(slot index, window id)`, in slot order.
    pub fn occupied(&self) -> Vec<(usize, WinId)> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, &id)| id != 0)
            .map(|(i, &id)| (i, id))
            .collect()
    }

    /// How many distinct stacks currently hold at least one hidden window.
    pub fn stacks_in_use(&self) -> usize {
        let mut seen: Vec<usize> = Vec::new();
        for (i, _) in self.occupied() {
            let stack = i / STACK_SIZE;
            if !seen.contains(&stack) {
                seen.push(stack);
            }
        }
        seen.len()
    }

    /// The 1-based number of the stack the digit hotkeys currently address.
    pub fn current_stack(&self) -> usize {
        self.shift / STACK_SIZE + 1
    }

    /// Take back windows NUtils hid but has lost track of (`state.toml` deleted
    /// or reset): a hidden window still carrying the mark [`window::hide`] puts
    /// on it goes back into its old slot, or the first free one if that is taken.
    /// A marked window that is visible again was shown some other way, so its
    /// stale mark is removed instead. Returns how many windows were recovered.
    pub fn recover(&mut self, windows: &[HWND]) -> usize {
        let mut recovered = 0;
        for &h in windows {
            let Some(slot) = window::hidden_slot(h) else {
                continue;
            };
            if window::is_visible(h) {
                window::clear_hidden_mark(h);
                continue;
            }
            if self.slots.contains(&to_id(h)) {
                continue;
            }
            // A wild slot number (a window some other tool marked) isn't trusted
            // to size the slot array.
            let slot = if slot < 100 * STACK_SIZE && self.get(slot) == 0 {
                slot
            } else {
                self.first_free_or_grow(0)
            };
            self.set(slot, to_id(h));
            recovered += 1;
        }
        recovered
    }

    /// Trim trailing empty slots back down to a multiple of [`STACK_SIZE`]
    /// (keeping at least one stack).
    pub fn prune(&mut self) {
        let last = self
            .slots
            .iter()
            .rposition(|&id| id != 0)
            .map(|i| i + 1)
            .unwrap_or(0);
        let new_len = ((last + STACK_SIZE - 1) / STACK_SIZE).max(1) * STACK_SIZE;
        self.slots.resize(new_len, 0);
    }

}

/// Slot index (0-based) to the human 1..10 label the UI shows.
pub fn slot_to_human(slot_in_stack: usize) -> usize {
    if slot_in_stack == STACK_SIZE - 1 {
        0 // slot 9 is labelled "0" on the number row
    } else {
        slot_in_stack + 1
    }
}

/// Digit typed on the number row (0..9, where 0 means the tenth slot) to a slot index.
pub fn human_to_slot(digit: u32) -> usize {
    if digit == 0 {
        STACK_SIZE - 1
    } else {
        (digit - 1) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, ShowWindow, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE,
        WS_OVERLAPPEDWINDOW,
    };

    /// A hidden (never shown) top-level test window.
    fn test_window() -> HWND {
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            CreateWindowExW(
                WINDOW_EX_STYLE(0), w!("STATIC"), PCWSTR::null(), WS_OVERLAPPEDWINDOW,
                0, 0, 100, 100, None, None, Some(hinst.into()), None,
            )
            .unwrap()
        }
    }

    #[test]
    fn marked_windows_are_recovered_into_their_slots() {
        let (a, b, c) = (test_window(), test_window(), test_window());
        window::hide(a, 3);
        window::hide(b, 3); // same slot: goes to the first free one instead
        window::hide(c, 12);
        let mut stacks = Stacks::new(); // as if state.toml was lost
        assert_eq!(stacks.recover(&[a, b, c]), 3);
        assert_eq!(stacks.get(3), to_id(a));
        assert_eq!(stacks.get(0), to_id(b));
        assert_eq!(stacks.get(12), to_id(c));
        assert_eq!(stacks.recover(&[a, b, c]), 0, "already known: not recovered twice");
        for h in [a, b, c] {
            unsafe { DestroyWindow(h).unwrap() };
        }
    }

    #[test]
    fn shown_or_unmarked_windows_are_left_alone() {
        let (marked, plain) = (test_window(), test_window());
        window::hide(marked, 2);
        unsafe {
            let _ = ShowWindow(marked, SW_SHOWNOACTIVATE); // shown some other way
        }
        let mut stacks = Stacks::new();
        assert_eq!(stacks.recover(&[marked, plain]), 0);
        assert_eq!(window::hidden_slot(marked), None, "stale mark removed");
        for h in [marked, plain] {
            unsafe { DestroyWindow(h).unwrap() };
        }
    }
}
