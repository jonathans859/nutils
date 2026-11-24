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
    let pcm = tone_pcm(freq, dur_ms);
    if !pcm.is_empty() {
        play_wav_bytes(&build_wav(&pcm));
    }
}

/// Play several soft tones back to back as one sound, so they keep an even rhythm.
/// Separate `tone` calls each reopen the audio device, which adds a gap of varying
/// length between them.
pub fn tones(seq: &[(u32, u32)]) {
    let pcm: Vec<u8> = seq.iter().flat_map(|&(f, d)| tone_pcm(f, d)).collect();
    if !pcm.is_empty() {
        play_wav_bytes(&build_wav(&pcm));
    }
}

/// Render the samples of one soft tone (16-bit mono PCM at `TONE_RATE`).
fn tone_pcm(freq: u32, dur_ms: u32) -> Vec<u8> {
    let n = (TONE_RATE as u64 * dur_ms as u64 / 1000) as usize;
    if n == 0 {
        return Vec::new();
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
    pcm
}

/// Play a complete in-memory WAV synchronously.
fn play_wav_bytes(wav: &[u8]) {
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
    play_or("windown", || tones(&[(587, 60), (440, 60)]));
}
/// Unhide a window — rising, low band (coming back = pitch up).
pub fn window_up() {
    play_or("winup", || tones(&[(440, 60), (587, 60)]));
}
pub fn cannot_hide() {
    play_or("HideEr", || tone(196, 120));
}
/// Make transparent — falling, high band (going away = pitch down, as with hide).
pub fn transparent() {
    play_or("transparent", || tones(&[(880, 60), (659, 60)]));
}
/// Make solid — rising, high band (coming back = pitch up, as with unhide).
pub fn solid() {
    play_or("solid", || tones(&[(659, 60), (880, 60)]));
}
/// Transparency did not take (or was lost) — the low error tone of `cannot_hide`,
/// in the transparency band's shape (falling), so it reads as "transparency
/// failed" rather than "hide failed".
pub fn transparency_error() {
    play_or("TransparentEr", || tones(&[(220, 90), (196, 120)]));
}
/// Auto-transparent ON — the falling transparency cue plus a third tone, so an
/// app-wide toggle is audibly distinct from a single-window one.
pub fn auto_transparent() {
    play_or("autotransparent", || tones(&[(1047, 60), (880, 60), (659, 60)]));
}
/// Auto-transparent OFF — rising three-tone, the mirror of `auto_transparent`.
pub fn auto_solid() {
    play_or("autosolid", || tones(&[(659, 60), (880, 60), (1047, 60)]));
}
/// A refusal: a low, falling double tone for an action that was not allowed
/// (e.g. a per-window transparency hotkey on an auto-transparent app).
pub fn refused() {
    play_or("refused", || tones(&[(196, 70), (147, 70)]));
}
/// A neutral, non-toggle chirp (e.g. configuration reloaded).
pub fn notify() {
    play_or("notify", || tone(587, 90));
}
/// A short ascending "ready" cue played at startup.
pub fn startup() {
    play_or("ready", || tones(&[(440, 60), (587, 60), (698, 60)]));
}
pub fn killed() {
    play_or("kill", || beep(60, 300));
}
/// The kill didn't take: the app is still running.
pub fn kill_error() {
    play_or("killEr", || tones(&[(220, 90), (147, 150)]));
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

/// Silence between counter beeps. The whole sequence is rendered into one buffer
/// and played with a single `PlaySoundW` call. One call per beep left the gaps up
/// to the device-open latency of each call, which varies, so the count sounded
/// uneven.
const COUNT_GAP_MS: u32 = 70;

/// Announce the current stack (1-based) by playing the stack cue / beeping N times.
/// An `empty` stack beeps a fifth lower (`stackEmpty.wav` in a sound pack).
pub fn stack_beeps(stack: usize, empty: bool) {
    if stack == 0 {
        return;
    }
    let (name, freq) = if empty { ("stackEmpty", 440) } else { ("stack", 659) };
    let path = sounds_dir().join(format!("{name}.wav"));
    if path.exists() {
        match std::fs::read(&path).ok().and_then(|w| repeat_wav(&w, stack, COUNT_GAP_MS)) {
            Some(wav) => play_wav_bytes(&wav),
            // Not a PCM WAV we can splice: play it file by file as before.
            None => {
                for _ in 0..stack {
                    play_wav(name, false);
                }
            }
        }
        return;
    }
    let beep = tone_pcm(freq, 60);
    let gap = vec![0u8; (TONE_RATE * COUNT_GAP_MS / 1000) as usize * 2];
    let mut pcm = Vec::with_capacity((beep.len() + gap.len()) * stack);
    for i in 0..stack {
        if i > 0 {
            pcm.extend_from_slice(&gap);
        }
        pcm.extend_from_slice(&beep);
    }
    play_wav_bytes(&build_wav(&pcm));
}

/// Build a WAV that plays the PCM (or float) WAV `wav` `times` times with `gap_ms`
/// of silence in between, keeping its own format. None if it can't be parsed.
fn repeat_wav(wav: &[u8], times: usize, gap_ms: u32) -> Option<Vec<u8>> {
    let parts: Vec<&[u8]> = std::iter::repeat(wav).take(times).collect();
    concat_wavs(&parts, gap_ms)
}

/// The `fmt ` and `data` chunks of a WAV file, if it is one.
fn wav_chunks(wav: &[u8]) -> Option<(&[u8], &[u8])> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let (mut fmt, mut data) = (None, None);
    let mut pos = 12;
    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let len = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = wav.get(pos + 8..(pos + 8 + len).min(wav.len()))?;
        match id {
            b"fmt " => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        pos += 8 + len + (len & 1); // chunks are word-aligned
    }
    Some((fmt?, data?))
}

/// Join PCM (or float) WAVs into one, with `gap_ms` of silence between them. They
/// must all share one format; None if any can't be parsed or they differ.
fn concat_wavs(wavs: &[&[u8]], gap_ms: u32) -> Option<Vec<u8>> {
    let chunks: Vec<(&[u8], &[u8])> = wavs.iter().map(|w| wav_chunks(w)).collect::<Option<_>>()?;
    let fmt = chunks.first()?.0;
    if fmt.len() < 16 || chunks.iter().any(|(f, _)| *f != fmt) {
        return None;
    }
    let tag = u16::from_le_bytes([fmt[0], fmt[1]]);
    let rate = u32::from_le_bytes(fmt[4..8].try_into().ok()?);
    let align = u16::from_le_bytes([fmt[12], fmt[13]]).max(1) as usize;
    let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
    if tag != 1 && tag != 3 {
        return None; // compressed formats can't be spliced byte-wise
    }
    let silence = if tag == 1 && bits == 8 { 0x80u8 } else { 0 };
    let gap = vec![silence; (rate as u64 * gap_ms as u64 / 1000) as usize * align];
    let mut pcm = Vec::new();
    for (i, (_, data)) in chunks.iter().enumerate() {
        if i > 0 {
            pcm.extend_from_slice(&gap);
        }
        // whole frames only, so the next part starts on a frame boundary
        pcm.extend_from_slice(&data[..data.len() - data.len() % align]);
    }
    let fmt_len = fmt.len() as u32;
    let mut w = Vec::with_capacity(28 + fmt.len() + pcm.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(4 + 8 + fmt_len + (fmt_len & 1) + 8 + pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVE");
    w.extend_from_slice(b"fmt ");
    w.extend_from_slice(&fmt_len.to_le_bytes());
    w.extend_from_slice(fmt);
    if fmt_len & 1 == 1 {
        w.push(0);
    }
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(&pcm);
    Some(w)
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
    let files: Vec<Vec<u8>> = digits
        .iter()
        .filter_map(|d| std::fs::read(sounds_dir().join(format!("nums\\{d}.wav"))).ok())
        .collect();
    let parts: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
    match (files.len() == digits.len()).then(|| concat_wavs(&parts, 0)).flatten() {
        Some(wav) => play_wav_bytes(&wav),
        // Unreadable or mixed-format digits: play them one by one as before.
        None => {
            for d in digits {
                play_wav(&format!("nums\\{d}"), false);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_wav_splices_copies_with_gaps() {
        let pcm = tone_pcm(659, 60);
        let wav = build_wav(&pcm);
        let out = repeat_wav(&wav, 3, COUNT_GAP_MS).expect("parses own WAV");
        let gap = (TONE_RATE * COUNT_GAP_MS / 1000) as usize * 2;
        let data_len = pcm.len() * 3 + gap * 2;
        assert_eq!(out.len(), 44 + data_len);
        assert_eq!(&out[40..44], &(data_len as u32).to_le_bytes());
        assert_eq!(&out[4..8], &((out.len() - 8) as u32).to_le_bytes());
        assert_eq!(&out[44..44 + pcm.len()], &pcm[..]);
        assert!(out[44 + pcm.len()..44 + pcm.len() + gap].iter().all(|&b| b == 0));
    }

    #[test]
    fn concat_wavs_rejects_mixed_formats() {
        let a = build_wav(&tone_pcm(440, 60));
        let mut b = a.clone();
        b[24..28].copy_from_slice(&44_100u32.to_le_bytes()); // different sample rate
        assert!(concat_wavs(&[&a, &a], 0).is_some());
        assert!(concat_wavs(&[&a, &b], 0).is_none());
    }
}
