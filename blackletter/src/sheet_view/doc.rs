//! The editable sheet document: `Character` + file identity + undo history.
//!
//! Same whole-snapshot undo as dungeons: sheets are small, snapshots can
//! never drift. Cap 64.

use std::path::{Path, PathBuf};

use blackletter_types::Character;

const UNDO_CAP: usize = 64;

pub struct SheetDoc {
    pub character: Character,
    /// The file this sheet was loaded from / last saved to.
    pub file: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<Character>,
    redo: Vec<Character>,
}

impl SheetDoc {
    /// Adopt an already-built character (the creation wizard's answer,
    /// plan Task 5.1): the doc starts dirty so the first save writes it.
    pub fn with_character(character: blackletter_types::Character) -> Self {
        Self {
            character,
            file: None,
            dirty: true,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            character: Character::new(
                name,
                1,
                blackletter_types::HitPoints {
                    current: 10,
                    max: 10,
                    level_bonus: 0,
                    bonus: 0,
                },
                blackletter_types::class::ClassType::Fighter,
                blackletter_types::AbilityScores::default(),
            ),
            file: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let character: Character = serde_json::from_str(&content)?;
        Ok(Self {
            character,
            file: Some(path.to_path_buf()),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    /// Snapshot, mutate, clear redo, mark dirty. Every mutation goes through here.
    pub fn edit(&mut self, f: impl FnOnce(&mut Character)) {
        self.undo.push(self.character.clone());
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        f(&mut self.character);
        self.dirty = true;
    }

    /// Snapshot, mutate, and hand back whatever `f` computed (roll results).
    pub fn edit_return<T>(&mut self, f: impl FnOnce(&mut Character) -> T) -> T {
        self.undo.push(self.character.clone());
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        f(&mut self.character)
    }

    /// Fallible mutation: the snapshot is pushed and the change applied only
    /// when `f` succeeds, so failed attempts never pollute the undo stack.
    pub fn try_edit<T, E>(
        &mut self,
        f: impl FnOnce(&mut Character) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut next = self.character.clone();
        let out = f(&mut next)?;
        self.undo.push(std::mem::replace(&mut self.character, next));
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        Ok(out)
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(std::mem::replace(&mut self.character, prev));
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(&mut self.character, next));
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    /// Write to `dir` under the sheet's name if `file` is unset. Returns the
    /// path written, or `None` when there was nothing to write.
    pub fn save_to_dir(&mut self, dir: &Path) -> anyhow::Result<Option<PathBuf>> {
        if !self.dirty {
            return Ok(None);
        }
        let path = match &self.file {
            // Known file: write exactly where it lives (no re-sanitizing —
            // campaign folder filenames are authoritative).
            Some(f) => {
                let json = serde_json::to_string_pretty(&self.character)?;
                std::fs::write(f, json)?;
                f.clone()
            }
            // New sheet: sanitize the name for the filename.
            None => {
                let stem: String = self
                    .character
                    .name
                    .chars()
                    .map(|c| match c {
                        '/' | '\\' | ':' | '\0' => '-',
                        c => c,
                    })
                    .collect();
                let path = dir.join(format!("{stem}.json"));
                let json = serde_json::to_string_pretty(&self.character)?;
                std::fs::write(&path, json)?;
                path
            }
        };
        self.file = Some(path.clone());
        self.dirty = false;
        Ok(Some(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_roundtrips_and_syncs() {
        let mut doc = SheetDoc::new("Undo Test");
        doc.edit(|c| {
            c.classes.classes[0].level.0 = 4;
            c.sync_proficiency_bonus();
        });
        assert_eq!(doc.character.classes.total_level(), 4);
        assert_eq!(doc.character.skills.proficiency_bonus, 2);
        assert!(doc.dirty);

        assert!(doc.undo());
        assert_eq!(doc.character.classes.total_level(), 1);
        assert!(doc.redo());
        assert_eq!(doc.character.classes.total_level(), 4);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("bl_sheet_doc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = SheetDoc::new("Save Load");
        doc.edit(|c| c.gold = 42);
        let path = doc.save_to_dir(&dir).unwrap().unwrap();
        assert!(path.exists());
        assert!(!doc.dirty);
        assert!(doc.save_to_dir(&dir).unwrap().is_none());
        let back = SheetDoc::load(&path).unwrap();
        assert_eq!(back.character.gold, 42);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
