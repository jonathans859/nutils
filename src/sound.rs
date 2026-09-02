//! Audio feedback: play a WAV from an optional `sounds\` pack, or fall back to
//! PC-speaker-style beeps with the same frequencies the original used.

use std::path::PathBuf;
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
use windows::Win32::System::Diagnostics::Debug::Beep;

fn sounds_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sounds")
}

pub fn beep(freq: u32, dur_ms: u32) {
    unsafe {
        let _ = Beep(freq, dur_ms);
    }
}

/// Returns true if `sounds\<name>.wav` was played.
fn play_wav(name: &str, async_: bool) -> bool {
    let path = sounds_dir().join(format!("{name}.wav"));
    if !path.exists() {
        return false;
    }
    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut flags = SND_FILENAME | SND_NODEFAULT;
    if async_ {
        flags |= SND_ASYNC;
    }
    unsafe { PlaySoundW(PCWSTR(wide.as_ptr()), None, flags).as_bool() }
}

/// Play `sounds\<name>.wav` (async) if it exists; otherwise run `fallback`.
pub fn play_or(name: &str, fallback: impl FnOnce()) {
    if !play_wav(name, true) {
        fallback();
    }
}

// Named cues mirroring the original sound set.
pub fn window_down() {
    play_or("windown", || beep(1500, 100));
}
pub fn window_up() {
    play_or("winup", || {
        beep(1500, 50);
        beep(1500, 50);
    });
}
pub fn cannot_hide() {
    play_or("HideEr", || beep(70, 100));
}
pub fn transparent() {
    beep(1760, 50);
}
pub fn solid() {
    beep(1760, 50);
    beep(1760, 50);
}
pub fn killed() {
    play_or("kill", || beep(60, 300));
}
pub fn disappeared() {
    play_or("Disappear", || beep(120, 80));
}

/// Priority-change chirp: higher pitch for higher priority (index 0..=5).
pub fn priority(index: i32) {
    let n = index - 2; // Normal (index 2) is the reference pitch
    let freq = (440.0 * 2f64.powf((n as f64 * 2.0) / 12.0)).round() as u32;
    play_or(&format!("ProcessPrioritySounds\\{index}"), move || {
        beep(freq.clamp(37, 32000), 60)
    });
}
pub fn priority_error() {
    play_or("ProcessPrioritySounds\\er", || beep(80, 130));
}

/// Announce the current stack (1-based) by playing the stack cue / beeping N times.
pub fn stack_beeps(stack: usize) {
    for _ in 0..stack {
        if !play_wav("stack", false) {
            beep(2000, 60);
        }
    }
}

/// Play a number pack for `n` (each digit as `sounds\nums\<d>.wav`), synchronously.
/// Returns false if any digit's file is missing, so the caller can fall back.
pub fn play_number(n: usize) -> bool {
    let digits: Vec<char> = n.to_string().chars().collect();
    if digits
        .iter()
        .any(|d| !sounds_dir().join(format!("nums\\{d}.wav")).exists())
    {
        return false;
    }
    for d in digits {
        play_wav(&format!("nums\\{d}"), false);
    }
    true
}
