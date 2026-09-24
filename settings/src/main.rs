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

use config::{has_modifiers, Config, FeedbackMode, Hotkeys};
use std::cell::RefCell;
use std::rc::Rc;
use wxdragon::prelude::*;

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Every row of the Keybindings list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Base,
    Slots,
    Priority,
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

/// What a row edits, which decides the shape of the Set Shortcut dialog.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The base modifier itself: modifiers only, and never empty.
    Base,
    /// Modifiers for a fixed set of keys (the slots, the priority keys).
    Mods(&'static str),
    /// A single shortcut: a key, with the base modifier or its own.
    Key,
}

const ROWS: &[(Act, &str, Kind)] = &[
    (Act::Base, "Base modifier", Kind::Base),
    (Act::Slots, "Hide/unhide slot", Kind::Mods("1–0")),
    (Act::Priority, "Process priority", Kind::Mods("F3–F8")),
    (Act::StackUp, "Next stack", Kind::Key),
    (Act::StackDown, "Previous stack", Kind::Key),
    (Act::FirstAvail, "Hide in first free slot", Kind::Key),
    (Act::Transparent, "Make window transparent", Kind::Key),
    (Act::Solid, "Make window solid", Kind::Key),
    (Act::ChTitle, "Change window title", Kind::Key),
    (Act::WinKill, "Kill active window", Kind::Key),
    (Act::ManageApp, "Auto-transparent active window's app", Kind::Key),
    (Act::UnmanageApp, "Stop auto-transparenting active window's app", Kind::Key),
    (Act::Status, "Speak how many windows are hidden", Kind::Key),
];

/// What a row is set to, independent of how config.toml stores it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Binding {
    /// No shortcut.
    Off,
    /// The base modifier with this key token (empty for a `Kind::Mods` row).
    Base(String),
    /// Its own modifiers (and key token, for a `Kind::Key` row).
    Own(String),
}

fn key_field(h: &mut Hotkeys, act: Act) -> Option<&mut String> {
    Some(match act {
        Act::StackUp => &mut h.stackup,
        Act::StackDown => &mut h.stackdown,
        Act::FirstAvail => &mut h.firstavailhide,
        Act::Transparent => &mut h.transparent,
        Act::Solid => &mut h.solid,
        Act::ChTitle => &mut h.chtitle,
        Act::WinKill => &mut h.winkill,
        Act::ManageApp => &mut h.manageapp,
        Act::UnmanageApp => &mut h.unmanageapp,
        Act::Status => &mut h.status,
        Act::Base | Act::Slots | Act::Priority => return None,
    })
}

fn mods_field(h: &mut Hotkeys, act: Act) -> Option<&mut Option<String>> {
    match act {
        Act::Slots => Some(&mut h.slots),
        Act::Priority => Some(&mut h.priority),
        _ => None,
    }
}

fn get_binding(h: &Hotkeys, act: Act) -> Binding {
    let mut h = h.clone();
    if act == Act::Base {
        return Binding::Own(h.base);
    }
    if let Some(own) = mods_field(&mut h, act) {
        return match own.as_deref() {
            None => Binding::Base(String::new()),
            Some("") => Binding::Off,
            Some(m) => Binding::Own(m.to_string()),
        };
    }
    let spec = key_field(&mut h, act).map(|s| s.clone()).unwrap_or_default();
    if spec.is_empty() {
        Binding::Off
    } else if has_modifiers(&spec) {
        Binding::Own(spec)
    } else {
        Binding::Base(spec)
    }
}

fn set_binding(h: &mut Hotkeys, act: Act, b: Binding) {
    if act == Act::Base {
        if let Binding::Own(m) = b {
            h.base = m;
        }
    } else if let Some(own) = mods_field(h, act) {
        *own = match b {
            Binding::Base(_) => None,
            Binding::Off => Some(String::new()),
            Binding::Own(m) => Some(m),
        };
    } else if let Some(field) = key_field(h, act) {
        *field = match b {
            Binding::Off => String::new(),
            Binding::Base(key) | Binding::Own(key) => key,
        };
    }
}

fn default_binding(act: Act) -> Binding {
    get_binding(&Hotkeys::default(), act)
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
    /// Human key name, e.g. "T", "F4", "Escape", "\". Empty for modifier-only rows.
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

/// Build a NUtils spec from a [`Chord`]. With `mods_only`, the key is ignored.
fn build_spec(c: &Chord, mods_only: bool) -> String {
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
    if !mods_only && !c.key.is_empty() {
        s.push_str(&keyname_to_token(&c.key));
    }
    s
}

/// A human-readable rendering, e.g. "Win+Shift+T", or just the modifiers
/// ("Ctrl+Shift") when `mods_only`. Empty when nothing is set.
fn display_chord(c: &Chord, mods_only: bool) -> String {
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
    if !mods_only && !c.key.is_empty() {
        parts.push(c.key.clone());
    }
    parts.join("+")
}

fn display_spec(spec: &str, mods_only: bool) -> String {
    display_chord(&parse_spec(spec), mods_only)
}

/// The shortcut a row stands for, as the user would press it — e.g.
/// "Shift+Alt+T", "Ctrl+Shift+1–0, own shortcut" or "none".
fn describe(h: &Hotkeys, act: Act, kind: Kind) -> String {
    let binding = get_binding(h, act);
    let spec = match &binding {
        Binding::Off => return "none".into(),
        Binding::Base(key) => format!("{}{key}", h.base),
        Binding::Own(spec) => spec.clone(),
    };
    let text = match kind {
        Kind::Base => display_spec(&spec, true),
        Kind::Mods(keys) => format!("{}+{keys}", display_spec(&spec, true)),
        Kind::Key => display_spec(&spec, false),
    };
    match binding {
        Binding::Own(_) if kind != Kind::Base => format!("{text}, own shortcut"),
        _ => text,
    }
}

fn list_item(cfg: &Config, act: Act, label: &str, kind: Kind) -> String {
    format!("{label}: {}", describe(&cfg.hotkeys, act, kind))
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

/// What the Set Shortcut dialog decided.
enum Outcome {
    Set(Binding),
    ResetToDefault,
}

const ID_CLEAR: i32 = ID_HIGHEST + 1;
const ID_RESET: i32 = ID_HIGHEST + 2;

/// Edit one row. Returns `None` when cancelled.
fn prompt_for_shortcut(
    parent: &Frame,
    action_label: &str,
    kind: Kind,
    current: &Binding,
    base: &str,
) -> Option<Outcome> {
    let title = format!("Set Shortcut: {action_label}");
    let dialog = Dialog::builder(parent, &title).with_size(440, 420).build();
    #[cfg(windows)]
    let live = live_region::LiveRegion::new(&dialog);
    let panel = Panel::builder(&dialog).build();
    let main_sizer = BoxSizer::builder(Orientation::Vertical).build();

    let info = match kind {
        Kind::Base => "Choose the base modifier. Every shortcut uses it unless it has its own.".to_string(),
        Kind::Mods(keys) => format!("Choose the modifiers held with {keys} for {action_label}:"),
        Kind::Key => format!("Configure the shortcut for {action_label}:"),
    };
    let info_label = StaticText::builder(&panel).with_label(&info).build();
    main_sizer.add(&info_label, 0, SizerFlag::Expand | SizerFlag::All, 8);

    // "Use the base modifier" — every row but the base itself. While checked, the
    // Modifiers list shows the base's modifiers and is disabled.
    let base_text = display_spec(base, true);
    let use_base = CheckBox::builder(&panel)
        .with_label(&format!("&Use the base modifier ({base_text})"))
        .build();
    let (start_base, start) = match current {
        Binding::Own(spec) => (false, parse_spec(spec)),
        Binding::Base(key) => (true, parse_spec(&format!("{base}{key}"))),
        Binding::Off => (true, parse_spec(base)),
    };
    use_base.set_value(start_base);
    if kind == Kind::Base {
        use_base.show(false);
    } else {
        main_sizer.add(&use_base, 0, SizerFlag::All, 8);
    }

    // Modifiers as a checkable list (check the ones you want).
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
    mods.enable(kind == Kind::Base || !start_base);
    main_sizer.add(&mods, 0, SizerFlag::Expand | SizerFlag::All, 8);

    // Key field (only for a single shortcut).
    let key_field = TextCtrl::builder(&panel)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::DontWrap)
        .build();
    if kind == Kind::Key {
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
    } else {
        key_field.show(false);
    }

    let preview = StaticText::builder(&panel).with_label("").build();
    main_sizer.add(&preview, 0, SizerFlag::Expand | SizerFlag::All, 8);

    // The shortcut as currently set in the dialog. With "use base" checked the
    // modifiers are the base's, whatever the (disabled) list shows.
    let read_chord = {
        let base = base.to_string();
        move || {
            let mut c = if kind != Kind::Base && use_base.get_value() {
                parse_spec(&base)
            } else {
                Chord {
                    ctrl: mods.is_checked(0),
                    shift: mods.is_checked(1),
                    win: mods.is_checked(2),
                    alt: mods.is_checked(3),
                    ..Default::default()
                }
            };
            c.key = if kind == Kind::Key { key_field.get_value() } else { String::new() };
            c
        }
    };

    // Updates the preview label and announces the shortcut to screen readers.
    let update_preview = {
        let read_chord = read_chord.clone();
        #[cfg(windows)]
        let live = live.clone();
        move || {
            let chord = read_chord();
            let shown = display_chord(&chord, kind != Kind::Key);
            let text = match kind {
                _ if shown.is_empty() => "none".to_string(),
                Kind::Mods(keys) => format!("{shown}+{keys}"),
                Kind::Key if chord.key.is_empty() => "none".to_string(),
                _ => shown,
            };
            preview.set_label(&format!("Shortcut: {text}"));
            #[cfg(windows)]
            live.announce(&format!("Shortcut: {text}"));
        }
    };
    update_preview();

    // Key capture: press a key in the key field to set ONLY the key. Modifiers come
    // from the base or the Modifiers list, so a keypress must not touch them.
    if kind == Kind::Key {
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
                    key_field.set_value(&name);
                    upd();
                    event.skip(false);
                    return;
                }
            }
            event.skip(true);
        });
    }

    // Toggling "use base" switches the Modifiers list between showing the base
    // (disabled) and the row's own modifiers (enabled, starting from the base).
    {
        let upd = update_preview.clone();
        use_base.on_toggled(move |_| {
            mods.enable(!use_base.get_value());
            upd();
        });
        let upd = update_preview.clone();
        mods.on_toggled(move |_| upd());
    }

    // Buttons: Clear and Reset to Default act at once and close the dialog;
    // OK / Cancel as usual.
    let button_sizer = BoxSizer::builder(Orientation::Horizontal).build();
    if kind != Kind::Base {
        let clear = Button::builder(&panel).with_label("&Clear").build();
        clear.on_click(move |_| dialog.end_modal(ID_CLEAR));
        button_sizer.add(&clear, 0, SizerFlag::Right, 8);
    }
    let reset = Button::builder(&panel).with_label("&Reset to Default").build();
    reset.on_click(move |_| dialog.end_modal(ID_RESET));
    button_sizer.add(&reset, 0, SizerFlag::Right, 8);
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
    match kind {
        Kind::Key => key_field.set_focus(),
        Kind::Mods(_) => use_base.set_focus(),
        Kind::Base => mods.set_focus(),
    }

    loop {
        match dialog.show_modal() {
            ID_OK => {
                let chord = read_chord();
                let with_base = kind != Kind::Base && use_base.get_value();
                // An empty key removes the shortcut — that's allowed.
                if kind == Kind::Key && chord.key.trim().is_empty() {
                    return Some(Outcome::Set(Binding::Off));
                }
                // Modifiers are needed so NUtils never takes a plain key from every
                // app; refuse and re-open so the user can add one.
                if !(chord.ctrl || chord.shift || chord.win || chord.alt) {
                    let msg = "Choose at least one modifier: Ctrl, Shift, Windows, or Alt.";
                    info_label.set_label(msg);
                    #[cfg(windows)]
                    live.announce(msg);
                    continue;
                }
                let binding = match kind {
                    Kind::Base => Binding::Own(build_spec(&chord, true)),
                    Kind::Mods(_) if with_base => Binding::Base(String::new()),
                    Kind::Mods(_) => Binding::Own(build_spec(&chord, true)),
                    Kind::Key if with_base => Binding::Base(keyname_to_token(&chord.key)),
                    Kind::Key => Binding::Own(build_spec(&chord, false)),
                };
                return Some(Outcome::Set(binding));
            }
            ID_CLEAR => return Some(Outcome::Set(Binding::Off)),
            ID_RESET => return Some(Outcome::ResetToDefault),
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

        let visual_cb = CheckBox::builder(&general)
            .with_label("&Visual check: confirm on screen that a window really became transparent (needs Screen Curtain off)")
            .build();
        visual_cb.set_value(cfg.borrow().settings.visual_check);
        gsizer.add(&visual_cb, 0, SizerFlag::All, 8);

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
        for (act, label, kind) in ROWS {
            list.append(&list_item(&cfg.borrow(), *act, label, *kind));
        }
        list.set_selection(0, true);
        ksizer.add(&list, 1, SizerFlag::Expand | SizerFlag::All, 8);

        // Clearing or resetting one shortcut lives in its Set Shortcut dialog.
        let btns = BoxSizer::builder(Orientation::Horizontal).build();
        let set_btn = Button::builder(&keys).with_label("&Set Shortcut...").build();
        let reset_all_btn = Button::builder(&keys).with_label("Reset &All").build();
        btns.add(&set_btn, 0, SizerFlag::Right, 8);
        btns.add(&reset_all_btn, 0, SizerFlag::Right, 8);
        ksizer.add_sizer(&btns, 0, SizerFlag::Expand | SizerFlag::All, 8);

        keys.set_sizer(ksizer, true);
        notebook.add_page(&keys, "Keybindings", false, None);

        // Rebuild the list from the current config, keeping the selection. Every
        // row is rebuilt, since changing the base changes most of them.
        let refresh = {
            let cfg = cfg.clone();
            let list = list;
            move || {
                let sel = list.get_selection().unwrap_or(0);
                list.clear();
                for (act, label, kind) in ROWS {
                    list.append(&list_item(&cfg.borrow(), *act, label, *kind));
                }
                let count = list.get_count();
                list.set_selection(if sel < count { sel } else { 0 }, true);
            }
        };

        // Set Shortcut (button, or Enter / double-click on a row).
        let do_set = {
            let cfg = cfg.clone();
            let frame = frame;
            let refresh = refresh.clone();
            move || {
                let Some(&(act, label, kind)) =
                    list.get_selection().and_then(|sel| ROWS.get(sel as usize))
                else {
                    return;
                };
                let (current, base) = {
                    let c = cfg.borrow();
                    (get_binding(&c.hotkeys, act), c.hotkeys.base.clone())
                };
                let binding = match prompt_for_shortcut(&frame, label, kind, &current, &base) {
                    Some(Outcome::Set(b)) => b,
                    Some(Outcome::ResetToDefault) => default_binding(act),
                    None => return,
                };
                set_binding(&mut cfg.borrow_mut().hotkeys, act, binding);
                refresh();
            }
        };
        let set_click = do_set.clone();
        set_btn.on_click(move |_| set_click());
        let set_dclick = do_set;
        list.on_item_double_clicked(move |_| set_dclick());

        // Reset all to defaults (with confirmation).
        let reset_all_cfg = cfg.clone();
        let reset_all_refresh = refresh.clone();
        let reset_all_frame = frame;
        reset_all_btn.on_click(move |_| {
            let warn = MessageDialog::builder(
                &reset_all_frame,
                "Reset all keyboard shortcuts, including the base modifier, to their default values?",
                "Reset Shortcuts",
            )
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
            .build();
            if warn.show_modal() == ID_YES {
                reset_all_cfg.borrow_mut().hotkeys = Hotkeys::default();
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
        let vsc = visual_cb;
        let save_frame = frame;
        save.on_click(move |_| {
            save_cfg.borrow_mut().settings.stack_counter = sc.get_value();
            save_cfg.borrow_mut().settings.feedback = match fbc.get_selection() {
                Some(1) => FeedbackMode::Text,
                Some(2) => FeedbackMode::Both,
                _ => FeedbackMode::Beeps,
            };
            save_cfg.borrow_mut().settings.detailed_status = dsc.get_value();
            save_cfg.borrow_mut().settings.visual_check = vsc.get_value();
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
    fn modifiers_only_ignores_the_key() {
        let c = Chord { ctrl: true, shift: true, key: "X".into(), ..Default::default() };
        assert_eq!(build_spec(&c, true), "^+");
    }

    #[test]
    fn roundtrips_spec() {
        for spec in ["#+t", "^+-", "#{f4}", "#{esc}", "^+"] {
            let mods_only = spec == "^+";
            let c = parse_spec(spec);
            assert_eq!(build_spec(&c, mods_only), spec, "roundtrip {spec}");
        }
    }

    #[test]
    fn display_is_readable() {
        assert_eq!(display_spec("#+t", false), "Win+Shift+T");
        assert_eq!(display_spec("#{f4}", false), "Win+F4");
        assert_eq!(display_spec("^+", true), "Ctrl+Shift");
    }

    #[test]
    fn defaults_all_use_the_base() {
        let h = Hotkeys::default();
        for (act, _, kind) in ROWS {
            let b = get_binding(&h, *act);
            match kind {
                Kind::Base => assert_eq!(b, Binding::Own("!+".into())),
                _ => assert!(matches!(b, Binding::Base(_)), "a default that isn't base + key"),
            }
        }
        assert_eq!(describe(&h, Act::ChTitle, Kind::Key), "Alt+Shift+T");
        assert_eq!(describe(&h, Act::Slots, Kind::Mods("1–0")), "Alt+Shift+1–0");
        assert_eq!(describe(&h, Act::Base, Kind::Base), "Alt+Shift");
    }

    #[test]
    fn bindings_round_trip_through_the_config() {
        let mut h = Hotkeys::default();
        for (act, _, kind) in ROWS {
            let cases: &[Binding] = match kind {
                Kind::Base => &[],
                Kind::Mods(_) => &[Binding::Off, Binding::Base(String::new())],
                Kind::Key => &[Binding::Off, Binding::Base("q".into())],
            };
            for b in cases.iter().cloned().chain([Binding::Own("^#".into())]) {
                let b = match (kind, b) {
                    (Kind::Key, Binding::Own(m)) => Binding::Own(format!("{m}q")),
                    (_, b) => b,
                };
                set_binding(&mut h, *act, b.clone());
                assert_eq!(get_binding(&h, *act), b);
            }
        }
    }

    #[test]
    fn own_shortcut_is_marked_and_ignores_the_base() {
        let mut h = Hotkeys::default();
        set_binding(&mut h, Act::ChTitle, Binding::Own("^+t".into()));
        set_binding(&mut h, Act::Base, Binding::Own("#".into()));
        assert_eq!(describe(&h, Act::ChTitle, Kind::Key), "Ctrl+Shift+T, own shortcut");
        assert_eq!(describe(&h, Act::Status, Kind::Key), "Win+I");
        set_binding(&mut h, Act::Priority, Binding::Off);
        assert_eq!(describe(&h, Act::Priority, Kind::Mods("F3–F8")), "none");
    }
}
