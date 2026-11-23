#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::background::EquipmentEntry;
use super::options::ChoiceOption;
use super::reference::Reference;

/// Raw `5e-SRD-Classes.json` entry — lean: only the fields Blackletter
/// reads. The old generated full-shape version never compiled against
/// real SRD JSON (externally-tagged option enums) and was unused.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseClass {
    pub index: String,
    pub name: String,
    pub hit_die: i32,
    pub saving_throws: Vec<Reference>,
    pub proficiencies: Vec<Reference>,
    pub subclasses: Vec<Reference>,
    /// "Choose N from ..." skill/weapon sets the creation wizard prompts
    /// (plan Task 5.1); the old lean shape dropped these.
    #[serde(default)]
    pub proficiency_choices: Vec<ChoiceOption>,
    /// Fixed starting package (wizard: a spellbook).
    #[serde(default)]
    pub starting_equipment: Vec<EquipmentEntry>,
    /// "choose X from Y" equipment alternatives.
    #[serde(default)]
    pub starting_equipment_options: Vec<ChoiceOption>,
    /// Multiclass gate: ability prerequisites and the REDUCED proficiency
    /// set gained when taking this class second (Wiki/03:52,87).
    #[serde(default)]
    pub multi_classing: Option<MultiClassing>,
    /// The class's spellcasting block; only the ability is read here —
    /// slot/known counts come from the Levels rows.
    #[serde(default)]
    pub spellcasting: Option<ClassSpellcasting>,
    #[serde(default)]
    pub url: String,
}

/// The `multi_classing` object of a class row.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiClassing {
    #[serde(default)]
    pub prerequisites: Vec<MulticlassPrerequisite>,
    #[serde(default)]
    pub proficiencies: Vec<Reference>,
}

/// The class row's `spellcasting` object (trimmed to what we read).
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClassSpellcasting {
    /// The ability this class's spells use for attack rolls and save
    /// DCs ("INT" for wizards; Wiki/02 class spellcasting sections).
    pub spellcasting_ability: Option<Reference>,
}

/// One ability-score prerequisite ("int", minimum 13).
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MulticlassPrerequisite {
    pub ability_score: Reference,
    pub minimum_score: i32,
}
