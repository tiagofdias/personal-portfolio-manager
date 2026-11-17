//! Tiny JSON persistence in the user's app-data folder.

use crate::ai::{AiSettings, ChatMessage};
use crate::portfolio::Portfolio;
use crate::profile::Profile;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub portfolio: Portfolio,
    #[serde(default)]
    pub profile: Profile,
    #[serde(default)]
    pub chat: Vec<ChatMessage>,
    #[serde(default)]
    pub ai: AiSettings,
    #[serde(default)]
    pub input_text: String,
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| {
            let h = PathBuf::from(h);
            if cfg!(target_os = "macos") { h.join("Library/Application Support") } else { h.join(".config") }
        }))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("PersonalPortfolioManager")
}

fn state_file() -> PathBuf {
    data_dir().join("state.json")
}

pub fn load() -> State {
    let mut s: State = std::fs::read_to_string(state_file())
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    s.profile.ensure_len();
    s
}

pub fn save(s: &State) -> std::io::Result<()> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    // Write-then-rename so a crash never leaves a half-written file.
    let tmp = dir.join("state.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(tmp, state_file())
}
