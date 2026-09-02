//! Quick check that NVDA speech works via the controller client. Speaks a test
//! phrase through NVDA if it is running. Run: cargo run --release --example speaktest
use std::os::windows::ffi::OsStrExt;
use windows::core::{s, w, PCWSTR};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type TestFn = unsafe extern "system" fn() -> i32;
type SpeakFn = unsafe extern "system" fn(PCWSTR) -> i32;

fn main() {
    unsafe {
        // Load next to this exe (examples build to target/release/examples), then fall
        // back to target/release where we copied the DLL, then the search path.
        let mut h = LoadLibraryW(w!("nvdaControllerClient64.dll"));
        if h.is_err() {
            let p = std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("nvdaControllerClient64.dll");
            let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
            h = LoadLibraryW(PCWSTR(wide.as_ptr()));
        }
        let hmod = match h {
            Ok(h) => h,
            Err(e) => {
                println!("could not load nvdaControllerClient64.dll: {e}");
                return;
            }
        };
        let test: TestFn = std::mem::transmute(GetProcAddress(hmod, s!("nvdaController_testIfRunning")));
        let speak: SpeakFn = std::mem::transmute(GetProcAddress(hmod, s!("nvdaController_speakText")));
        let running = test() == 0;
        println!("NVDA running (testIfRunning==0): {running}");
        if running {
            let msg: Vec<u16> = "NUtils screen reader test. If you hear this, NVDA speech works."
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let r = speak(PCWSTR(msg.as_ptr()));
            println!("speakText returned {r} (0 == success). You should have heard NVDA speak.");
        } else {
            println!("NVDA is not reporting as running, so nothing was spoken.");
        }
    }
}
