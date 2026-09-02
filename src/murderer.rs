//! WinMurderer: on each timer tick, close or kill any window matching a configured
//! rule. Ported from the original's `WinMurderer.ini` polling loop.

use crate::config::{Degree, MatchKind, Rule};
use crate::window;

fn matches(rule: &Rule, hwnd: windows::Win32::Foundation::HWND) -> bool {
    let needle = rule.value.to_ascii_lowercase();
    if needle.is_empty() {
        return false;
    }
    match rule.match_kind {
        MatchKind::Title => window::get_title(hwnd).to_ascii_lowercase().contains(&needle),
        MatchKind::Class => window::class_name(hwnd).to_ascii_lowercase() == needle,
        MatchKind::Exe => window::owner_exe(hwnd).map(|e| e == needle).unwrap_or(false),
    }
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
                        window::kill_owner(hwnd);
                    }
                }
                break;
            }
        }
    }
}
