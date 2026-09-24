//! `state.toml`: what NUtils records as it runs — the auto-transparent apps and
//! the hidden windows — kept apart from the settings in `config.toml`. The
//! settings are only written by the user and the settings editor; this file only
//! by NUtils. Both sit next to `nutils.exe`.

use crate::config::{exe_dir, MatchKind};
use crate::window::WinId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// An application whose windows are made transparent automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedApp {
    #[serde(default, rename = "match")]
    pub match_kind: MatchKind,
    pub value: String,
}

/// The hidden windows, valid only until the next reboot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hidden {
    /// Approximate system boot time (unix seconds); the windows are discarded if
    /// this no longer matches, since window handles are meaningless after a reboot.
    pub boot_epoch: i64,
    pub slots: Vec<WinId>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    /// Kept until the user stops auto-transparenting the app.
    #[serde(default)]
    pub managed_apps: Vec<ManagedApp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<Hidden>,
}

fn path() -> PathBuf {
    exe_dir().join("state.toml")
}

impl State {
    /// The saved state, or an empty one if there is none (or it can't be read).
    pub fn load() -> State {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(
                path(),
                format!("# Written by NUtils as it runs; settings are in config.toml.\n{text}"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_apps_and_hidden_windows() {
        let state = State {
            managed_apps: vec![ManagedApp { match_kind: MatchKind::Exe, value: "a.exe".into() }],
            hidden: Some(Hidden { boot_epoch: 42, slots: vec![0, 1234, 0] }),
        };
        let text = toml::to_string(&state).unwrap();
        let back: State = toml::from_str(&text).unwrap();
        assert_eq!(back.managed_apps[0].value, "a.exe");
        assert_eq!(back.hidden.unwrap().slots, vec![0, 1234, 0]);
    }

    #[test]
    fn missing_parts_are_empty() {
        let back: State = toml::from_str("").unwrap();
        assert!(back.managed_apps.is_empty() && back.hidden.is_none());
    }
}
