//! Audio feedback: play a WAV from an optional `sounds\` pack, or fall back to
//! PC-speaker-style beeps with the same frequencies the original used.

use std::path::PathBuf;
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_MEMORY, SND_NODEFAULT};
use windows::Win32::System::Diagnostics::Debug::Beep;

/// Peak amplitude of generated tones (0.0–1.0). Kept low so cues are soft.
const TONE_AMP: f32 = 0.20;
const TONE_RATE: u32 = 22_050;

fn sounds_dir() -> PathBuf {
    crate::config::exe_dir().join("sounds")
}

pub fn beep(freq: u32, dur_ms: u32) {
    unsafe {
        let _ = Beep(freq, dur_ms);
    }
}

/// Play a soft sine tone: a `freq` Hz sine at low amplitude with a short
/// raised-cosine fade in/out (so there is no hard click), rendered to an in-memory
/// WAV and played synchronously. Much gentler than the square-wave `Beep`.
pub fn tone(freq: u32, dur_ms: u32) {
    let n = (TONE_RATE as u64 * dur_ms as u64 / 1000) as usize;
    if n == 0 {
        return;
    }
    let edge = ((TONE_RATE as usize * 6 / 1000).max(1)).min(n / 2); // ~6ms fade
    let step = 2.0 * std::f32::consts::PI * freq as f32 / TONE_RATE as f32;
    let mut pcm: Vec<u8> = Vec::with_capacity(n * 2);
    for i in 0..n {
        let env = if i < edge {
            0.5 - 0.5 * (std::f32::consts::PI * i as f32 / edge as f32).cos()
        } else if i >= n - edge {
            0.5 - 0.5 * (std::f32::consts::PI * (n - i) as f32 / edge as f32).cos()
        } else {
            1.0
        };
        let s = (step * i as f32).sin() * TONE_AMP * env;
        let v = (s * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    let wav = build_wav(&pcm);
    unsafe {
        // With SND_MEMORY the first arg is a pointer to the WAV bytes; sync play
        // (no SND_ASYNC) keeps `wav` alive for the duration of playback.
        let _ = PlaySoundW(
            PCWSTR(wav.as_ptr() as *const u16),
            None,
            SND_MEMORY | SND_NODEFAULT,
        );
    }
}

/// Wrap 16-bit mono PCM samples in a minimal RIFF/WAVE container.
fn build_wav(pcm: &[u8]) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let byte_rate = TONE_RATE * 2; // mono, 2 bytes/sample
    let mut w = Vec::with_capacity(44 + pcm.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVE");
    w.extend_from_slice(b"fmt ");
    w.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    w.extend_from_slice(&1u16.to_le_bytes()); // format = PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // channels = mono
    w.extend_from_slice(&TONE_RATE.to_le_bytes());
    w.extend_from_slice(&byte_rate.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes()); // block align
    w.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    w.extend_from_slice(pcm);
    w
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

// Toggle cues use a consistent metaphor: the window going away (hidden, made
// transparent) falls in pitch, and the window coming back (shown, made solid)
// rises. Hiding lives in a lower pitch band and transparency in a higher one, so
// the two kinds of toggle are easy to tell apart by ear. The app-wide
// (auto-transparent) toggles use the same directions with a third tone, so "this
// app" and "this window" are distinguishable by tone count alone.

/// Hide a window — falling, low band (going away = pitch down).
pub fn window_down() {
    play_or("windown", || {
        tone(587, 60);
        tone(440, 60);
    });
}
/// Unhide a window — rising, low band (coming back = pitch up).
pub fn window_up() {
    play_or("winup", || {
        tone(440, 60);
        tone(587, 60);
    });
}
pub fn cannot_hide() {
    play_or("HideEr", || tone(196, 120));
}
/// Make transparent — falling, high band (going away = pitch down, as with hide).
pub fn transparent() {
    play_or("transparent", || {
        tone(880, 60);
        tone(659, 60);
    });
}
/// Make solid — rising, high band (coming back = pitch up, as with unhide).
pub fn solid() {
    play_or("solid", || {
        tone(659, 60);
        tone(880, 60);
    });
}
/// Auto-transparent ON — the falling transparency cue plus a third tone, so an
/// app-wide toggle is audibly distinct from a single-window one.
pub fn auto_transparent() {
    play_or("autotransparent", || {
        tone(1047, 60);
        tone(880, 60);
        tone(659, 60);
    });
}
/// Auto-transparent OFF — rising three-tone, the mirror of `auto_transparent`.
pub fn auto_solid() {
    play_or("autosolid", || {
        tone(659, 60);
        tone(880, 60);
        tone(1047, 60);
    });
}
/// A refusal: a low, falling double tone for an action that was not allowed
/// (e.g. a per-window transparency hotkey on an auto-transparent app).
pub fn refused() {
    play_or("refused", || {
        tone(196, 70);
        tone(147, 70);
    });
}
/// A neutral, non-toggle chirp (e.g. configuration reloaded).
pub fn notify() {
    play_or("notify", || tone(587, 90));
}
/// A short ascending "ready" cue played at startup.
pub fn startup() {
    play_or("ready", || {
        tone(440, 60);
        tone(587, 60);
        tone(698, 60);
    });
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
            tone(659, 60);
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
