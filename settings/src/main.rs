//! NUtils settings — an accessible shortcuts editor built with wxWidgets (via
//! wxDragon), closely following Fedra's accessible dialog design: a General tab,
//! and a Keybindings tab that lists shortcuts in a `ListBox` and edits one at a
//! time in a "Set Shortcut" sub-dialog (modifier checkboxes + a key-capture field
//! + a screen-reader live region announcing the detected key).
//!
//! It edits the same `config.toml` the core reads (structs shared from
//! `../../src/config.rs`); the running core notices the file change and reloads.

#![windows_subsystem = "windows"]

#[path = "../../src/config.rs"]
#[allow(dead_code)]
mod config;

use config::{Config, FeedbackMode, Hotkeys};
use std::cell::RefCell;
use std::rc::Rc;
use wxdragon::prelude::*;

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Every rebindable action. `Bass` is special: it stores only the modifier
/// prefix held together with the digits 0–9 (no key).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Bass,
    StackUp,
    StackDown,
    FirstAvail,
    Transparent,
    Solid,
    ChTitle,
    WinKill,
    ManageApp,
    UnmanageApp,
    Status,
}

const ROWS: &[(Act, &str)] = &[
    (Act::Bass, "Hide/unhide slot (modifiers held with 0–9)"),
    (Act::StackUp, "Next stack"),
    (Act::StackDown, "Previous stack"),
    (Act::FirstAvail, "Hide in first free slot"),
    (Act::Transparent, "Make window transparent"),
    (Act::Solid, "Make window solid"),
    (Act::ChTitle, "Change window title"),
    (Act::WinKill, "Kill active window"),
    (Act::ManageApp, "Auto-transparent active window's app"),
    (Act::UnmanageApp, "Stop auto-transparenting active window's app"),
    (Act::Status, "Speak how many windows are hidden"),
];

fn is_bass(act: Act) -> bool {
    act == Act::Bass
}

fn spec_of(cfg: &Config, act: Act) -> String {
    let h = &cfg.hotkeys;
    match act {
        Act::Bass => h.bass.clone(),
        Act::StackUp => h.stackup.clone(),
        Act::StackDown => h.stackdown.clone(),
        Act::FirstAvail => h.firstavailhide.clone(),
        Act::Transparent => h.transparent.clone(),
        Act::Solid => h.solid.clone(),
        Act::ChTitle => h.chtitle.clone(),
        Act::WinKill => h.winkill.clone(),
        Act::ManageApp => h.manageapp.clone(),
        Act::UnmanageApp => h.unmanageapp.clone(),
        Act::Status => h.status.clone(),
    }
}

fn set_spec(cfg: &mut Config, act: Act, s: String) {
    let h = &mut cfg.hotkeys;
    match act {
        Act::Bass => h.bass = s,
        Act::StackUp => h.stackup = s,
        Act::StackDown => h.stackdown = s,
        Act::FirstAvail => h.firstavailhide = s,
        Act::Transparent => h.transparent = s,
        Act::Solid => h.solid = s,
        Act::ChTitle => h.chtitle = s,
        Act::WinKill => h.winkill = s,
        Act::ManageApp => h.manageapp = s,
        Act::UnmanageApp => h.unmanageapp = s,
        Act::Status => h.status = s,
    }
}

fn default_spec(act: Act) -> String {
    let mut d = Config::default();
    spec_of(&{ d.hotkeys = Hotkeys::default(); d }, act)
}

// ---------------------------------------------------------------------------
// Chord <-> NUtils spec conversion
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Chord {
    win: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
    /// Human key name, e.g. "T", "F4", "Escape", "\". Empty for `Bass`.
    key: String,
}

/// wxWidgets keycodes for modifier keys, ignored while capturing.
fn is_modifier_code(code: i32) -> bool {
    matches!(code, 306 | 307 | 308 | 309 | 393 | 394 | 396)
}

/// "Obvious" dialog/typing keys that must NOT be bound as hotkeys: Tab, Enter
/// (main + numpad), Escape, Space, Backspace. They keep their normal dialog roles
/// (move focus, confirm, cancel) instead of being captured. Arrows, Home/End,
/// PageUp/Down, Insert/Delete, F-keys, numpad and printable keys stay bindable.
fn is_disallowed_key(code: i32) -> bool {
    matches!(code, 9 | 13 | 370 | 27 | 32 | 8)
}

/// A wxWidgets keycode to a human key name (like Fedra's `from_key_code`).
fn keycode_to_keyname(code: i32) -> Option<String> {
    Some(match code {
        340..=363 => format!("F{}", code - 340 + 1),
        27 => "Escape".into(),
        13 | 370 => "Enter".into(),
        9 => "Tab".into(),
        32 => "Space".into(),
        8 => "Backspace".into(),
        127 | 386 => "Delete".into(),
        314 | 378 => "Left".into(),
        315 | 382 => "Up".into(),
        316 | 380 => "Right".into(),
        317 | 383 => "Down".into(),
        313 | 377 => "Home".into(),
        312 | 379 => "End".into(),
        366 | 376 => "PageUp".into(),
        367 | 381 => "PageDown".into(),
        322 | 384 => "Insert".into(),
        335 | 388 => "Num+".into(),
        337 | 390 => "Num-".into(),
        334 | 387 => "Num*".into(),
        339 | 391 => "Num/".into(),
        338 | 389 => "Num.".into(),
        65..=90 => char::from_u32(code as u32)?.to_string(),
        97..=122 => char::from_u32((code - 32) as u32)?.to_string(),
        48..=57 => char::from_u32(code as u32)?.to_string(),
        44 | 188 => ",".into(),
        46 | 190 => ".".into(),
        47 | 191 => "/".into(),
        91 | 219 => "[".into(),
        93 | 221 => "]".into(),
        92 | 220 => "\\".into(),
        45 | 189 => "-".into(),
        61 | 187 => "=".into(),
        59 | 186 => ";".into(),
        39 | 222 => "'".into(),
        96 | 192 => "`".into(),
        _ => return None,
    })
}

/// A human key name to the NUtils spec key token (`{f4}`, `t`, `\`, …).
fn keyname_to_token(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "escape" => "{esc}".into(),
        "enter" => "{enter}".into(),
        "tab" => "{tab}".into(),
        "space" => "{space}".into(),
        "delete" => "{del}".into(),
        "backspace" => "{bs}".into(),
        "left" => "{left}".into(),
        "up" => "{up}".into(),
        "right" => "{right}".into(),
        "down" => "{down}".into(),
        "home" => "{home}".into(),
        "end" => "{end}".into(),
        "pageup" => "{pgup}".into(),
        "pagedown" => "{pgdn}".into(),
        "insert" => "{ins}".into(),
        "num+" => "{add}".into(),
        "num-" => "{subtract}".into(),
        "num*" => "{multiply}".into(),
        "num/" => "{divide}".into(),
        "num." => "{decimal}".into(),
        _ if lower.starts_with('f') && lower[1..].chars().all(|c| c.is_ascii_digit()) && lower.len() > 1 => {
            format!("{{{lower}}}")
        }
        _ if name.chars().count() == 1 => name.to_ascii_lowercase(),
        _ => name.to_ascii_lowercase(),
    }
}

/// A stored spec's key part (after the modifiers) to a human key name.
fn token_to_keyname(rest: &str) -> String {
    if rest.starts_with('{') && rest.ends_with('}') && rest.len() > 2 {
        let inner = &rest[1..rest.len() - 1];
        match inner.to_ascii_lowercase().as_str() {
            "esc" => "Escape".into(),
            "enter" => "Enter".into(),
            "tab" => "Tab".into(),
            "space" => "Space".into(),
            "del" => "Delete".into(),
            "left" => "Left".into(),
            "up" => "Up".into(),
            "right" => "Right".into(),
            "down" => "Down".into(),
            "home" => "Home".into(),
            "end" => "End".into(),
            "pgup" => "PageUp".into(),
            "pgdn" => "PageDown".into(),
            "ins" => "Insert".into(),
            "add" => "Num+".into(),
            "subtract" => "Num-".into(),
            "multiply" => "Num*".into(),
            "divide" => "Num/".into(),
            "decimal" => "Num.".into(),
            other => other.to_uppercase(),
        }
    } else {
        rest.to_uppercase()
    }
}

/// Parse a NUtils spec (e.g. `#+t`) into a [`Chord`].
fn parse_spec(spec: &str) -> Chord {
    let mut c = Chord::default();
    let mut rest = spec;
    loop {
        match rest.chars().next() {
            Some('#') => c.win = true,
            Some('^') => c.ctrl = true,
            Some('!') => c.alt = true,
            Some('+') => c.shift = true,
            _ => break,
        }
        rest = &rest[1..];
    }
    if !rest.is_empty() {
        c.key = token_to_keyname(rest);
    }
    c
}

/// Build a NUtils spec from a [`Chord`]. For `bass`, the key is ignored.
fn build_spec(c: &Chord, bass: bool) -> String {
    let mut s = String::new();
    if c.win {
        s.push('#');
    }
    if c.ctrl {
        s.push('^');
    }
    if c.alt {
        s.push('!');
    }
    if c.shift {
        s.push('+');
    }
    if !bass && !c.key.is_empty() {
        s.push_str(&keyname_to_token(&c.key));
    }
    s
}

/// A human-readable rendering, e.g. "Win+Shift+T" or "Ctrl+Shift" or "(unset)".
fn display_chord(c: &Chord, bass: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if c.win {
        parts.push("Win".into());
    }
    if c.ctrl {
        parts.push("Ctrl".into());
    }
    if c.alt {
        parts.push("Alt".into());
    }
    if c.shift {
        parts.push("Shift".into());
    }
    if !bass && !c.key.is_empty() {
        parts.push(c.key.clone());
    }
    if parts.is_empty() {
        "(unset)".into()
    } else {
        parts.join("+")
    }
}

fn display_spec(spec: &str, bass: bool) -> String {
    display_chord(&parse_spec(spec), bass)
}

fn list_item(cfg: &Config, act: Act, label: &str) -> String {
    format!("{}: {}", label, display_spec(&spec_of(cfg, act), is_bass(act)))
}

// ---------------------------------------------------------------------------
// Screen-reader live region (announces the detected key), copied from Fedra.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod live_region {
    use accesskit::{
        ActionHandler, ActionRequest, ActivationHandler, Live, Node, NodeId, Role, Tree, TreeId,
        TreeUpdate,
    };
    use accesskit_windows::SubclassingAdapter;
    use std::cell::RefCell;
    use std::rc::Rc;
    use windows::Win32::Foundation::HWND;
    use wxdragon::prelude::*;

    const ROOT_ID: NodeId = NodeId(1);
    const ANNOUNCEMENT_ID: NodeId = NodeId(2);

    struct Activation;
    impl ActivationHandler for Activation {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            let mut root = Node::new(Role::Window);
            root.set_children(vec![ANNOUNCEMENT_ID]);
            let mut ann = Node::new(Role::Label);
            ann.set_value("");
            ann.set_live(Live::Polite);
            Some(TreeUpdate {
                nodes: vec![(ANNOUNCEMENT_ID, ann), (ROOT_ID, root)],
                tree: Some(Tree::new(ROOT_ID)),
                focus: ROOT_ID,
                tree_id: TreeId::ROOT,
            })
        }
    }
    struct Action;
    impl ActionHandler for Action {
        fn do_action(&mut self, _request: ActionRequest) {}
    }

    /// A polite live region that speaks the detected chord as the user types it
    /// (updating a StaticText is silent to assistive tech).
    #[derive(Clone)]
    pub struct LiveRegion {
        adapter: Rc<RefCell<SubclassingAdapter>>,
        last: Rc<RefCell<Option<String>>>,
    }

    impl LiveRegion {
        pub fn new(dialog: &Dialog) -> Self {
            let hwnd = HWND(dialog.get_handle() as *mut _);
            let adapter = SubclassingAdapter::new(hwnd, Activation, Action);
            Self {
                adapter: Rc::new(RefCell::new(adapter)),
                last: Rc::new(RefCell::new(None)),
            }
        }

        pub fn announce(&self, text: &str) {
            let mut new_text = text.to_string();
            let mut last = self.last.borrow_mut();
            if last.as_deref() == Some(new_text.as_str()) {
                new_text.push('\u{00A0}'); // nudge SRs to re-announce identical text
            }
            *last = Some(new_text.clone());
            let mut node = Node::new(Role::Label);
            node.set_value(new_text);
            node.set_live(Live::Polite);
            let mut root = Node::new(Role::Window);
            root.set_children(vec![ANNOUNCEMENT_ID]);
            let update = TreeUpdate {
                nodes: vec![(ANNOUNCEMENT_ID, node), (ROOT_ID, root)],
                tree: None,
                focus: ROOT_ID,
                tree_id: TreeId::ROOT,
            };
            let mut adapter = self.adapter.borrow_mut();
            if let Some(events) = adapter.update_if_active(|| update) {
                events.raise();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Set Shortcut sub-dialog
// ---------------------------------------------------------------------------

/// Returns `None` = cancelled, `Some(Some(spec))` = set (empty spec = unbound).
fn prompt_for_shortcut(
    parent: &Frame,
    action_label: &str,
    bass: bool,
    current: &str,
) -> Option<Option<String>> {
    let title = format!("Set Shortcut: {action_label}");
    let dialog = Dialog::builder(parent, &title).with_size(420, 360).build();
    #[cfg(windows)]
    let live = live_region::LiveRegion::new(&dialog);
    let panel = Panel::builder(&dialog).build();
    let main_sizer = BoxSizer::builder(Orientation::Vertical).build();

    let info = if bass {
        format!("Choose the modifier keys held with 0–9 for {action_label}:")
    } else {
        format!("Configure the shortcut for {action_label}:")
    };
    let info_label = StaticText::builder(&panel).with_label(&info).build();
    main_sizer.add(&info_label, 0, SizerFlag::Expand | SizerFlag::All, 8);

    // Modifiers as a checkable list (check the ones you want).
    let start = parse_spec(current);
    let mods_label = StaticText::builder(&panel).with_label("&Modifiers:").build();
    main_sizer.add(&mods_label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 8);
    let mods = CheckListBox::builder(&panel).build();
    for label in ["Ctrl", "Shift", "Windows", "Alt"] {
        mods.append(label);
    }
    mods.check(0, start.ctrl);
    mods.check(1, start.shift);
    mods.check(2, start.win);
    mods.check(3, start.alt);
    main_sizer.add(&mods, 0, SizerFlag::Expand | SizerFlag::All, 8);

    // Key field (not shown for the modifiers-only "bass" action).
    let key_field = TextCtrl::builder(&panel)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::DontWrap)
        .build();
    if !bass {
        key_field.set_value(&start.key);
        key_field.set_accessibility_label("Key");
        let key_label = StaticText::builder(&panel).with_label("&Key:").build();
        let key_sizer = BoxSizer::builder(Orientation::Horizontal).build();
        key_sizer.add(&key_label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 8);
        key_sizer.add(&key_field, 1, SizerFlag::Expand, 0);
        main_sizer.add_sizer(&key_sizer, 0, SizerFlag::Expand | SizerFlag::All, 8);
        let hint = StaticText::builder(&panel)
            .with_label("Tip: click in the Key field and press any key (letters, digits, +, arrows, F-keys, …).")
            .build();
        main_sizer.add(&hint, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 8);
    }

    let preview = StaticText::builder(&panel).with_label("Detected: (none)").build();
    main_sizer.add(&preview, 0, SizerFlag::Expand | SizerFlag::All, 8);

    // Reads the current chord from the widgets.
    let read_chord = {
        let mods = mods;
        let key_field = key_field;
        move || Chord {
            ctrl: mods.is_checked(0),
            shift: mods.is_checked(1),
            win: mods.is_checked(2),
            alt: mods.is_checked(3),
            key: key_field.get_value(),
        }
    };

    // Updates the preview label and announces the chord to screen readers.
    let update_preview = {
        let read_chord = read_chord.clone();
        let preview = preview;
        #[cfg(windows)]
        let live = live.clone();
        move || {
            let text = display_chord(&read_chord(), bass);
            preview.set_label(&format!("Detected: {text}"));
            #[cfg(windows)]
            live.announce(&format!("Detected: {text}"));
        }
    };
    update_preview();

    // Key capture: press a key in the key field to set ONLY the key. Modifiers are
    // chosen via the checkable Modifiers list, so a keypress must not touch them
    // (pressing a plain key used to clear the checkboxes the user had set).
    if !bass {
        let key_c = key_field;
        let upd = update_preview.clone();
        key_field.on_key_down(move |event| {
            if let WindowEventData::Keyboard(ref ke) = event {
                let code = ke.get_key_code().unwrap_or(0);
                if is_modifier_code(code) {
                    event.skip(false);
                    return;
                }
                // Tab/Enter/Escape/Space/Backspace are not sensible hotkeys and are
                // the dialog's own navigation keys — let them do their normal job
                // rather than being captured.
                if is_disallowed_key(code) {
                    event.skip(true);
                    return;
                }
                if let Some(name) = keycode_to_keyname(code) {
                    key_c.set_value(&name);
                    upd();
                    event.skip(false);
                    return;
                }
            }
            event.skip(true);
        });
    }

    // Toggling a modifier updates the preview.
    {
        let upd = update_preview.clone();
        mods.on_toggled(move |_| upd());
    }

    // Buttons: OK / Cancel.
    let button_sizer = BoxSizer::builder(Orientation::Horizontal).build();
    let ok = Button::builder(&panel).with_id(ID_OK).with_label("OK").build();
    ok.set_default();
    let cancel = Button::builder(&panel)
        .with_id(ID_CANCEL)
        .with_label("Cancel")
        .build();
    button_sizer.add_stretch_spacer(1);
    button_sizer.add(&ok, 0, SizerFlag::Right, 8);
    button_sizer.add(&cancel, 0, SizerFlag::Right, 8);
    main_sizer.add_sizer(&button_sizer, 0, SizerFlag::Expand | SizerFlag::All, 8);

    panel.set_sizer(main_sizer, true);
    let dialog_sizer = BoxSizer::builder(Orientation::Vertical).build();
    dialog_sizer.add(&panel, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(dialog_sizer, true);
    dialog.set_affirmative_id(ID_OK);
    dialog.set_escape_id(ID_CANCEL);
    dialog.centre();
    if bass {
        mods.set_focus();
    } else {
        key_field.set_focus();
    }

    loop {
        match dialog.show_modal() {
            ID_OK => {
                let chord = read_chord();
                // An empty key clears the binding — that's allowed.
                if !bass && chord.key.trim().is_empty() {
                    return Some(Some(String::new()));
                }
                // A real shortcut must have at least one modifier; a bare key is
                // rejected and the dialog re-opens so the user can add one.
                if !bass && !(chord.ctrl || chord.shift || chord.win || chord.alt) {
                    let msg = "A shortcut needs at least one modifier: Ctrl, Shift, Windows, or Alt.";
                    info_label.set_label(msg);
                    #[cfg(windows)]
                    live.announce(msg);
                    continue;
                }
                return Some(Some(build_spec(&chord, bass)));
            }
            _ => return None,
        }
    }
}
// ---------------------------------------------------------------------------
// Main window
// ---------------------------------------------------------------------------

fn main() {
    SystemOptions::set_option_by_int("msw.no-manifest-check", 1);

    let _ = wxdragon::main(|_| {
        let cfg = Rc::new(RefCell::new(Config::load_or_init()));

        let frame = Frame::builder()
            .with_title("NUtils Settings")
            .with_size(Size::new(560, 600))
            .build();
        let panel = Panel::builder(&frame).build();
        let notebook = Notebook::builder(&panel).build();

        // ---- General tab -------------------------------------------------
        let general = Panel::builder(&notebook)
            .with_style(PanelStyle::TabTraversal)
            .build();
        let gsizer = BoxSizer::builder(Orientation::Vertical).build();

        let stack_cb = CheckBox::builder(&general)
            .with_label("&Announce the current stack by beeping that many times")
            .build();
        stack_cb.set_value(cfg.borrow().settings.stack_counter);
        gsizer.add(&stack_cb, 0, SizerFlag::All, 8);

        // Feedback mode: beeps, spoken text, or both.
        let fb_label = StaticText::builder(&general).with_label("&Feedback:").build();
        let fb_choice = Choice::builder(&general).build();
        fb_choice.append("Beeps");
        fb_choice.append("Spoken text");
        fb_choice.append("Both");
        fb_choice.set_selection(match cfg.borrow().settings.feedback {
            FeedbackMode::Beeps => 0,
            FeedbackMode::Text => 1,
            FeedbackMode::Both => 2,
        });
        let fb_sizer = BoxSizer::builder(Orientation::Horizontal).build();
        fb_sizer.add(&fb_label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 8);
        fb_sizer.add(&fb_choice, 1, SizerFlag::Expand, 0);
        gsizer.add_sizer(&fb_sizer, 0, SizerFlag::Expand | SizerFlag::All, 8);

        let detail_cb = CheckBox::builder(&general)
            .with_label("&Detailed status: also speak each hidden window's stack, position and title")
            .build();
        detail_cb.set_value(cfg.borrow().settings.detailed_status);
        gsizer.add(&detail_cb, 0, SizerFlag::All, 8);

        general.set_sizer(gsizer, true);
        notebook.add_page(&general, "General", true, None);

        // ---- Keybindings tab --------------------------------------------
        let keys = Panel::builder(&notebook)
            .with_style(PanelStyle::TabTraversal)
            .build();
        let ksizer = BoxSizer::builder(Orientation::Vertical).build();
        let list_label = StaticText::builder(&keys).with_label("&Shortcuts:").build();
        ksizer.add(&list_label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 8);

        let list = ListBox::builder(&keys).build();
        for (act, label) in ROWS {
            list.append(&list_item(&cfg.borrow(), *act, label));
        }
        list.set_selection(0, true);
        ksizer.add(&list, 1, SizerFlag::Expand | SizerFlag::All, 8);

        let btns = BoxSizer::builder(Orientation::Horizontal).build();
        let set_btn = Button::builder(&keys).with_label("&Set Shortcut...").build();
        let clear_btn = Button::builder(&keys).with_label("&Clear").build();
        let reset_btn = Button::builder(&keys).with_label("&Reset to Default").build();
        let reset_all_btn = Button::builder(&keys).with_label("Reset &All").build();
        btns.add(&set_btn, 0, SizerFlag::Right, 8);
        btns.add(&clear_btn, 0, SizerFlag::Right, 8);
        btns.add(&reset_btn, 0, SizerFlag::Right, 8);
        btns.add(&reset_all_btn, 0, SizerFlag::Right, 8);
        ksizer.add_sizer(&btns, 0, SizerFlag::Expand | SizerFlag::All, 8);

        keys.set_sizer(ksizer, true);
        notebook.add_page(&keys, "Keybindings", false, None);

        // Rebuild the list from the current config, keeping the selection.
        let refresh = {
            let cfg = cfg.clone();
            let list = list;
            move || {
                let sel = list.get_selection().unwrap_or(0);
                list.clear();
                for (act, label) in ROWS {
                    list.append(&list_item(&cfg.borrow(), *act, label));
                }
                let count = list.get_count();
                list.set_selection(if sel < count { sel } else { 0 }, true);
            }
        };

        let selected_act = {
            let list = list;
            move || -> Option<Act> {
                let sel = list.get_selection()? as usize;
                ROWS.get(sel).map(|(a, _)| *a)
            }
        };

        // Set Shortcut (button or double-click).
        let do_set = {
            let cfg = cfg.clone();
            let frame = frame;
            let refresh = refresh.clone();
            let selected_act = selected_act.clone();
            move || {
                let Some(act) = selected_act() else { return };
                let label = ROWS.iter().find(|(a, _)| *a == act).map(|(_, l)| *l).unwrap_or("");
                let current = spec_of(&cfg.borrow(), act);
                if let Some(result) = prompt_for_shortcut(&frame, label, is_bass(act), &current) {
                    if let Some(spec) = result {
                        set_spec(&mut cfg.borrow_mut(), act, spec);
                        refresh();
                    }
                }
            }
        };
        let set_click = do_set.clone();
        set_btn.on_click(move |_| set_click());
        let set_dclick = do_set;
        list.on_item_double_clicked(move |_| set_dclick());

        // Clear.
        let clear_cfg = cfg.clone();
        let clear_refresh = refresh.clone();
        let clear_sel = selected_act.clone();
        clear_btn.on_click(move |_| {
            if let Some(act) = clear_sel() {
                set_spec(&mut clear_cfg.borrow_mut(), act, String::new());
                clear_refresh();
            }
        });

        // Reset selected to default.
        let reset_cfg = cfg.clone();
        let reset_refresh = refresh.clone();
        let reset_sel = selected_act.clone();
        reset_btn.on_click(move |_| {
            if let Some(act) = reset_sel() {
                set_spec(&mut reset_cfg.borrow_mut(), act, default_spec(act));
                reset_refresh();
            }
        });

        // Reset all to defaults (with confirmation).
        let reset_all_cfg = cfg.clone();
        let reset_all_refresh = refresh.clone();
        let reset_all_frame = frame;
        reset_all_btn.on_click(move |_| {
            let warn = MessageDialog::builder(
                &reset_all_frame,
                "Reset all keyboard shortcuts to their default values?",
                "Reset Shortcuts",
            )
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
            .build();
            if warn.show_modal() == ID_YES {
                let mut c = reset_all_cfg.borrow_mut();
                for (act, _) in ROWS {
                    set_spec(&mut c, *act, default_spec(*act));
                }
                drop(c);
                reset_all_refresh();
            }
        });

        // ---- Save / Cancel + frame layout -------------------------------
        let main_sizer = BoxSizer::builder(Orientation::Vertical).build();
        main_sizer.add(&notebook, 1, SizerFlag::Expand | SizerFlag::All, 8);

        let button_sizer = BoxSizer::builder(Orientation::Horizontal).build();
        let save = Button::builder(&panel).with_label("Save").build();
        save.set_default();
        let cancel = Button::builder(&panel).with_label("Cancel").build();
        button_sizer.add_stretch_spacer(1);
        button_sizer.add(&save, 0, SizerFlag::Right, 8);
        button_sizer.add(&cancel, 0, SizerFlag::Right, 8);
        main_sizer.add_sizer(&button_sizer, 0, SizerFlag::Expand | SizerFlag::All, 8);
        panel.set_sizer(main_sizer, true);

        let frame_sizer = BoxSizer::builder(Orientation::Vertical).build();
        frame_sizer.add(&panel, 1, SizerFlag::Expand, 0);
        frame.set_sizer(frame_sizer, true);

        let save_cfg = cfg.clone();
        let sc = stack_cb;
        let fbc = fb_choice;
        let dsc = detail_cb;
        let save_frame = frame;
        save.on_click(move |_| {
            save_cfg.borrow_mut().settings.stack_counter = sc.get_value();
            save_cfg.borrow_mut().settings.feedback = match fbc.get_selection() {
                Some(1) => FeedbackMode::Text,
                Some(2) => FeedbackMode::Both,
                _ => FeedbackMode::Beeps,
            };
            save_cfg.borrow_mut().settings.detailed_status = dsc.get_value();
            let _ = save_cfg.borrow().save();
            save_frame.close(true);
        });
        let cancel_frame = frame;
        cancel.on_click(move |_| cancel_frame.close(true));

        frame.show(true);
        frame.centre();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_win_shift_letter() {
        let c = Chord { win: true, shift: true, key: "T".into(), ..Default::default() };
        assert_eq!(build_spec(&c, false), "#+t");
    }

    #[test]
    fn captures_function_key() {
        // WXK_F1 == 340, so F4 == 343
        assert_eq!(keycode_to_keyname(343).unwrap(), "F4");
        let c = Chord { win: true, key: "F4".into(), ..Default::default() };
        assert_eq!(build_spec(&c, false), "#{f4}");
    }

    #[test]
    fn bass_is_modifiers_only() {
        let c = Chord { ctrl: true, shift: true, key: "X".into(), ..Default::default() };
        assert_eq!(build_spec(&c, true), "^+");
    }

    #[test]
    fn roundtrips_spec() {
        for spec in ["#+t", "^+-", "#{f4}", "#{esc}", "^+"] {
            let bass = spec == "^+";
            let c = parse_spec(spec);
            assert_eq!(build_spec(&c, bass), spec, "roundtrip {spec}");
        }
    }

    #[test]
    fn display_is_readable() {
        assert_eq!(display_spec("#+t", false), "Win+Shift+T");
        assert_eq!(display_spec("#{f4}", false), "Win+F4");
        assert_eq!(display_spec("^+", true), "Ctrl+Shift");
    }
}
