//! The editable dungeon document: model + file identity + undo history.
//!
//! Undo is whole-`Dungeon` snapshots (dungeons are small; the old tracker
//! proved this scales fine and it can never drift). Cap 64.

use std::path::{Path, PathBuf};

use blackletter_dungeon::model::Dungeon;
use blackletter_dungeon::save;

const UNDO_CAP: usize = 64;

pub struct DungeonDoc {
    pub dungeon: Dungeon,
    /// The file this dungeon was loaded from / last saved to.
    pub file: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<Dungeon>,
    redo: Vec<Dungeon>,
}

impl DungeonDoc {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            dungeon: Dungeon::new(name),
            file: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let dungeon: Dungeon = serde_json::from_str(&content)?;
        Ok(Self {
            dungeon,
            file: Some(path.to_path_buf()),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    /// Snapshot, mutate, clear redo, mark dirty. Every mutation goes through here.
    pub fn edit(&mut self, f: impl FnOnce(&mut Dungeon)) {
        self.undo.push(self.dungeon.clone());
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        f(&mut self.dungeon);
        self.dirty = true;
    }

    /// Fallible mutation: the snapshot is pushed and the change applied only
    /// when `f` succeeds, so failed attempts never pollute the undo stack.
    pub fn try_edit<T, E>(&mut self, f: impl FnOnce(&mut Dungeon) -> Result<T, E>) -> Result<T, E> {
        let mut next = self.dungeon.clone();
        let out = f(&mut next)?;
        self.undo.push(std::mem::replace(&mut self.dungeon, next));
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        Ok(out)
    }

    #[allow(dead_code)] // the undo affordance badge arrives with the tab bar
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(std::mem::replace(&mut self.dungeon, prev));
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(&mut self.dungeon, next));
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    /// Write to `dir` under the dungeon's name if `file` is unset. Returns the
    /// path written, or `None` when there was nothing to write.
    pub fn save_to_dir(&mut self, dir: &Path) -> anyhow::Result<Option<PathBuf>> {
        if !self.dirty {
            return Ok(None);
        }
        let path = match &self.file {
            // Known file: write exactly where it lives (no re-sanitizing).
            Some(f) => {
                let json = serde_json::to_string_pretty(&self.dungeon)?;
                std::fs::write(f, json)?;
                f.clone()
            }
            // New dungeon: the save module picks (and sanitizes) the filename.
            None => save::save_to(dir, &self.dungeon.name, &self.dungeon)?,
        };
        self.file = Some(path.clone());
        self.dirty = false;
        Ok(Some(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackletter_dungeon::model::Direction;

    #[test]
    fn undo_redo_roundtrips() {
        let mut doc = DungeonDoc::new("Undo Test");
        let e = doc.dungeon.entrance;
        doc.edit(|d| {
            d.add_horizontal_door(e, Direction::E).unwrap();
        });
        assert_eq!(doc.dungeon.rooms.len(), 2);
        assert!(doc.dirty);

        assert!(doc.undo());
        assert_eq!(doc.dungeon.rooms.len(), 1);
        assert!(doc.can_redo());

        assert!(doc.redo());
        assert_eq!(doc.dungeon.rooms.len(), 2);
        assert!(!doc.can_redo());
    }

    #[test]
    fn undo_stack_is_capped() {
        let mut doc = DungeonDoc::new("Cap Test");
        for _ in 0..(UNDO_CAP + 10) {
            doc.edit(|_| {});
        }
        assert_eq!(doc.undo.len(), UNDO_CAP);
    }

    #[test]
    fn save_to_dir_writes_and_clears_dirty() {
        let dir = std::env::temp_dir().join(format!("bl_doc_save_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = DungeonDoc::new("Doc Save");
        doc.dirty = true;
        let path = doc.save_to_dir(&dir).unwrap().unwrap();
        assert!(path.exists());
        assert!(!doc.dirty);
        // Second save is a no-op while clean.
        assert!(doc.save_to_dir(&dir).unwrap().is_none());
        // And it reloads.
        let back = DungeonDoc::load(&path).unwrap();
        assert_eq!(back.dungeon.name, "Doc Save");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
