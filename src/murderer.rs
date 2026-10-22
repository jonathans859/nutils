//! WinMurderer: on each timer tick, close or kill any window matching a configured
//! rule. Ported from the original's `WinMurderer.ini` polling loop.

use crate::config::{Degree, MatchKind, Rule};
use crate::window;

fn matches(rule: &Rule, hwnd: windows::Win32::Foundation::HWND) -> bool {
    rule.matches(|kind| match kind {
        MatchKind::Title => Some(window::get_title(hwnd)),
        MatchKind::Class => Some(window::class_name(hwnd)),
        MatchKind::Exe => window::owner_exe(hwnd),
    })
}

/// Evaluate every rule against every top-level window and act on matches.
pub fn sweep(rules: &[Rule]) {
    if rules.is_empty() {
        return;
    }
    for hwnd in window::enum_top_windows() {
        if !window::is_visible(hwnd) {
            continue;
        }
        for rule in rules {
            if matches(rule, hwnd) {
                match rule.degree {
                    Degree::Close => window::close(hwnd),
                    Degree::Kill => {
                        // Silent, and no wait: the next sweep tries again.
                        window::kill_owner(hwnd, 0);
                    }
                }
                break;
            }
        }
    }
}
