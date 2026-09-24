//! The optional visual check: confirm on screen that a window really disappeared
//! when it was made transparent.
//!
//! Reading the layered state back (window.rs) catches Windows refusing the change,
//! but it can't see a window that accepts alpha 0 and is drawn anyway. The only
//! way to know is to look: capture the window's area before and after, and
//! compare. (Every app tested so far, DirectComposition ones like Windows
//! Terminal included, really disappears; this is a safety net.)
//!
//! It is off by default because it can't work with NVDA's Screen Curtain on
//! (every capture is black), and it only runs from a hotkey, when the window is
//! the active one and so on screen.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmFlush, DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

/// Every `STEP`th pixel in each direction is compared — plenty to tell a window
/// from what's behind it, at a sixteenth of the work.
const STEP: usize = 4;

/// Share of unchanged pixels at or above which the window counts as still
/// visible. Windows that really vanished left at most 90% unchanged in testing
/// (Notepad over another Notepad); an untouched window leaves 100%. The margin
/// absorbs a blinking caret.
const STILL_VISIBLE: f64 = 0.98;

/// A sampled capture of the screen area a window covers.
pub struct Snapshot {
    rect: RECT,
    pixels: Vec<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Seen {
    /// The screen changed: the window is gone.
    Gone,
    /// The screen looks the same: the window is still drawn.
    StillVisible,
    /// Can't tell: the capture is blank (Screen Curtain) or failed.
    Unknown,
}

/// Capture the screen area `hwnd` covers, clipped to the desktop. `None` if the
/// window has no visible area (minimised, off-screen).
pub fn snapshot(hwnd: HWND) -> Option<Snapshot> {
    let mut r = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
        let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        r.left = r.left.max(vx);
        r.top = r.top.max(vy);
        r.right = r.right.min(vx + vw);
        r.bottom = r.bottom.min(vy + vh);
    }
    if r.right <= r.left || r.bottom <= r.top {
        return None;
    }
    let pixels = capture(&r)?;
    Some(Snapshot { rect: r, pixels })
}

/// After a change, wait for the screen to show it, capture the same area again
/// and judge whether the window is still there.
pub fn compare(before: &Snapshot) -> Seen {
    // Each DwmFlush waits for one composition pass; a few make sure the change
    // has reached the screen, including for apps that repaint on the next frame.
    for _ in 0..3 {
        unsafe {
            let _ = DwmFlush();
        }
    }
    match capture(&before.rect) {
        Some(after) => judge(&before.pixels, &after),
        None => Seen::Unknown,
    }
}

/// Compare two sampled captures of the same area.
fn judge(before: &[u32], after: &[u32]) -> Seen {
    if before.is_empty() || before.len() != after.len() {
        return Seen::Unknown;
    }
    // One colour throughout is Screen Curtain's black screen (or a window with
    // nothing to see): whatever happens next, the comparison means nothing.
    if before.iter().all(|&p| p == before[0]) {
        return Seen::Unknown;
    }
    if unchanged_share(before, after) >= STILL_VISIBLE {
        Seen::StillVisible
    } else {
        Seen::Gone
    }
}

fn unchanged_share(before: &[u32], after: &[u32]) -> f64 {
    let same = before.iter().zip(after).filter(|(a, b)| a == b).count();
    same as f64 / before.len().max(1) as f64
}

/// Copy a screen area and keep every `STEP`th pixel of every `STEP`th row, as
/// 0xRRGGBB (the alpha byte BitBlt leaves is meaningless).
fn capture(r: &RECT) -> Option<Vec<u32>> {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let result = match CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) => {
                let old = SelectObject(mem, bmp.into());
                // CAPTUREBLT includes layered windows, which are exactly what we test.
                let copied = BitBlt(mem, 0, 0, w, h, Some(screen), r.left, r.top, SRCCOPY | CAPTUREBLT);
                let pixels = copied.ok().map(|_| {
                    let all = std::slice::from_raw_parts(bits as *const u32, (w * h) as usize);
                    all.chunks(w as usize)
                        .step_by(STEP)
                        .flat_map(|row| row.iter().step_by(STEP).map(|p| p & 0x00FF_FFFF))
                        .collect()
                });
                SelectObject(mem, old);
                let _ = DeleteObject(bmp.into());
                pixels
            }
            Err(_) => None,
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_screen_means_still_visible() {
        let before: Vec<u32> = (0..1000).collect();
        let mut after = before.clone();
        after[3] = 0xFFFFFF; // a blinking caret
        assert_eq!(judge(&before, &after), Seen::StillVisible);
    }

    #[test]
    fn changed_screen_means_gone() {
        let before: Vec<u32> = (0..1000).collect();
        let after: Vec<u32> = before.iter().map(|p| if p % 10 == 0 { 7 } else { *p }).collect();
        assert_eq!(judge(&before, &after), Seen::Gone, "10% changed is a vanished window");
    }

    /// On the real screen (needs Screen Curtain off, so ignored by default):
    ///   cargo test --release visual::tests::on_screen -- --ignored --nocapture
    #[test]
    #[ignore]
    fn on_screen_plain_window_disappears() {
        use crate::window;
        use windows::core::w;
        use windows::Win32::Foundation::{COLORREF, LPARAM, LRESULT, WPARAM};
        use windows::Win32::Graphics::Gdi::{CreateSolidBrush, UpdateWindow};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::*;

        unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
            DefWindowProcW(h, m, wp, lp)
        }
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: w!("NUtilsVisualTest"),
                hbrBackground: CreateSolidBrush(COLORREF(0x0020_40E0)),
                ..Default::default()
            };
            RegisterClassW(&class);
            // Topmost and never activated, so it is on top without taking focus.
            let h = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("NUtilsVisualTest"),
                w!("visual check test"),
                WS_POPUP | WS_BORDER,
                200, 200, 300, 200,
                None, None, Some(hinst.into()), None,
            )
            .unwrap();
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(h);
            for _ in 0..3 {
                let _ = DwmFlush();
            }

            let before = snapshot(h).expect("window is on screen");
            let unchanged = compare(&before);
            assert!(window::make_transparent(h));
            let transparent = compare(&before);
            window::make_solid(h);
            DestroyWindow(h).unwrap();

            assert_ne!(unchanged, Seen::Unknown, "blank capture: is Screen Curtain on?");
            assert_eq!(unchanged, Seen::StillVisible);
            assert_eq!(transparent, Seen::Gone);
        }
    }

    #[test]
    fn blank_capture_cannot_tell() {
        let black = vec![0u32; 1000];
        assert_eq!(judge(&black, &black), Seen::Unknown, "Screen Curtain");
        assert_eq!(judge(&[], &[]), Seen::Unknown);
    }
}
