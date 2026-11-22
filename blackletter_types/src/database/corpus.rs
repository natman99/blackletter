//! Startup corpus loader (plan Task 4.1): loads the vendored
//! 5e-database SRD JSON into indexed in-memory tables. Every entry is
//! edition-tagged; the default and only auto-loaded edition is 2014.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use super::{
    background::Background, class::DatabaseClass, condition::Condition, feat::DatabaseFeat,
    feature::DatabaseFeature, item::Equipment, level::DatabaseLevel, race::Race,
    reference::Edition, spell::DatabaseSpell, subclass::DatabaseSubclass,
    traits::Trait as DatabaseTrait,
};

/// The whole SRD, indexed. Immutable after load.
#[derive(Debug, Clone, Default)]
pub struct Corpus {
    pub edition: Edition,
    pub spells: Vec<DatabaseSpell>,
    pub items: Vec<Equipment>,
    pub feats: Vec<DatabaseFeat>,
    pub levels: Vec<DatabaseLevel>,
    pub features: Vec<DatabaseFeature>,
    pub races: Vec<Race>,
    pub subraces: Vec<super::race::Subrace>,
    pub classes: Vec<DatabaseClass>,
    pub subclasses: Vec<DatabaseSubclass>,
    pub backgrounds: Vec<Background>,
    pub traits: Vec<DatabaseTrait>,
    pub conditions: Vec<Condition>,
    indexes: CorpusIndexes,
}

#[derive(Debug, Clone, Default)]
struct CorpusIndexes {
    by_index: HashMap<String, usize>,
    class_levels: HashMap<(String, Option<String>), Vec<usize>>,
}

impl Corpus {
    /// Load every `5e-SRD-*.json` under `dir` (e.g.
    /// `5e-database/src/2014/en`). Missing optional files are skipped;
    /// malformed JSON is an error.
    pub fn load(dir: &Path, edition: Edition) -> Result<Self> {
        let mut c = Corpus {
            edition,
            ..Default::default()
        };
        read(&dir.join("5e-SRD-Spells.json"), &mut c.spells)?;
        read(&dir.join("5e-SRD-Equipment.json"), &mut c.items)?;
        read(&dir.join("5e-SRD-Feats.json"), &mut c.feats)?;
        read(&dir.join("5e-SRD-Levels.json"), &mut c.levels)?;
        read(&dir.join("5e-SRD-Features.json"), &mut c.features)?;
        read(&dir.join("5e-SRD-Races.json"), &mut c.races)?;
        read(&dir.join("5e-SRD-Subraces.json"), &mut c.subraces)?;
        read(&dir.join("5e-SRD-Classes.json"), &mut c.classes)?;
        read(&dir.join("5e-SRD-Subclasses.json"), &mut c.subclasses)?;
        read(&dir.join("5e-SRD-Backgrounds.json"), &mut c.backgrounds)?;
        read(&dir.join("5e-SRD-Traits.json"), &mut c.traits)?;
        read(&dir.join("5e-SRD-Conditions.json"), &mut c.conditions)?;
        c.build_indexes();
        Ok(c)
    }

    /// Find the vendored 2014 corpus: `$BLACKLETTER_SRD_DIR`, then
    /// `srd/2014/en` (the in-repo vendored corpus) and
    /// `5e-database/src/2014/en` (a dev clone) relative to the cwd and
    /// to the executable's ancestors (dev builds walk up out of
    /// `target/`).
    pub fn load_vendored() -> Result<Self> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(dir) = std::env::var("BLACKLETTER_SRD_DIR") {
            candidates.push(dir.into());
        }
        let mut up = Some(std::env::current_dir()?);
        while let Some(dir) = up {
            for sub in ["srd/2014/en", "5e-database/src/2014/en"] {
                candidates.push(dir.join(sub));
            }
            up = dir.parent().map(std::path::Path::to_path_buf);
        }
        if let Ok(exe) = std::env::current_exe() {
            let mut up = exe.parent().map(Path::to_path_buf);
            for _ in 0..6 {
                let Some(dir) = up else { break };
                for sub in ["srd/2014/en", "5e-database/src/2014/en"] {
                    candidates.push(dir.join(sub));
                }
                up = dir.parent().map(Path::to_path_buf);
            }
        }
        for cand in candidates {
            if cand.join("5e-SRD-Spells.json").is_file() {
                return Self::load(&cand, Edition::Y2014);
            }
        }
        anyhow::bail!("no vendored 5e-database found; set BLACKLETTER_SRD_DIR")
    }

    fn build_indexes(&mut self) {
        let mut by_index = HashMap::new();
        macro_rules! index {
            ($field:ident) => {
                for (i, e) in self.$field.iter().enumerate() {
                    by_index.insert(e.index.clone(), i);
                }
            };
        }
        index!(spells);
        index!(items);
        index!(feats);
        index!(features);
        index!(races);
        index!(subraces);
        index!(classes);
        index!(subclasses);
        index!(backgrounds);
        index!(traits);
        index!(conditions);
        let mut class_levels = HashMap::new();
        for (i, l) in self.levels.iter().enumerate() {
            class_levels
                .entry((
                    l.class.index.clone(),
                    l.subclass.as_ref().map(|s| s.index.clone()),
                ))
                .or_insert_with(Vec::new)
                .push(i);
        }
        self.indexes = CorpusIndexes {
            by_index,
            class_levels,
        };
    }

    /// Whether any table holds this SRD index (indexes are globally
    /// unique across the corpus).
    #[must_use]
    pub fn has_index(&self, index: &str) -> bool {
        self.indexes.by_index.contains_key(index)
    }

    pub fn item(&self, index: &str) -> Option<&Equipment> {
        self.items.iter().find(|e| e.index == index)
    }

    pub fn feature(&self, index: &str) -> Option<&DatabaseFeature> {
        self.features.iter().find(|e| e.index == index)
    }

    pub fn spell(&self, index: &str) -> Option<&DatabaseSpell> {
        self.spells.iter().find(|e| e.index == index)
    }

    /// The level rows for a class (optionally narrowed to a subclass),
    /// sorted by level.
    #[must_use]
    pub fn levels_for(
        &self,
        class_index: &str,
        subclass_index: Option<&str>,
    ) -> Vec<&DatabaseLevel> {
        let mut rows: Vec<&DatabaseLevel> = match subclass_index {
            Some(sub) => self
                .indexes
                .class_levels
                .get(&(class_index.to_string(), Some(sub.to_string())))
                .map(|idxs| idxs.iter().map(|i| &self.levels[*i]).collect())
                .unwrap_or_default(),
            None => self
                .levels
                .iter()
                .filter(|l| l.class.index == class_index && l.subclass.is_none())
                .collect(),
        };
        rows.sort_by_key(|l| l.level);
        rows
    }
}

fn read<T: serde::de::DeserializeOwned>(path: &Path, out: &mut Vec<T>) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let parsed = serde_json::from_str::<Vec<T>>(&text)
        .with_context(|| format!("parsing {}", path.display()))?;
    out.extend(parsed);
    Ok(())
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    fn corpus() -> Corpus {
        // The corpus is vendored in-repo under `srd/` — tests must
        // never reach outside the project.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../srd/2014/en");
        Corpus::load(&dir, Edition::Y2014).expect("vendored corpus loads")
    }

    #[test]
    fn corpus_loads_all_tables() {
        let c = corpus();
        assert_eq!(c.edition, Edition::Y2014);
        assert!(c.spells.len() > 300, "spells: {}", c.spells.len());
        assert!(c.items.len() > 200, "items: {}", c.items.len());
        assert!(c.features.len() > 300, "features: {}", c.features.len());
        assert_eq!(c.levels.len(), 290);
        assert!(c.classes.iter().any(|k| k.index == "sorcerer"));
        assert!(c.item("club").is_some());
        assert!(c.spell("fireball").is_some());
    }

    #[test]
    fn every_level_feature_reference_resolves() {
        // Plan Task 4.1 coverage rule: walk levels 1-20 for every class
        // and assert every feature reference resolves in Features.json.
        let c = corpus();
        let mut rows = 0;
        for class in &c.classes {
            let levels = c.levels_for(&class.index, None);
            assert_eq!(levels.len(), 20, "class {} levels", class.index);
            for l in levels {
                rows += 1;
                for f in &l.features {
                    assert!(
                        c.feature(&f.index).is_some(),
                        "unresolved feature {} at {} level {}",
                        f.index,
                        class.index,
                        l.level
                    );
                }
            }
        }
        assert_eq!(rows, 20 * c.classes.len());
    }

    #[test]
    fn srd_equipment_and_feat_convert() {
        let c = corpus();
        let club =
            crate::Item::try_from_database(c.item("club").unwrap().clone()).expect("club converts");
        assert_eq!(club.name, "Club");
        assert_eq!(
            club.roll
                .map(|r| r.to_string().trim().to_string())
                .as_deref(),
            Some("1d4")
        );
        let grappler = crate::sheet::feat::Feat::try_from_database(
            c.feats
                .iter()
                .find(|f| f.index == "grappler")
                .unwrap()
                .clone(),
        );
        assert_eq!(grappler.name, "Grappler");
        assert!(grappler.desc.contains("Grappling"));
    }

    #[test]
    fn sorcerer_slot_table_matches_engine_math() {
        // SRD Levels.json carries per-level slots; cross-check the
        // full-caster progression the engine computes for level 5.
        let c = corpus();
        let l5 = &c.levels_for("sorcerer", None)[4];
        let sc = l5.spellcasting.as_ref().expect("sorcerer spellcasting");
        assert_eq!(sc.spell_slots_level_1, Some(4));
        assert_eq!(sc.spell_slots_level_3, Some(2));
        assert_eq!(sc.spells_known, Some(6));
    }
}
