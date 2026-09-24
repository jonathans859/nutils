#![windows_subsystem = "windows"]

mod config;
mod feedback;
mod hotkeys;
mod inject;
mod murderer;
mod sound;
mod stacks;
mod state;
mod status;
mod tray;
mod ui;
mod visual;
mod window;
mod winevent;

use config::{Config, MatchKind};
use state::ManagedApp;
use stacks::{human_to_slot, Stacks, STACK_SIZE};
use std::cell::RefCell;
use tray::MenuChoice;
use window::{from_id, root, to_id};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::WindowsAndMessaging::*;

const TIMER_ID: usize = 1;
const WINDOW_TITLE: PCWSTR = w!("NUtils");

// Hotkey ids.
const HK_DIGIT_BASE: i32 = 1; // 1..=10 for digits 0..9
const HK_PRIO_BASE: i32 = 11; // 11..=16 for F3..F8
const HK_CHTITLE: i32 = 20;
const HK_TRANSPARENT: i32 = 21;
const HK_SOLID: i32 = 22;
const HK_FIRSTAVAIL: i32 = 23;
const HK_WINKILL: i32 = 25;
const HK_MANAGEAPP: i32 = 26;
const HK_UNMANAGEAPP: i32 = 27;
const HK_STACKUP: i32 = 28;
const HK_STACKDOWN: i32 = 29;
const HK_STATUS: i32 = 30;
const HK_MAX: i32 = 30;

struct App {
    hwnd: HWND,
    cfg: Config,
    stacks: Stacks,
    /// Apps whose windows are made transparent automatically (kept in state.toml).
    managed_apps: Vec<ManagedApp>,
    _tray: tray::Tray,
    hook: winevent::HookThread,
    fb: feedback::Feedback,
    /// Windows made transparent by the per-window hotkey (as opposed to by an
    /// auto-transparent app). Manual transparency is the user's own state: the
    /// auto-transparent hotkeys never undo it, and stopping auto-transparent
    /// leaves these windows transparent. Pruned on the timer tick.
    manual_transparent: std::collections::HashSet<window::WinId>,
    /// Windows of auto-transparent apps that could not be made transparent, so
    /// each is announced once rather than on every timer tick.
    auto_failed: std::collections::HashSet<window::WinId>,
    /// Last-seen modification time of config.toml, for change-driven reloads.
    config_mtime: Option<std::time::SystemTime>,
}

/// Modification time of the config file, if it exists.
fn config_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(Config::config_path())
        .and_then(|m| m.modified())
        .ok()
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn main() {
    // Single instance: bail if another NUtils window already exists.
    unsafe {
        if FindWindowW(None, WINDOW_TITLE).is_ok() {
            return;
        }
    }

    let cfg = Config::load_or_init();
    let state = state::State::load();
    let mut stacks = Stacks::from_saved(state.hidden.as_ref());
    // Windows NUtils hid but lost track of (state.toml deleted or reset).
    let recovered = stacks.recover(&window::enum_top_windows());

    let instance = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    let class = w!("NUtilsMainWnd");
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
    }

    // A normal (never-shown) top-level window: receives hotkeys, the tray callback,
    // and the timer, and can take foreground for the menu.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            WINDOW_TITLE,
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("create message window")
    };

    winevent::set_managed(&state.managed_apps);
    let hook = winevent::install();
    // Inject the in-process helper into any already-running managed app, so its
    // *new* windows are hidden with no flash. Also hide the windows it has open now.
    for w in window::enum_top_windows() {
        if winevent::is_managed_window(w) {
            inject::ensure(w);
            window::make_transparent(w);
        }
    }
    let tray = tray::Tray::new(hwnd);
    let fb = feedback::Feedback::new(cfg.settings.feedback);

    let mut app = App {
        hwnd,
        cfg,
        stacks,
        managed_apps: state.managed_apps,
        _tray: tray,
        hook,
        fb,
        manual_transparent: std::collections::HashSet::new(),
        auto_failed: std::collections::HashSet::new(),
        config_mtime: config_mtime(),
    };
    app.set_hotkeys(true);
    if recovered > 0 {
        app.save_state();
    }
    app.fb.ready(recovered); // startup: beep + "NUtils ready"
    APP.with(|a| *a.borrow_mut() = Some(app));

    unsafe {
        SetTimer(Some(hwnd), TIMER_ID, 1000, None);
    }

    // Message loop.
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // Teardown.
    if let Some(app) = APP.with(|a| a.borrow_mut().take()) {
        app.set_hotkeys(false);
        winevent::uninstall(app.hook);
        inject::uninstall_all();
        // app.tray removed on drop
    }
}

/// The window procedure moves `App` out of the thread-local while handling a
/// message, so any message dispatched by a nested modal pump (dialogs, the tray
/// menu) simply finds `None` and is ignored rather than causing a re-entrant
/// borrow. This is the whole reentrancy strategy.
extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY | tray::WM_TRAY | WM_TIMER => {
            if let Some(mut app) = APP.with(|a| a.borrow_mut().take()) {
                let keep_running = app.handle(msg, wp, lp);
                APP.with(|a| *a.borrow_mut() = Some(app));
                if !keep_running {
                    unsafe { PostQuitMessage(0) };
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

impl App {
    /// Handle one message. Returns `false` to request application exit.
    fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> bool {
        match msg {
            WM_HOTKEY => self.on_hotkey(wp.0 as i32),
            tray::WM_TRAY => {
                let mouse = lp.0 as u32;
                if mouse == WM_RBUTTONUP || mouse == WM_LBUTTONUP {
                    return self.on_tray_menu();
                }
            }
            WM_TIMER => self.on_timer(),
            _ => {}
        }
        true
    }

    // ---- hotkeys ----------------------------------------------------------

    fn on_hotkey(&mut self, id: i32) {
        match id {
            HK_DIGIT_BASE..=10 => self.toggle_slot((id - HK_DIGIT_BASE) as u32),
            HK_PRIO_BASE..=16 => self.set_priority((id - HK_PRIO_BASE) as usize),
            HK_TRANSPARENT => self.set_transparent(true),
            HK_SOLID => self.set_transparent(false),
            HK_FIRSTAVAIL => self.hide_in_first(),
            HK_WINKILL => self.kill_active(),
            HK_STACKUP => self.stack_shift(true),
            HK_STACKDOWN => self.stack_shift(false),
            HK_STATUS => self.announce_status(),
            HK_CHTITLE => self.change_title(),
            HK_MANAGEAPP => self.manage_active_app(),
            HK_UNMANAGEAPP => self.unmanage_active_app(),
            _ => {}
        }
    }

    /// (De)register every global hotkey from the current config.
    fn set_hotkeys(&self, enable: bool) {
        let h = Some(self.hwnd);
        for id in 1..=HK_MAX {
            unsafe {
                let _ = UnregisterHotKey(h, id);
            }
        }
        if !enable {
            return;
        }
        let hk = &self.cfg.hotkeys;
        let register = |id: i32, spec: &str| {
            if let Some(k) = hotkeys::parse(spec) {
                unsafe {
                    let _ = RegisterHotKey(h, id, k.mods, k.vk);
                }
            }
        };
        let reg = |id: i32, binding: &str| {
            if let Some(spec) = hk.resolve(binding) {
                register(id, &spec);
            }
        };
        if let Some(mods) = hk.slot_mods() {
            for d in 0..10i32 {
                register(HK_DIGIT_BASE + d, &format!("{mods}{d}"));
            }
        }
        if let Some(mods) = hk.priority_mods() {
            for i in 0..6i32 {
                register(HK_PRIO_BASE + i, &format!("{mods}{{f{}}}", i + 3));
            }
        }
        reg(HK_CHTITLE, &hk.chtitle);
        reg(HK_TRANSPARENT, &hk.transparent);
        reg(HK_SOLID, &hk.solid);
        reg(HK_FIRSTAVAIL, &hk.firstavailhide);
        reg(HK_WINKILL, &hk.winkill);
        reg(HK_MANAGEAPP, &hk.manageapp);
        reg(HK_UNMANAGEAPP, &hk.unmanageapp);
        reg(HK_STACKUP, &hk.stackup);
        reg(HK_STACKDOWN, &hk.stackdown);
        reg(HK_STATUS, &hk.status);
    }

    // ---- window actions ---------------------------------------------------

    /// Hide the active window into `slot` (0..9 within the current stack), or
    /// unhide whatever is already stored there.
    fn toggle_slot(&mut self, digit: u32) {
        let slot = human_to_slot(digit) + self.stacks.shift;
        self.stacks.ensure(slot);
        if self.stacks.get(slot) != 0 {
            let h = from_id(self.stacks.get(slot));
            window::show(h);
            self.stacks.clear(slot);
            self.fb.window_up();
        } else {
            let hwnd = root(window::foreground());
            if window::is_shell_window(hwnd) {
                self.fb.cannot_hide();
                return;
            }
            window::hide(hwnd, slot);
            self.stacks.set(slot, to_id(hwnd));
            self.fb.window_down();
        }
        self.stacks.prune();
        self.save_state();
    }

    fn hide_in_first(&mut self) {
        let slot = self.stacks.first_free_or_grow(self.stacks.shift);
        let hwnd = root(window::foreground());
        if window::is_shell_window(hwnd) {
            self.fb.cannot_hide();
            return;
        }
        window::hide(hwnd, slot);
        self.stacks.set(slot, to_id(hwnd));
        self.fb.window_down();
        self.stacks.prune();
        self.save_state();
    }

    /// Per-window transparency (Win+Shift+\ and Win+Shift+/).
    ///
    /// Refused on a window of an auto-transparent app: that app's transparency is
    /// owned by the auto-transparent machinery — the in-process helper and the
    /// window-event watcher would just re-apply it — so the two must not fight.
    /// Stop auto-transparenting the app first.
    fn set_transparent(&mut self, transparent: bool) {
        let hwnd = root(window::foreground());
        if winevent::is_managed_window(hwnd) {
            self.fb.managed_blocked();
            return;
        }
        if transparent {
            if self.make_transparent_checked(hwnd) {
                self.manual_transparent.insert(to_id(hwnd));
                self.fb.transparent();
            }
        } else {
            self.manual_transparent.remove(&to_id(hwnd));
            if window::make_solid(hwnd) {
                self.fb.solid();
            } else {
                self.fb.solid_failed();
            }
        }
    }

    /// Make the active window transparent for a hotkey, and confirm it: Windows
    /// must accept the change and, with the visual check on, the screen must show
    /// it. On failure the window is left as it was and the reason is announced.
    fn make_transparent_checked(&mut self, hwnd: HWND) -> bool {
        // Only compare the screen for a window that is drawn now: one that's
        // already transparent shows what's behind it before *and* after.
        let before = if self.cfg.settings.visual_check && !window::is_transparent(hwnd) {
            visual::snapshot(hwnd)
        } else {
            None
        };
        if !window::make_transparent(hwnd) {
            self.fb.transparent_failed();
            return false;
        }
        if before.is_some_and(|b| visual::compare(&b) == visual::Seen::StillVisible) {
            window::make_solid(hwnd);
            self.fb.still_visible();
            return false;
        }
        true
    }

    fn kill_active(&mut self) {
        if window::kill_owner(root(window::foreground())) {
            self.fb.killed();
        }
    }

    fn set_priority(&mut self, index: usize) {
        if window::set_priority(root(window::foreground()), index) {
            self.fb.priority(index as i32);
        } else {
            self.fb.priority_error();
        }
    }

    /// Speak how many windows are hidden in how many stacks — plus, when
    /// `detailed_status` is on, each window's stack, position and title.
    fn announce_status(&mut self) {
        let text = if self.cfg.settings.detailed_status {
            status::detailed(&self.stacks)
        } else {
            status::brief(&self.stacks)
        };
        self.fb.status(&text);
    }

    fn stack_shift(&mut self, up: bool) {
        if up {
            self.stacks.shift += STACK_SIZE;
        } else {
            self.stacks.shift = self.stacks.shift.saturating_sub(STACK_SIZE);
        }
        let stack_1based = self.stacks.shift / STACK_SIZE + 1;
        let counter = self.cfg.settings.stack_counter;
        self.fb.stack(stack_1based, counter);
    }

    // ---- dialogs ----------------------------------------------------------

    fn change_title(&mut self) {
        self.set_hotkeys(false);
        let target = root(window::foreground());
        let current = window::get_title(target);
        if let Some(new) = ui::input_box("Set Title", "Enter the new title for this window", &current)
        {
            if !new.is_empty() {
                window::set_title(target, &new);
            }
        }
        self.set_hotkeys(true);
    }

    fn unhide_slot(&mut self, slot: usize) {
        let id = self.stacks.get(slot);
        if id == 0 {
            return;
        }
        window::show(from_id(id));
        self.stacks.clear(slot);
        self.fb.window_up();
        self.stacks.prune();
        self.save_state();
    }

    /// Launch the wxWidgets settings editor (a sibling `nutils-settings.exe`).
    /// It edits config.toml; the timer notices the change and reloads.
    fn launch_settings(&self) {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("nutils-settings.exe")));
        if let Some(path) = exe {
            if path.exists() {
                let _ = std::process::Command::new(path).spawn();
                return;
            }
        }
        self.set_hotkeys(false);
        ui::message_box(
            "Settings",
            "nutils-settings.exe was not found next to nutils.exe.",
            MB_ICONWARNING,
        );
        self.set_hotkeys(true);
    }

    /// Write `state.toml`: the auto-transparent apps and the hidden windows.
    fn save_state(&self) {
        state::State {
            managed_apps: self.managed_apps.clone(),
            hidden: Some(self.stacks.to_saved()),
        }
        .save();
    }

    /// Reload configuration and language in place (no process restart), keeping
    /// currently-hidden windows.
    fn reload(&mut self) {
        self.set_hotkeys(false);
        self.cfg = Config::load_or_init();
        self.fb.set_mode(self.cfg.settings.feedback);
        self.config_mtime = config_mtime();
        self.set_hotkeys(true);
        self.fb.reloaded();
    }

    /// Add the active window's owning app to the managed (auto-transparent) list.
    fn manage_active_app(&mut self) {
        self.set_hotkeys(false);
        let active = root(window::foreground());
        if let Some(exe) = window::owner_exe(active) {
            // Try the window that's active now first: if it can't be made
            // transparent (an app running as administrator, or one that is drawn
            // regardless), the app's other windows can't either, so don't add it.
            if !self.make_transparent_checked(active) {
                self.set_hotkeys(true);
                return;
            }
            let already = self
                .managed_apps
                .iter()
                .any(|a| a.match_kind == MatchKind::Exe && a.value.eq_ignore_ascii_case(&exe));
            if !already {
                self.managed_apps.push(ManagedApp {
                    match_kind: MatchKind::Exe,
                    value: exe.clone(),
                });
                self.save_state();
                winevent::set_managed(&self.managed_apps);
            }
            // Inject the in-process helper so this app's *future* windows are born
            // transparent (zero flash).
            inject::ensure(active);
            self.fb.managed(&exe);
        }
        self.set_hotkeys(true);
    }

    /// Stop auto-transparenting the active window's app: remove it from the managed
    /// list, stop injecting into it, and make its windows solid again.
    ///
    /// Refused unless the app really is on the managed list: this hotkey undoes
    /// auto-transparency only, never a window the user made transparent by hand.
    /// For the same reason, windows in `manual_transparent` stay transparent —
    /// they go back to being the user's own manual state, undone with Win+Shift+/.
    fn unmanage_active_app(&mut self) {
        self.set_hotkeys(false);
        let active = root(window::foreground());
        if let Some(exe) = window::owner_exe(active) {
            let before = self.managed_apps.len();
            self.managed_apps
                .retain(|a| !(a.match_kind == MatchKind::Exe && a.value.eq_ignore_ascii_case(&exe)));
            if self.managed_apps.len() == before {
                self.fb.not_managed();
                self.set_hotkeys(true);
                return;
            }
            self.save_state();
            winevent::set_managed(&self.managed_apps);
            // Stop the in-process helper FIRST, so it can't re-transparent windows,
            // then make every window this app currently owns solid again. Windows
            // NUtils never made transparent are left alone by make_solid.
            inject::uninstall_owner(active);
            for w in window::enum_top_windows() {
                if window::owner_exe(w).as_deref() != Some(exe.as_str()) {
                    continue;
                }
                self.auto_failed.remove(&to_id(w));
                if !self.manual_transparent.contains(&to_id(w)) {
                    window::make_solid(w);
                }
            }
            self.fb.unmanaged(&exe);
        }
        self.set_hotkeys(true);
    }

    // ---- tray -------------------------------------------------------------

    /// Returns false to exit the application.
    fn on_tray_menu(&mut self) -> bool {
        self.set_hotkeys(false);
        let choice = tray::show_menu(self.hwnd, &self.stacks);
        self.set_hotkeys(true);
        match choice {
            Some(MenuChoice::Exit) => return false,
            Some(MenuChoice::Settings) => self.launch_settings(),
            Some(MenuChoice::Status) => self.announce_status(),
            Some(MenuChoice::Unhide(slot)) => self.unhide_slot(slot),
            None => {}
        }
        true
    }

    // ---- timer & api ------------------------------------------------------

    /// Check that every window NUtils made transparent still is, and re-apply it
    /// where it has been undone: some apps reset their own layered style, or
    /// rebuild a window, and would otherwise reappear while NUtils still believes
    /// they're transparent. A window that can't be made transparent again is
    /// announced once, so the user knows it is visible.
    fn keep_transparent(&mut self) {
        let mut lost = Vec::new();
        self.manual_transparent.retain(|&id| {
            let h = from_id(id);
            if !window::exists(h) {
                return false;
            }
            if window::is_transparent(h) || window::make_transparent(h) {
                return true;
            }
            lost.push(h);
            false
        });
        self.auto_failed.retain(|&id| window::exists(from_id(id)));
        for w in window::enum_top_windows() {
            if !window::is_visible(w) || window::is_transparent(w) || !winevent::is_managed_window(w) {
                continue;
            }
            if window::make_transparent(w) {
                inject::ensure(w);
            } else if self.auto_failed.insert(to_id(w)) {
                lost.push(w);
            }
        }
        for h in lost {
            self.fb.transparency_lost(&window::get_title(h));
        }
    }

    /// Purge slots whose windows vanished or were restored elsewhere, then run
    /// the WinMurderer sweep.
    fn on_timer(&mut self) {
        let mut changed = false;
        for i in 0..self.stacks.slots.len() {
            let id = self.stacks.get(i);
            if id == 0 {
                continue;
            }
            let h = from_id(id);
            if !window::exists(h) || window::is_visible(h) {
                window::clear_hidden_mark(h); // shown some other way
                self.stacks.clear(i);
                self.fb.disappeared();
                changed = true;
            }
        }
        if changed {
            self.stacks.prune();
            self.save_state();
        }
        self.keep_transparent();
        murderer::sweep(&self.cfg.rules);

        // Live-reload if the settings editor (or a manual edit) changed config.toml.
        let current = config_mtime();
        if current != self.config_mtime && current.is_some() {
            self.reload();
        }
    }
}
