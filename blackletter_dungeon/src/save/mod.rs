use std::fs;
use std::path::{Path, PathBuf};

use crate::model::Dungeon;

/// The legacy save directory of the standalone `dungeon-tracker` app.
/// Kept under the old project identifier so existing saves can be imported.
pub fn legacy_saves_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "dungeon-tracker")
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("saves")
}

/// Turn an arbitrary name into a safe filename stem.
fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "untitled".to_string()
    } else {
        cleaned
    }
}

fn save_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.json", sanitize_name(name)))
}

/// Save a dungeon to `dir` under a filename derived from `name`.
pub fn save_to(dir: &Path, name: &str, dungeon: &Dungeon) -> Result<PathBuf, SaveError> {
    fs::create_dir_all(dir).map_err(SaveError::Io)?;
    let path = save_path(dir, name);
    let json = serde_json::to_string_pretty(dungeon).map_err(SaveError::Serialize)?;
    fs::write(&path, json).map_err(SaveError::Io)?;
    Ok(path)
}

/// Load a dungeon previously saved to `dir`/`name`.
pub fn load_from(dir: &Path, name: &str) -> Result<Dungeon, LoadError> {
    let path = save_path(dir, name);
    let content = fs::read_to_string(&path).map_err(LoadError::Read)?;
    let dungeon: Dungeon = serde_json::from_str(&content).map_err(LoadError::Parse)?;
    Ok(dungeon)
}

/// List the names of all saves in `dir` (sorted), without the `.json` suffix.
pub fn list_saves(dir: &Path) -> Vec<String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let file_name = e.file_name().to_string_lossy().to_string();
            file_name.strip_suffix(".json").map(|s| s.to_string())
        })
        .collect();
    names.sort();
    names
}

#[derive(Debug)]
pub enum SaveError {
    Io(std::io::Error),
    Serialize(serde_json::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "io error: {e}"),
            SaveError::Serialize(e) => write!(f, "serialize error: {e}"),
        }
    }
}
impl std::error::Error for SaveError {}

#[derive(Debug)]
pub enum LoadError {
    Read(std::io::Error),
    Parse(serde_json::Error),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Read(e) => write!(f, "read error: {e}"),
            LoadError::Parse(e) => write!(f, "parse error: {e}"),
        }
    }
}
impl std::error::Error for LoadError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Direction;

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join("dungeon_tracker_test_roundtrip");
        let _ = fs::remove_dir_all(&dir);

        let mut d = Dungeon::new("Round Trip");
        let a = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        d.room_mut(a).unwrap().name = Some("Armory".into());
        d.room_mut(a).unwrap().trap = crate::model::TrapState::Active;

        let path = save_to(&dir, &d.name, &d).unwrap();
        assert!(path.exists());

        let loaded = load_from(&dir, &d.name).unwrap();
        assert_eq!(loaded.name, d.name);
        assert_eq!(loaded.rooms.len(), d.rooms.len());
        assert_eq!(loaded.doors.len(), d.doors.len());
        let loaded_room = loaded.room(a).unwrap();
        assert_eq!(loaded_room.name, Some("Armory".into()));
        assert_eq!(loaded_room.trap, crate::model::TrapState::Active);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_saves_reports_names() {
        let dir = std::env::temp_dir().join("dungeon_tracker_test_list");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("alpha.json"), "{}").unwrap();
        fs::write(dir.join("beta.json"), "{}").unwrap();
        fs::write(dir.join("notasave.txt"), "x").unwrap();
        assert_eq!(list_saves(&dir), vec!["alpha", "beta"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_replaces_bad_chars() {
        assert_eq!(sanitize_name("My/Dungeon:5e"), "My_Dungeon_5e");
        assert_eq!(sanitize_name("   "), "untitled");
    }
}
