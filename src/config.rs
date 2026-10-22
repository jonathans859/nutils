//! Configuration: `config.toml`, shared by the core and the settings editor.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How a managed-app / rule pattern is matched against a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    /// Match on the owning process image name, e.g. `wxdragon.exe` (case-insensitive).
    Exe,
    /// Substring match on the window title.
    Title,
    /// Exact match on the window class name.
    Class,
}

impl Default for MatchKind {
    fn default() -> Self {
        MatchKind::Exe
    }
}

/// What to do to a window that matches a WinMurderer rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Degree {
    /// Politely ask the window to close (WM_CLOSE).
    Close,
    /// Forcefully terminate the owning process.
    Kill,
}

/// Hotkey bindings, in NUtils' modifier syntax: `^`=Ctrl, `+`=Shift, `#`=Win,
/// `!`=Alt, then a key such as `t`, `\` or a braced name like `{f4}`.
///
/// Every shortcut uses the `base` modifiers unless it names its own. An action's
/// binding is one of:
/// - a bare key (`"t"`): base + that key — the default for every action;
/// - modifiers and a key (`"^+t"`): its own shortcut, ignoring the base;
/// - empty (`""`): no shortcut.
///
/// The slot keys (1–0) and priority keys (F3–F8) are fixed; only their modifiers
/// can be changed: left out means the base, `""` turns them off.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub base: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slots: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    pub stackdown: String,
    pub stackup: String,
    pub chtitle: String,
    pub transparent: String,
    pub solid: String,
    pub firstavailhide: String,
    pub winkill: String,
    /// Add the active window's app to the auto-transparent list.
    pub manageapp: String,
    /// Stop auto-transparenting the active window's app (and make it solid again).
    pub unmanageapp: String,
    /// Speak how many windows are hidden in how many stacks.
    pub status: String,
    /// Speak the position and title of each window hidden in the current stack.
    pub stackstatus: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Hotkeys {
            base: "!+".into(),
            slots: None,
            priority: None,
            stackdown: "-".into(),
            stackup: "=".into(),
            chtitle: "t".into(),
            transparent: "\\".into(),
            solid: "/".into(),
            firstavailhide: "h".into(),
            // Its own shortcut: base + F4 is the "Below normal" priority key.
            winkill: "#{f4}".into(),
            manageapp: "a".into(),
            unmanageapp: "s".into(),
            status: "i".into(),
            stackstatus: "c".into(),
        }
    }
}

/// Whether a binding names its own modifiers (and so ignores the base).
pub fn has_modifiers(spec: &str) -> bool {
    spec.starts_with(['^', '+', '#', '!'])
}

impl Hotkeys {
    /// The full shortcut an action binding stands for, or `None` for no shortcut.
    /// A bare key needs a base with at least one modifier, so NUtils never grabs
    /// a plain key from every app.
    pub fn resolve(&self, binding: &str) -> Option<String> {
        if binding.is_empty() {
            None
        } else if has_modifiers(binding) {
            Some(binding.to_string())
        } else if has_modifiers(&self.base) {
            Some(format!("{}{}", self.base, binding))
        } else {
            None
        }
    }

    /// Modifiers held with the slot keys 1–0, or `None` when they are off.
    pub fn slot_mods(&self) -> Option<&str> {
        Self::fixed_key_mods(self.slots.as_deref(), &self.base)
    }

    /// Modifiers held with the priority keys F3–F8, or `None` when they are off.
    pub fn priority_mods(&self) -> Option<&str> {
        Self::fixed_key_mods(self.priority.as_deref(), &self.base)
    }

    fn fixed_key_mods<'a>(own: Option<&'a str>, base: &'a str) -> Option<&'a str> {
        let mods = own.unwrap_or(base);
        has_modifiers(mods).then_some(mods)
    }

    /// Every shortcut that is switched on, with its full spec, in the order
    /// NUtils registers them: when two share a key, the earlier one gets it.
    pub fn shortcuts(&self) -> Vec<(Action, String)> {
        let mut out = Vec::new();
        if let Some(mods) = self.slot_mods() {
            for d in 0..10 {
                out.push((Action::Slot(d), format!("{mods}{d}")));
            }
        }
        if let Some(mods) = self.priority_mods() {
            for i in 0..6 {
                out.push((Action::Priority(i), format!("{mods}{{f{}}}", i + 3)));
            }
        }
        for (action, binding) in [
            (Action::ChTitle, &self.chtitle),
            (Action::Transparent, &self.transparent),
            (Action::Solid, &self.solid),
            (Action::FirstAvail, &self.firstavailhide),
            (Action::WinKill, &self.winkill),
            (Action::ManageApp, &self.manageapp),
            (Action::UnmanageApp, &self.unmanageapp),
            (Action::StackUp, &self.stackup),
            (Action::StackDown, &self.stackdown),
            (Action::Status, &self.status),
            (Action::StackStatus, &self.stackstatus),
        ] {
            if let Some(spec) = self.resolve(binding) {
                out.push((action, spec));
            }
        }
        out
    }

    /// Actions set to the same shortcut as an earlier one, which then never run.
    pub fn clashes(&self) -> Vec<Clash> {
        let all = self.shortcuts();
        let mut out = Vec::new();
        for (i, (action, spec)) in all.iter().enumerate() {
            let key = shortcut_key(spec);
            if let Some((first, _)) = all[..i].iter().find(|(_, s)| shortcut_key(s) == key) {
                out.push(Clash {
                    spec: spec.clone(),
                    first: *first,
                    second: *action,
                });
            }
        }
        out
    }
}

/// A spec reduced to what Windows sees: the set of modifiers and the key, so
/// `"!+t"` and `"+!T"` compare equal.
fn shortcut_key(spec: &str) -> (Vec<char>, String) {
    let key = spec.trim_start_matches(['^', '+', '#', '!']);
    let mut mods: Vec<char> = spec[..spec.len() - key.len()].chars().collect();
    mods.sort_unstable();
    mods.dedup();
    (mods, key.to_ascii_lowercase())
}

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Hide or unhide the slot on this digit key (0–9).
    Slot(u32),
    /// Set the active process's priority, 0 (low) to 5 (realtime): F3–F8.
    Priority(usize),
    ChTitle,
    Transparent,
    Solid,
    FirstAvail,
    WinKill,
    ManageApp,
    UnmanageApp,
    StackUp,
    StackDown,
    Status,
    StackStatus,
}

pub const PRIORITY_NAMES: [&str; 6] = [
    "Low priority",
    "Below normal priority",
    "Normal priority",
    "Above normal priority",
    "High priority",
    "Realtime priority",
];

impl Action {
    /// The action's name, as the settings program lists it.
    pub fn label(self) -> String {
        match self {
            Action::Slot(d) => format!("Hide/unhide slot {d}"),
            Action::Priority(i) => PRIORITY_NAMES[i].to_string(),
            Action::ChTitle => "Change window title".into(),
            Action::Transparent => "Make window transparent".into(),
            Action::Solid => "Make window solid".into(),
            Action::FirstAvail => "Hide in first free slot".into(),
            Action::WinKill => "Kill active window".into(),
            Action::ManageApp => "Auto-transparent active window's app".into(),
            Action::UnmanageApp => "Stop auto-transparenting active window's app".into(),
            Action::StackUp => "Next stack".into(),
            Action::StackDown => "Previous stack".into(),
            Action::Status => "Speak how many windows are hidden".into(),
            Action::StackStatus => "Speak the windows hidden in the current stack".into(),
        }
    }
}

/// Two actions set to the same shortcut: only `first` gets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clash {
    pub spec: String,
    pub first: Action,
    pub second: Action,
}

/// How NUtils reports actions back to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedbackMode {
    /// PC-speaker beeps / sound pack only.
    Beeps,
    /// Spoken/screen-reader text only (via Prism).
    Text,
    /// Both beeps and text.
    Both,
}

impl Default for FeedbackMode {
    fn default() -> Self {
        FeedbackMode::Beeps
    }
}

impl FeedbackMode {
    pub fn beeps(self) -> bool {
        matches!(self, FeedbackMode::Beeps | FeedbackMode::Both)
    }
    pub fn text(self) -> bool {
        matches!(self, FeedbackMode::Text | FeedbackMode::Both)
    }
}

/// Which releases update checks look for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Tagged releases (v4.0.0, ...).
    #[default]
    Stable,
    /// The development build of the latest change on master.
    Dev,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// `true` = beep N times when switching to stack N; `false` = use number pack.
    #[serde(default = "default_true")]
    pub stack_counter: bool,
    /// Beeps, spoken text, or both.
    #[serde(default)]
    pub feedback: FeedbackMode,
    /// `true` = the status hotkey also lists each hidden window's stack, position
    /// and title; `false` = it announces only the counts.
    #[serde(default)]
    pub detailed_status: bool,
    /// `true` = when a hotkey makes a window transparent, also compare the screen
    /// before and after, to catch windows that stay drawn anyway. Needs Screen
    /// Curtain off; with it on the check is skipped.
    #[serde(default)]
    pub visual_check: bool,
    /// `true` = check GitHub for a newer release when NUtils starts, and show it
    /// in the tray. Checking by hand works either way.
    #[serde(default = "default_true")]
    pub check_for_updates: bool,
    /// Stable releases, or development builds of every change.
    #[serde(default)]
    pub update_channel: UpdateChannel,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            stack_counter: true,
            feedback: FeedbackMode::Beeps,
            detailed_status: false,
            visual_check: false,
            check_for_updates: true,
            update_channel: UpdateChannel::Stable,
        }
    }
}

/// A WinMurderer watch-list entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default, rename = "match")]
    pub match_kind: MatchKind,
    /// The value to match (title text by default, to preserve legacy behavior).
    pub value: String,
    pub degree: Degree,
}

impl Rule {
    /// Whether a window matches, given a way to read its title, class or
    /// program. Case-insensitive; an empty value never matches.
    pub fn matches(&self, field: impl FnOnce(MatchKind) -> Option<String>) -> bool {
        let needle = self.value.to_ascii_lowercase();
        if needle.is_empty() {
            return false;
        }
        let Some(have) = field(self.match_kind) else {
            return false;
        };
        let have = have.to_ascii_lowercase();
        match self.match_kind {
            MatchKind::Title => have.contains(&needle),
            MatchKind::Class | MatchKind::Exe => have == needle,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub hotkeys: Hotkeys,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// The folder `nutils.exe` (or `nutils-settings.exe`) lives in.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Config {
    /// `config.toml`, always beside the executable: NUtils is portable, so the
    /// app and everything it keeps travel in one folder.
    pub fn config_path() -> PathBuf {
        exe_dir().join("config.toml")
    }

    /// Load config, creating a default file on first run.
    pub fn load_or_init() -> Config {
        let path = Self::config_path();
        if let Ok(text) = std::fs::read_to_string(&path) {
            match toml::from_str::<Config>(&text) {
                Ok(cfg) => return cfg,
                Err(e) => {
                    eprintln!("nutils: config parse error ({e}); using defaults");
                    return Config::default();
                }
            }
        }

        let cfg = Config::default();
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self)
            .unwrap_or_else(|_| "# failed to serialize config\n".into());
        std::fs::write(Self::config_path(), text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_no_clashes() {
        assert_eq!(Hotkeys::default().clashes(), vec![]);
    }

    #[test]
    fn kill_on_base_f4_clashes_with_a_priority_key() {
        let h = Hotkeys {
            winkill: "{f4}".into(),
            ..Hotkeys::default()
        };
        assert_eq!(
            h.clashes(),
            vec![Clash {
                spec: "!+{f4}".into(),
                first: Action::Priority(1),
                second: Action::WinKill,
            }]
        );
    }

    #[test]
    fn a_windows_base_clashes_with_the_default_kill_key() {
        let h = Hotkeys {
            base: "#".into(),
            ..Hotkeys::default()
        };
        let clashes = h.clashes();
        assert_eq!(clashes.len(), 1);
        assert_eq!((clashes[0].first, clashes[0].second), (Action::Priority(1), Action::WinKill));
    }

    #[test]
    fn modifier_order_and_key_case_do_not_hide_a_clash() {
        let h = Hotkeys {
            chtitle: "+!T".into(),
            ..Hotkeys::default()
        };
        assert_eq!(h.clashes().len(), 0);
        let h = Hotkeys {
            solid: "+!T".into(),
            ..Hotkeys::default()
        };
        assert_eq!(
            h.clashes(),
            vec![Clash {
                spec: "+!T".into(),
                first: Action::ChTitle,
                second: Action::Solid,
            }]
        );
    }

    #[test]
    fn bare_key_uses_the_base() {
        let hk = Hotkeys::default();
        assert_eq!(hk.resolve("t").as_deref(), Some("!+t"));
        assert_eq!(hk.resolve("{f4}").as_deref(), Some("!+{f4}"));
    }

    #[test]
    fn own_modifiers_override_the_base() {
        assert_eq!(Hotkeys::default().resolve("^+t").as_deref(), Some("^+t"));
    }

    #[test]
    fn empty_binding_or_empty_base_registers_nothing() {
        let mut hk = Hotkeys::default();
        assert_eq!(hk.resolve(""), None);
        hk.base.clear();
        assert_eq!(hk.resolve("t"), None, "never a bare key on its own");
        assert_eq!(hk.resolve("#t").as_deref(), Some("#t"));
    }

    #[test]
    fn slot_and_priority_modifiers() {
        let mut hk = Hotkeys::default();
        assert_eq!(hk.slot_mods(), Some("!+"));
        hk.slots = Some("^+".into());
        hk.priority = Some(String::new());
        assert_eq!(hk.slot_mods(), Some("^+"));
        assert_eq!(hk.priority_mods(), None);
    }

    #[test]
    fn sample_config_matches_the_defaults() {
        let sample: Config = toml::from_str(include_str!("../config.toml")).unwrap();
        let (a, b) = (sample.hotkeys, Hotkeys::default());
        assert_eq!(
            toml::to_string(&a).unwrap(),
            toml::to_string(&b).unwrap(),
            "config.toml's [hotkeys] should show the built-in defaults"
        );
    }

    #[test]
    fn rules_match_like_winmurderer() {
        let rule = |match_kind, value: &str| Rule { match_kind, value: value.into(), degree: Degree::Close };
        let window = |kind| match kind {
            MatchKind::Title => Some("Untitled - Notepad".to_string()),
            MatchKind::Class => Some("Notepad".to_string()),
            MatchKind::Exe => Some("notepad.exe".to_string()),
        };
        assert!(rule(MatchKind::Title, "untitled").matches(window));
        assert!(rule(MatchKind::Class, "NOTEPAD").matches(window));
        assert!(!rule(MatchKind::Class, "Note").matches(window), "a class must match in full");
        assert!(rule(MatchKind::Exe, "Notepad.exe").matches(window));
        assert!(!rule(MatchKind::Title, "").matches(window), "an empty value matches nothing");
        assert!(!rule(MatchKind::Exe, "notepad.exe").matches(|_| None), "an unknown program");
    }
}
