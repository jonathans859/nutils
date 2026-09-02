//! The stack/slot model: an array of hidden-window handles grouped into stacks of
//! ten, plus a hide-order history and persistence that is invalidated across reboots.

use crate::window::WinId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

#[derive(Serialize, Deserialize)]
struct Persisted {
    /// Approximate system boot time (unix seconds); state is discarded if this
    /// no longer matches, since window handles are meaningless after a reboot.
    boot_epoch: i64,
    slots: Vec<WinId>,
}

fn boot_epoch() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let uptime_secs = (unsafe { GetTickCount64() } / 1000) as i64;
    now - uptime_secs
}

fn state_path() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("NUtils").join("state.toml");
    }
    PathBuf::from("state.toml")
}

impl Stacks {
    pub fn new() -> Self {
        Stacks {
            slots: vec![0; STACK_SIZE],
            shift: 0,
            history: Vec::new(),
        }
    }

    /// Load persisted hidden windows, discarding them if the machine has rebooted.
    pub fn load() -> Self {
        let mut s = Stacks::new();
        if let Ok(text) = std::fs::read_to_string(state_path()) {
            if let Ok(p) = toml::from_str::<Persisted>(&text) {
                if (p.boot_epoch - boot_epoch()).abs() <= 10 {
                    s.slots = p.slots;
                    if s.slots.len() < STACK_SIZE {
                        s.slots.resize(STACK_SIZE, 0);
                    }
                    for (i, &id) in s.slots.iter().enumerate() {
                        if id != 0 {
                            s.history.push(i);
                        }
                    }
                }
            }
        }
        s
    }

    pub fn save(&self) {
        let p = Persisted {
            boot_epoch: boot_epoch(),
            slots: self.slots.clone(),
        };
        if let Ok(text) = toml::to_string(&p) {
            let path = state_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, text);
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

    pub fn last_hidden(&self) -> Option<usize> {
        self.history.last().copied()
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

    pub fn stack_count(&self) -> usize {
        (self.slots.len() + STACK_SIZE - 1) / STACK_SIZE
    }

    pub fn is_stack_empty(&self, stack: usize) -> bool {
        let start = stack * STACK_SIZE;
        let end = (start + STACK_SIZE).min(self.slots.len());
        if start >= self.slots.len() {
            return true;
        }
        self.slots[start..end].iter().all(|&id| id == 0)
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
