//! The persistent custom-content library (plan Task 4.3): the second
//! content source beside the read-only SRD corpus.
//!
//! Two databases side by side, by user ruling: the vendored SRD never
//! changes, and everything the user adds (by hand today, scraped from a
//! wiki tomorrow) lands here, on disk, and shows up in every picker from
//! then on. Entries use the sheet's own homebrew-friendly types — a custom
//! spell *is* a `sheet::Spell`, so picking one needs no conversion, only
//! the `custom:` index prefix to route the lookup.

use std::collections::BTreeMap;
#[cfg(feature = "serde")]
use std::path::Path;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::Item;
use crate::sheet::{
    background::Background, feat::Feat, feature::Feature, race::Race, spells::Spell, traits::Trait,
};

/// Marks a picker choice as library-sourced rather than corpus-sourced.
/// Corpus indexes never contain a colon, so the prefix cannot collide.
pub const CUSTOM_PREFIX: &str = "custom:";

/// Every custom entry, keyed by name (BTreeMap: pickers list them sorted
/// and diffs stay stable). Loaded once at startup, saved on every change.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    pub spells: BTreeMap<String, Spell>,
    pub items: BTreeMap<String, Item>,
    pub feats: BTreeMap<String, Feat>,
    pub races: BTreeMap<String, Race>,
    pub backgrounds: BTreeMap<String, Background>,
    pub features: BTreeMap<String, Feature>,
    pub traits: BTreeMap<String, Trait>,
}

impl Library {
    /// The picker index for a library entry.
    #[must_use]
    pub fn custom_index(name: &str) -> String {
        format!("{CUSTOM_PREFIX}{name}")
    }

    #[must_use]
    pub fn is_custom(index: &str) -> bool {
        index.starts_with(CUSTOM_PREFIX)
    }

    /// The name part of a `custom:<name>` index.
    #[must_use]
    pub fn custom_name(index: &str) -> Option<&str> {
        index.strip_prefix(CUSTOM_PREFIX)
    }

    /// Read the library file; a missing file is an empty library, not an
    /// error (first run).
    #[cfg(feature = "serde")]
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Write the library (creating the parent dir on first save).
    #[cfg(feature = "serde")]
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    #[test]
    fn index_routing_is_collision_free() {
        let idx = Library::custom_index("Fireball");
        assert!(Library::is_custom(&idx));
        assert_eq!(Library::custom_name(&idx), Some("Fireball"));
        // Corpus indexes never route as custom.
        assert!(!Library::is_custom("fireball"));
        assert!(!Library::is_custom("custom"));
    }

    #[test]
    fn save_load_roundtrip_and_missing_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("bl_library_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested/library.json");
        assert_eq!(Library::load(&path).unwrap(), Library::default());

        let mut lib = Library::default();
        lib.spells.insert(
            "Fireball".into(),
            Spell {
                name: "Fireball".into(),
                level: 3,
                ..Default::default()
            },
        );
        lib.items
            .insert("Doomhammer".into(), Item::new("Doomhammer".into()));
        lib.save(&path).unwrap();

        let back = Library::load(&path).unwrap();
        assert_eq!(back, lib);
        assert_eq!(back.spells["Fireball"].level, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
