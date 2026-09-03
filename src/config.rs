//! Configuration: modern TOML config with one-time migration from the legacy
//! `hotkeys.ini` / `settings.ini` / `WinMurderer.ini` files.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// Hotkey bindings. Values use the original NUtils modifier syntax so existing
/// muscle memory carries over: `^`=Ctrl, `+`=Shift, `#`=Win, `!`=Alt, and
/// `{f4}` / `{esc}` for named keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotkeys {
    /// Base modifiers held with a digit 1-0 to hide/unhide that slot.
    pub bass: String,
    pub stackdown: String,
    pub stackup: String,
    pub chtitle: String,
    pub transparent: String,
    pub solid: String,
    pub firstavailhide: String,
    pub winkill: String,
    /// Add the active window's app to the auto-transparent list.
    #[serde(default = "default_manageapp")]
    pub manageapp: String,
    /// Stop auto-transparenting the active window's app (and make it solid again).
    #[serde(default = "default_unmanageapp")]
    pub unmanageapp: String,
}

fn default_manageapp() -> String {
    "#+a".into()
}
fn default_unmanageapp() -> String {
    "#+s".into()
}

impl Default for Hotkeys {
    fn default() -> Self {
        Hotkeys {
            bass: "^+".into(),
            stackdown: "^+-".into(),
            stackup: "^+=".into(),
            chtitle: "#+t".into(),
            transparent: "#+\\".into(),
            solid: "#+/".into(),
            firstavailhide: "#+h".into(),
            winkill: "#{f4}".into(),
            manageapp: "#+a".into(),
            unmanageapp: "#+s".into(),
        }
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
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            stack_counter: true,
            feedback: FeedbackMode::Beeps,
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

/// An application whose newly-shown windows are auto-made-transparent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedApp {
    #[serde(default, rename = "match")]
    pub match_kind: MatchKind,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub hotkeys: Hotkeys,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub managed_apps: Vec<ManagedApp>,
}

impl Config {
    /// `%APPDATA%\NUtils\config.toml`, falling back to the executable's directory.
    pub fn config_path() -> PathBuf {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return Path::new(&appdata).join("NUtils").join("config.toml");
        }
        PathBuf::from("config.toml")
    }

    /// Load config, creating a default file (or migrating legacy `.ini`s) on first run.
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

        // No TOML yet: migrate legacy ini files if any exist next to the exe.
        let cfg = migrate_legacy().unwrap_or_default();
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .unwrap_or_else(|_| "# failed to serialize config\n".into());
        std::fs::write(path, text)
    }
}

/// Minimal INI reader for legacy migration: `section -> key -> value`.
fn parse_ini(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut current = String::new();
    sections.push((current.clone(), Vec::new()));
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].to_string();
            sections.push((current.clone(), Vec::new()));
        } else if let Some((k, v)) = line.split_once('=') {
            if let Some(sec) = sections.last_mut() {
                sec.1.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }
    sections
}

fn ini_get<'a>(
    secs: &'a [(String, Vec<(String, String)>)],
    section: &str,
    key: &str,
) -> Option<&'a str> {
    secs.iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(section))
        .and_then(|(_, kv)| {
            kv.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.as_str())
        })
}

/// Read legacy `hotkeys.ini` / `settings.ini` / `WinMurderer.ini` from the exe dir.
fn migrate_legacy() -> Option<Config> {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let hk_path = dir.join("hotkeys.ini");
    let set_path = dir.join("settings.ini");
    let wm_path = dir.join("WinMurderer.ini");
    if !hk_path.exists() && !set_path.exists() && !wm_path.exists() {
        return None;
    }

    let mut cfg = Config::default();

    if let Ok(t) = std::fs::read_to_string(&hk_path) {
        let s = parse_ini(&t);
        let g = |k: &str, d: &str| ini_get(&s, "hotkeys", k).unwrap_or(d).to_string();
        cfg.hotkeys = Hotkeys {
            bass: g("bass", &cfg.hotkeys.bass),
            stackdown: g("stackdown", &cfg.hotkeys.stackdown),
            stackup: g("stackup", &cfg.hotkeys.stackup),
            chtitle: g("chtitle", &cfg.hotkeys.chtitle),
            transparent: g("transparent", &cfg.hotkeys.transparent),
            solid: g("solid", &cfg.hotkeys.solid),
            firstavailhide: g("firstavailhide", &cfg.hotkeys.firstavailhide),
            winkill: g("winkill", &cfg.hotkeys.winkill),
            manageapp: g("manageapp", &cfg.hotkeys.manageapp),
            unmanageapp: g("unmanageapp", &cfg.hotkeys.unmanageapp),
        };
    }

    if let Ok(t) = std::fs::read_to_string(&set_path) {
        let s = parse_ini(&t);
        if let Some(sc) = ini_get(&s, "settings", "StackCounter") {
            cfg.settings.stack_counter = sc.trim() != "0";
        }
    }

    if let Ok(t) = std::fs::read_to_string(&wm_path) {
        let s = parse_ini(&t);
        for (name, kv) in &s {
            if name.is_empty() {
                continue;
            }
            let title = kv
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("title"))
                .map(|(_, v)| v.clone());
            let degree = kv
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("degree"))
                .map(|(_, v)| v.clone());
            if let Some(title) = title {
                let degree = match degree.as_deref() {
                    Some("2") => Degree::Kill,
                    _ => Degree::Close,
                };
                cfg.rules.push(Rule {
                    match_kind: MatchKind::Title,
                    value: title,
                    degree,
                });
            }
        }
    }

    Some(cfg)
}
