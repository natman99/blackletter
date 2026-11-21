//! The session file: what the app re-opens on launch, in the platform config
//! dir (`~/.config/blackletter/session.json`, `%APPDATA%` on Windows).
//!
//! Every field is `#[serde(default)]` so an older session file always loads.

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::root::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredMode {
    #[default]
    Sheet,
    Dungeon,
}

impl From<Mode> for StoredMode {
    fn from(m: Mode) -> Self {
        match m {
            Mode::Sheet => StoredMode::Sheet,
            Mode::Dungeon => StoredMode::Dungeon,
        }
    }
}

impl From<StoredMode> for Mode {
    fn from(m: StoredMode) -> Self {
        match m {
            StoredMode::Sheet => Mode::Sheet,
            StoredMode::Dungeon => Mode::Dungeon,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// The campaign folder the app last had open.
    pub campaign: Option<PathBuf>,
    /// Sheet files inside the campaign, by path.
    pub sheets: Vec<PathBuf>,
    /// Dungeon files inside the campaign, by path.
    pub dungeons: Vec<PathBuf>,
    pub mode: StoredMode,
    /// Last window size, restored on launch (position stays compositor-chosen).
    pub window_size: Option<(f32, f32)>,
}

/// Where the session file lives. `None` when the platform has no config dir.
pub fn session_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "blackletter").map(|p| p.config_dir().join("session.json"))
}

pub fn load(path: &Path) -> Session {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, session: &Session) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(session)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    crate::autosave::atomic_write(path, json.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_missing_file_defaults() {
        let dir = std::env::temp_dir().join("blackletter_session_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");

        // Missing file: defaults, no error.
        assert_eq!(load(&path), Session::default());

        let s = Session {
            campaign: Some(dir.clone()),
            sheets: vec![dir.join("a.json")],
            dungeons: vec![dir.join("d.json")],
            mode: StoredMode::Dungeon,
            window_size: Some((1200., 800.)),
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);

        // An older/partial file still loads (every field defaulted).
        std::fs::write(&path, r#"{"mode": "dungeon"}"#).unwrap();
        let partial = load(&path);
        assert_eq!(partial.mode, StoredMode::Dungeon);
        assert!(partial.sheets.is_empty());

        // A corrupt file is a fresh session, not a crash.
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load(&path), Session::default());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
