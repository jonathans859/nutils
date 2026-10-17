//! Human-readable summaries of what is currently hidden.
//!
//! The same text serves two audiences: the tray menu labels its hidden-window
//! submenu with [`brief`], and the status hotkey speaks either [`brief`] or
//! [`detailed`] depending on the `detailed_status` setting; the stack status
//! hotkey speaks [`current`]. Everything here is phrased to be read aloud,
//! so slots are announced as "position 1" using the digit the user actually
//! presses (see [`slot_to_human`]) rather than the internal index.

use crate::stacks::{slot_to_human, Stacks, STACK_SIZE};
use crate::window::{self, WinId};

/// "1 window" / "3 windows" — a count with its noun correctly pluralised.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// A speakable name for a hidden window: its title, or the owning executable when
/// the window has no title at all (some tool windows and splash screens).
pub fn window_name(id: WinId) -> String {
    let hwnd = window::from_id(id);
    let title = window::get_title(hwnd);
    if !title.trim().is_empty() {
        return title;
    }
    window::owner_exe(hwnd).unwrap_or_else(|| "untitled window".into())
}

/// One line: how many windows are hidden across how many stacks.
pub fn brief(stacks: &Stacks) -> String {
    let hidden = stacks.occupied().len();
    if hidden == 0 {
        return "No windows hidden".into();
    }
    format!(
        "{} hidden in {}",
        count(hidden, "window"),
        count(stacks.stacks_in_use(), "stack")
    )
}

/// [`brief`], plus the current stack and every hidden window with its position and
/// title, grouped by stack.
pub fn detailed(stacks: &Stacks) -> String {
    let occupied = stacks.occupied();
    if occupied.is_empty() {
        return brief(stacks);
    }

    let mut text = brief(stacks);
    text.push_str(&format!(". Current stack {}", stacks.current_stack()));

    let mut stack = None;
    for (i, id) in occupied {
        if stack != Some(i / STACK_SIZE) {
            stack = Some(i / STACK_SIZE);
            text.push_str(&format!(". Stack {}:", i / STACK_SIZE + 1));
        } else {
            text.push(';');
        }
        text.push_str(&format!(
            " position {}, {}",
            slot_to_human(i % STACK_SIZE),
            window_name(id)
        ));
    }
    text.push('.');
    text
}

/// The current stack and each window hidden in it, with its position and title.
pub fn current(stacks: &Stacks) -> String {
    let stack = stacks.current_stack();
    let positions: Vec<String> = stacks
        .occupied()
        .into_iter()
        .filter(|(i, _)| i / STACK_SIZE + 1 == stack)
        .map(|(i, id)| format!("position {}, {}", slot_to_human(i % STACK_SIZE), window_name(id)))
        .collect();
    if positions.is_empty() {
        return format!("Stack {stack}: nothing hidden.");
    }
    format!("Stack {stack}: {}.", positions.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hidden windows at the given slot indices. The ids are not real HWNDs, so
    /// `window_name` falls back to "untitled window" — which is exactly what makes
    /// the formatting assertions below deterministic.
    fn stacks_with(slots: &[usize]) -> Stacks {
        let mut s = Stacks::new();
        for (n, &i) in slots.iter().enumerate() {
            s.ensure(i);
            s.set(i, n as WinId + 1);
        }
        s
    }

    #[test]
    fn brief_reports_nothing_hidden() {
        assert_eq!(brief(&Stacks::new()), "No windows hidden");
    }

    #[test]
    fn brief_uses_singular_for_one() {
        assert_eq!(brief(&stacks_with(&[0])), "1 window hidden in 1 stack");
    }

    #[test]
    fn brief_counts_windows_and_stacks_separately() {
        // Two in stack 1, one in stack 3 — three windows, two stacks.
        let s = stacks_with(&[0, 3, 21]);
        assert_eq!(brief(&s), "3 windows hidden in 2 stacks");
    }

    #[test]
    fn detailed_groups_by_stack_and_names_positions() {
        let mut s = stacks_with(&[0, 9, 11]);
        s.shift = STACK_SIZE; // viewing stack 2
        assert_eq!(
            detailed(&s),
            "3 windows hidden in 2 stacks. Current stack 2. \
             Stack 1: position 1, untitled window; position 0, untitled window. \
             Stack 2: position 2, untitled window."
        );
    }

    #[test]
    fn current_names_only_the_current_stacks_positions() {
        let mut s = stacks_with(&[0, 9, 11]);
        assert_eq!(
            current(&s),
            "Stack 1: position 1, untitled window; position 0, untitled window."
        );
        s.shift = STACK_SIZE;
        assert_eq!(current(&s), "Stack 2: position 2, untitled window.");
        s.shift = 2 * STACK_SIZE;
        assert_eq!(current(&s), "Stack 3: nothing hidden.");
    }

    #[test]
    fn detailed_falls_back_to_brief_when_empty() {
        assert_eq!(detailed(&Stacks::new()), "No windows hidden");
    }
}
