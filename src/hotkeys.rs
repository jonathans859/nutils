//! Parse the legacy NUtils hotkey syntax into modifier flags + a virtual-key code,
//! and manage `RegisterHotKey` registrations.
//!
//! Syntax: a prefix of modifier chars (`^`=Ctrl, `+`=Shift, `#`=Win, `!`=Alt)
//! followed by a single key. The key is either a literal character (`t`, `\`, digit)
//! or a braced name (`{f4}`, `{esc}`, `{f11}`).

use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// A parsed hotkey: modifier flags for `RegisterHotKey` plus the virtual-key code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub mods: HOT_KEY_MODIFIERS,
    pub vk: u32,
}

/// Split a binding string into its modifier prefix and the remaining key token.
fn split_mods(spec: &str) -> (HOT_KEY_MODIFIERS, &str) {
    let mut mods = MOD_NOREPEAT;
    let mut rest = spec;
    loop {
        let mut chars = rest.chars();
        match chars.next() {
            Some('^') => mods |= MOD_CONTROL,
            Some('+') => mods |= MOD_SHIFT,
            Some('#') => mods |= MOD_WIN,
            Some('!') => mods |= MOD_ALT,
            _ => break,
        }
        rest = chars.as_str();
    }
    (mods, rest)
}

/// Map a key token (already stripped of modifiers) to a virtual-key code.
fn key_to_vk(key: &str) -> Option<u32> {
    if key.is_empty() {
        return None;
    }
    // Braced named keys: {f1}..{f24}, {esc}, {enter}, {tab}, {space}, ...
    if key.starts_with('{') && key.ends_with('}') {
        let name = key[1..key.len() - 1].to_ascii_lowercase();
        if let Some(n) = name.strip_prefix('f') {
            if let Ok(num) = n.parse::<u32>() {
                if (1..=24).contains(&num) {
                    return Some(VK_F1.0 as u32 + (num - 1));
                }
            }
        }
        return Some(match name.as_str() {
            "esc" | "escape" => VK_ESCAPE.0 as u32,
            "enter" | "return" => VK_RETURN.0 as u32,
            "tab" => VK_TAB.0 as u32,
            "bs" | "back" | "backspace" => VK_BACK.0 as u32,
            "space" => VK_SPACE.0 as u32,
            "del" | "delete" => VK_DELETE.0 as u32,
            "ins" | "insert" => VK_INSERT.0 as u32,
            "home" => VK_HOME.0 as u32,
            "end" => VK_END.0 as u32,
            "pgup" => VK_PRIOR.0 as u32,
            "pgdn" => VK_NEXT.0 as u32,
            "up" => VK_UP.0 as u32,
            "down" => VK_DOWN.0 as u32,
            "left" => VK_LEFT.0 as u32,
            "right" => VK_RIGHT.0 as u32,
            "add" => VK_ADD.0 as u32,
            "subtract" => VK_SUBTRACT.0 as u32,
            "multiply" => VK_MULTIPLY.0 as u32,
            "divide" => VK_DIVIDE.0 as u32,
            "decimal" => VK_DECIMAL.0 as u32,
            _ => return None,
        });
    }

    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None; // more than one char and not braced
    }
    Some(char_to_vk(c))
}

/// Virtual-key code for a single literal character, including the OEM punctuation
/// keys the original bindings use (`\`, `/`, `-`, `=`).
fn char_to_vk(c: char) -> u32 {
    match c {
        '0'..='9' => c as u32,                        // VK_0..VK_9 == '0'..'9'
        'a'..='z' => c.to_ascii_uppercase() as u32,   // VK_A..VK_Z == 'A'..'Z'
        'A'..='Z' => c as u32,
        '\\' => VK_OEM_5.0 as u32,
        '/' => VK_OEM_2.0 as u32,
        '-' => VK_OEM_MINUS.0 as u32,
        '=' => VK_OEM_PLUS.0 as u32,
        ';' => VK_OEM_1.0 as u32,
        '\'' => VK_OEM_7.0 as u32,
        ',' => VK_OEM_COMMA.0 as u32,
        '.' => VK_OEM_PERIOD.0 as u32,
        '`' => VK_OEM_3.0 as u32,
        '[' => VK_OEM_4.0 as u32,
        ']' => VK_OEM_6.0 as u32,
        other => other.to_ascii_uppercase() as u32,
    }
}

/// Parse a full binding such as `"#+t"` or `"^+1"` into a [`Hotkey`].
pub fn parse(spec: &str) -> Option<Hotkey> {
    let (mods, key) = split_mods(spec);
    let vk = key_to_vk(key)?;
    Some(Hotkey { mods, vk })
}

/// Parse a "bass" prefix + a suffix key, e.g. bass `"^+"` and suffix `"1"` or `"{f4}"`.
pub fn parse_with_suffix(bass: &str, suffix: &str) -> Option<Hotkey> {
    parse(&format!("{bass}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_win_shift_t() {
        let hk = parse("#+t").unwrap();
        assert!(hk.mods & MOD_WIN == MOD_WIN);
        assert!(hk.mods & MOD_SHIFT == MOD_SHIFT);
        assert_eq!(hk.vk, b'T' as u32);
    }

    #[test]
    fn parses_win_f4() {
        let hk = parse("#{f4}").unwrap();
        assert!(hk.mods & MOD_WIN == MOD_WIN);
        assert_eq!(hk.vk, VK_F4.0 as u32);
    }

    #[test]
    fn parses_ctrl_shift_digit() {
        let hk = parse_with_suffix("^+", "1").unwrap();
        assert_eq!(hk.vk, b'1' as u32);
    }

    #[test]
    fn parses_backslash() {
        let hk = parse("#+\\").unwrap();
        assert_eq!(hk.vk, VK_OEM_5.0 as u32);
    }
}
