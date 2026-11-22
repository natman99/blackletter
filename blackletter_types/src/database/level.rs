#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::reference::Reference;

/// Raw `5e-SRD-Levels.json` entry: one row per class (or subclass) per
/// level — the level-up backbone (plan Task 4.1).
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseLevel {
    pub level: i32,
    pub ability_score_bonuses: i32,
    pub prof_bonus: i32,
    pub features: Vec<Reference>,
    pub spellcasting: Option<SpellcastingProgression>,
    pub index: String,
    pub class: Reference,
    pub subclass: Option<Reference>,
    #[serde(default)]
    pub url: String,
}

/// Per-level spellcasting progression (class rows; subclass rows carry
/// their own).
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpellcastingProgression {
    pub cantrips_known: Option<i32>,
    pub spells_known: Option<i32>,
    pub spell_slots_level_1: Option<i32>,
    pub spell_slots_level_2: Option<i32>,
    pub spell_slots_level_3: Option<i32>,
    pub spell_slots_level_4: Option<i32>,
    pub spell_slots_level_5: Option<i32>,
    pub spell_slots_level_6: Option<i32>,
    pub spell_slots_level_7: Option<i32>,
    pub spell_slots_level_8: Option<i32>,
    pub spell_slots_level_9: Option<i32>,
}
