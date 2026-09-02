//! Action feedback: PC-speaker beeps, spoken text, or both.
//!
//! Spoken text speaks through the **active screen reader** when one is running —
//! NVDA via its controller-client DLL — and falls back to Windows SAPI only when
//! no screen reader is present, so screen-reader users hear their own voice rather
//! than a second SAPI voice talking over it. (Build with `--features speech` to
//! use Prism instead, which additionally supports JAWS/ZoomText but needs the
//! Visual Studio ATL component to build.) When no speaker at all is available,
//! `Text` mode falls back to beeps so feedback is never silent.

use crate::config::FeedbackMode;
use crate::sound;

pub struct Feedback {
    mode: FeedbackMode,
    speaker: Option<Speaker>,
}

impl Feedback {
    pub fn new(mode: FeedbackMode) -> Self {
        Feedback {
            mode,
            speaker: if mode.text() { Speaker::new() } else { None },
        }
    }

    pub fn set_mode(&mut self, mode: FeedbackMode) {
        self.mode = mode;
        if mode.text() && self.speaker.is_none() {
            self.speaker = Speaker::new();
        }
    }

    fn speech_available(&self) -> bool {
        self.speaker.is_some()
    }

    /// Beep when in a beep mode, or when text was requested but no speaker is
    /// available (so the user is never left with silent feedback).
    fn should_beep(&self) -> bool {
        self.mode.beeps() || (self.mode.text() && !self.speech_available())
    }

    fn speak(&mut self, text: &str) {
        if self.mode.text() {
            if let Some(s) = &mut self.speaker {
                s.speak(text);
            }
        }
    }

    // --- named cues (mirror the `sound` module) ---------------------------

    pub fn window_down(&mut self) {
        if self.should_beep() {
            sound::window_down();
        }
        self.speak("Hidden");
    }
    pub fn window_up(&mut self) {
        if self.should_beep() {
            sound::window_up();
        }
        self.speak("Shown");
    }
    pub fn cannot_hide(&mut self) {
        if self.should_beep() {
            sound::cannot_hide();
        }
        self.speak("Cannot hide this window");
    }
    pub fn transparent(&mut self) {
        if self.should_beep() {
            sound::transparent();
        }
        self.speak("Transparent");
    }
    pub fn solid(&mut self) {
        if self.should_beep() {
            sound::solid();
        }
        self.speak("Solid");
    }
    pub fn killed(&mut self) {
        if self.should_beep() {
            sound::killed();
        }
        self.speak("Killed");
    }
    pub fn disappeared(&mut self) {
        if self.should_beep() {
            sound::disappeared();
        }
        self.speak("Window gone");
    }
    pub fn priority(&mut self, index: i32) {
        if self.should_beep() {
            sound::priority(index);
        }
        self.speak(priority_name(index));
    }
    pub fn priority_error(&mut self) {
        if self.should_beep() {
            sound::priority_error();
        }
        self.speak("Could not change priority");
    }
    pub fn stack(&mut self, stack: usize, use_counter: bool) {
        if self.should_beep() {
            if use_counter || !sound::play_number(stack) {
                sound::stack_beeps(stack);
            }
        }
        self.speak(&format!("Stack {stack}"));
    }
    pub fn managed(&mut self, exe: &str) {
        if self.should_beep() {
            sound::solid();
        }
        self.speak(&format!("Auto-transparent {exe}"));
    }
    pub fn unmanaged(&mut self, exe: &str) {
        if self.should_beep() {
            sound::solid();
        }
        self.speak(&format!("Stopped auto-transparent {exe}"));
    }
    pub fn reloaded(&mut self) {
        if self.should_beep() {
            sound::solid();
        }
        self.speak("Configuration reloaded");
    }
}

fn priority_name(index: i32) -> &'static str {
    match index {
        0 => "Low priority",
        1 => "Below normal priority",
        2 => "Normal priority",
        3 => "Above normal priority",
        4 => "High priority",
        5 => "Realtime priority",
        _ => "Priority changed",
    }
}

// ---------------------------------------------------------------------------
// Speaker: the active screen reader (NVDA) when running, else SAPI. Prism under
// the `speech` feature.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "speech"))]
mod speaker_impl {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{s, w, PCWSTR};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Media::Speech::{ISpVoice, SpVoice, SPF_ASYNC, SPF_PURGEBEFORESPEAK};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    // NVDA controller-client entry points (nvdaControllerClient64.dll).
    type TestFn = unsafe extern "system" fn() -> i32;
    type SpeakFn = unsafe extern "system" fn(PCWSTR) -> i32;
    type CancelFn = unsafe extern "system" fn() -> i32;

    /// A loaded NVDA controller client. `nvdaController_testIfRunning` returns 0
    /// (RPC_S_OK) only while NVDA is actually running.
    struct Nvda {
        test: TestFn,
        speak: SpeakFn,
        cancel: CancelFn,
    }

    impl Nvda {
        fn load() -> Option<Nvda> {
            unsafe {
                let hmod = load_client()?;
                let test = GetProcAddress(hmod, s!("nvdaController_testIfRunning"))?;
                let speak = GetProcAddress(hmod, s!("nvdaController_speakText"))?;
                let cancel = GetProcAddress(hmod, s!("nvdaController_cancelSpeech"))?;
                Some(Nvda {
                    test: std::mem::transmute::<_, TestFn>(test),
                    speak: std::mem::transmute::<_, SpeakFn>(speak),
                    cancel: std::mem::transmute::<_, CancelFn>(cancel),
                })
            }
        }

        fn running(&self) -> bool {
            unsafe { (self.test)() == 0 }
        }

        fn speak(&self, wide: &[u16]) {
            unsafe {
                let _ = (self.cancel)(); // interrupt, like a fresh announcement
                let _ = (self.speak)(PCWSTR(wide.as_ptr()));
            }
        }
    }

    /// Load `nvdaControllerClient64.dll` — first from next to our exe (where we
    /// ship it), then from the default search path as a fallback.
    fn load_client() -> Option<HMODULE> {
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())) {
            let path = dir.join("nvdaControllerClient64.dll");
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
            if let Ok(h) = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) } {
                return Some(h);
            }
        }
        unsafe { LoadLibraryW(w!("nvdaControllerClient64.dll")).ok() }
    }

    /// Speaks through the active screen reader (NVDA) when it is running, else via
    /// the built-in Windows Speech API (SAPI 5).
    pub struct Speaker {
        nvda: Option<Nvda>,
        sapi: Option<ISpVoice>,
    }

    impl Speaker {
        pub fn new() -> Option<Self> {
            let nvda = Nvda::load();
            let sapi = unsafe {
                // Safe to call repeatedly; ignore "already initialized".
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                CoCreateInstance(&SpVoice, None, CLSCTX_ALL).ok()
            };
            if nvda.is_none() && sapi.is_none() {
                return None;
            }
            Some(Speaker { nvda, sapi })
        }

        pub fn speak(&mut self, text: &str) {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            // Prefer the screen reader if it's actually running right now.
            if let Some(n) = &self.nvda {
                if n.running() {
                    n.speak(&wide);
                    return;
                }
            }
            if let Some(v) = &self.sapi {
                let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32;
                unsafe {
                    let _ = v.Speak(PCWSTR(wide.as_ptr()), flags, None);
                }
            }
        }
    }
}

#[cfg(feature = "speech")]
mod speaker_impl {
    /// Speaks via Prism (routes to the active screen reader / TTS).
    pub struct Speaker {
        backend: prism::Backend,
        _ctx: prism::Context,
    }

    impl Speaker {
        pub fn new() -> Option<Self> {
            let ctx = prism::Context::new().ok()?;
            let backend = ctx.acquire_best().ok()?;
            Some(Speaker { backend, _ctx: ctx })
        }

        pub fn speak(&mut self, text: &str) {
            let _ = self.backend.speak(text, true);
        }
    }
}

use speaker_impl::Speaker;
