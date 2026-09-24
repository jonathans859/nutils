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
            winkill: "{f4}".into(),
            manageapp: "a".into(),
            unmanageapp: "s".into(),
            status: "i".into(),
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
}
