#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

#[cfg(feature = "serde")]
use crate::Score;
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

    /// Import an SRD race into the sheet shape (plan Task 4.2: the race
    /// picker). Ability bonuses and walking speed become typed fields; the
    /// race's traits ride along as prose — trait mechanics arrive with the
    /// creation wizard (Phase 5), which reads the corpus trait tables.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn try_from_database(value: crate::database::race::Race) -> Self {
        let ability_bonuses = value
            .ability_bonuses
            .iter()
            .filter_map(|b| {
                let score = Score::ALL
                    .iter()
                    .zip(Score::ALL_STR)
                    .find(|(_, s)| *s == b.ability_score.index)
                    .map(|(score, _)| *score)?;
                Some(AbilityScoreBonus {
                    score,
                    bonus: b.bonus as i32,
                })
            })
            .collect();
        let traits: Vec<&str> = value.traits.iter().map(|t| t.name.as_str()).collect();
        let mut desc = String::new();
        if !traits.is_empty() {
            desc.push_str(&format!("Traits: {}.", traits.join("; ")));
        }
        if !value.language_desc.is_empty() {
            if !desc.is_empty() {
                desc.push(' ');
            }
            desc.push_str(&value.language_desc);
        }
        Self {
            name: value.name,
            desc,
            ability_bonuses,
            speed: Some(value.speed),
            feature: None,
            source: Some(Source::Single("SRD (2014)".into())),
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    #[test]
    fn srd_race_imports_typed_fields() {
        let raw = crate::database::race::Race {
            index: "dwarf".into(),
            name: "Dwarf".into(),
            speed: 25,
            ability_bonuses: vec![crate::database::race::AbilityBonuse {
                ability_score: crate::database::race::AbilityScore {
                    index: "con".into(),
                    ..Default::default()
                },
                bonus: 2,
            }],
            language_desc: "Common and Dwarvish".into(),
            traits: vec![
                crate::database::race::Trait {
                    index: "darkvision".into(),
                    name: "Darkvision".into(),
                    ..Default::default()
                },
                crate::database::race::Trait {
                    index: "stonecunning".into(),
                    name: "Stonecunning".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let r = Race::try_from_database(raw);
        assert_eq!(r.name, "Dwarf");
        assert_eq!(r.speed, Some(25));
        assert_eq!(r.ability_bonuses.len(), 1);
        assert_eq!(r.ability_bonuses[0].score, Score::Con);
        assert_eq!(r.ability_bonuses[0].bonus, 2);
        assert!(r.desc.contains("Darkvision"));
        assert!(r.desc.contains("Stonecunning"));
        assert!(r.desc.contains("Dwarvish"));
    }
}
