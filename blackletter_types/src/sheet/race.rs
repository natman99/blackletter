#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

use crate::{
    feature::{AbilityScoreBonus, Feature},
    sheet::source::Source,
};

/// A race on the sheet (SRD or homebrew). Like `Feat`/`Trait`: descriptive
/// text plus optional mechanics. Ability bonuses and speed are typed fields
/// (for display and derivation); anything else a race grants (Darkvision,
/// damage resistances, skill proficiencies) rides on `feature`.
///
/// Database races (`database::race::Race`) can be imported into this shape;
/// homebrew races are just this struct with different contents.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
pub struct Race {
    #[cfg_attr(
        feature = "rhai",
        rhai_type(get = Self::get_name, set = Self::set_name)
    )]
    pub name: String,
    #[cfg_attr(
        feature = "rhai",
        rhai_type(get = Self::get_desc, set = Self::set_desc)
    )]
    pub desc: String,
    /// Ability score bonuses the race grants (e.g. Elf: +2 Dex).
    pub ability_bonuses: Vec<AbilityScoreBonus>,
    /// Base walking speed, if the race differs from the sheet default
    /// (Dwarf: 25). `None` = use the sheet's `walking_speed`.
    pub speed: Option<i64>,
    /// Mechanics: senses, resistances, proficiencies, ...
    pub feature: Option<Feature>,
    pub source: Option<Source>,
}

impl Race {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn get_name(&self) -> String {
        self.name.clone()
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn get_desc(&self) -> String {
        self.desc.clone()
    }

    pub fn set_desc(&mut self, desc: String) {
        self.desc = desc;
    }
}
