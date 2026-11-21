//! Typed background: the fourth `Grant` alongside race, class, and feats.
//!
//! Lossless apply/remove like `Race`: the background's mechanical effects
//! (skill proficiencies) flow through the grant architecture; its fiction
//! (feature text, personality traits) rides along for display.

use std::str::FromStr;

#[cfg(feature = "rhai")]
use rhai::CustomType;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{
    feature::{Effect, Feature},
    grant::Grant,
    language::Language,
    skills::Skill,
    source::Source,
};

/// A character's background (SRD: Acolyte, Soldier, ...).
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Background {
    pub name: String,
    /// Skill proficiencies the background grants.
    pub skill_proficiencies: Vec<Skill>,
    /// Languages already chosen for the background.
    pub languages: Vec<Language>,
    /// Number of language choices still open (SRD Acolyte: 2, any).
    pub language_choices: Option<i64>,
    /// The background feature (e.g. Acolyte's Shelter of the Faithful).
    pub feature: Option<Feature>,
    /// Starting equipment as `"{name} ×{qty}"` display strings.
    pub equipment: Vec<String>,
    /// Starting gold (gp).
    pub gold: i64,
    pub source: Option<Source>,
}

impl Grant for Background {
    fn grant_name(&self) -> String {
        self.name.clone()
    }

    fn grant_source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    fn effects(&self) -> Vec<Effect> {
        self.skill_proficiencies
            .iter()
            .map(|s| Effect::AddProficiency(*s))
            .collect()
    }
}

/// Parse a 5e-database proficiency index (`"skill-insight"`) into a `Skill`.
#[cfg(feature = "serde")]
fn skill_from_index(index: &str) -> Option<Skill> {
    let name = index.strip_prefix("skill-")?;
    Skill::from_str(name).ok()
}

#[cfg(feature = "serde")]
impl TryFrom<crate::database::background::Background> for Background {
    type Error = String;

    fn try_from(value: crate::database::background::Background) -> Result<Self, Self::Error> {
        let mut skill_proficiencies = Vec::new();
        for p in &value.starting_proficiencies {
            match p.index.split_once('-') {
                // Only skills are mechanical in SRD backgrounds; tool/weapon
                // proficiencies would land at the feature layer later.
                Some(("skill", _)) => {
                    let skill = skill_from_index(&p.index)
                        .ok_or_else(|| format!("unknown skill proficiency: {}", p.index))?;
                    skill_proficiencies.push(skill);
                }
                Some((other, _)) => return Err(format!("unsupported proficiency kind: {other}")),
                None => return Err(format!("malformed proficiency index: {}", p.index)),
            }
        }
        let feature = value.feature.map(|f| Feature {
            name: f.name,
            source: Some(Source::Single("5e-SRD-Backgrounds".to_string())),
            effects: Vec::new(),
        });
        let equipment = value
            .starting_equipment
            .iter()
            .map(|e| format!("{} ×{}", e.equipment.name, e.quantity))
            .collect();
        Ok(Self {
            name: value.name,
            skill_proficiencies,
            languages: Vec::new(),
            language_choices: value.language_options.map(|o| o.choose),
            feature,
            equipment,
            gold: value.starting_gold.map(|g| g.quantity).unwrap_or(0),
            source: Some(Source::Single(format!(
                "5e-SRD-Backgrounds/{}",
                value.index
            ))),
        })
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    fn srd() -> Vec<crate::database::background::Background> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../5e-database/src/2014/en/5e-SRD-Backgrounds.json"
        );
        let text = std::fs::read_to_string(path).expect("vendored SRD backgrounds");
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn acolyte_converts_and_grants_its_skills() {
        let bgs = srd();
        let raw = bgs.iter().find(|b| b.index == "acolyte").unwrap();
        let bg = Background::try_from(raw.clone()).unwrap();
        assert_eq!(bg.name, "Acolyte");
        assert_eq!(
            bg.skill_proficiencies,
            vec![Skill::Insight, Skill::Religion]
        );
        assert_eq!(bg.language_choices, Some(2));
        assert_eq!(bg.gold, 15);
        assert_eq!(bg.feature.as_ref().unwrap().name, "Shelter of the Faithful");
        assert!(
            bg.equipment
                .iter()
                .any(|e| e.starts_with("Clothes, common"))
        );

        // The grant architecture sees the proficiencies.
        let effects = bg.effects();
        assert_eq!(effects.len(), 2);
        assert!(effects.contains(&Effect::AddProficiency(Skill::Insight)));
        assert_eq!(bg.grant_name(), "Acolyte");
    }

    #[test]
    fn every_srd_background_converts() {
        for raw in srd() {
            let bg =
                Background::try_from(raw.clone()).unwrap_or_else(|e| panic!("{}: {e}", raw.index));
            assert!(!bg.skill_proficiencies.is_empty());
        }
    }

    #[test]
    fn background_round_trips_through_json() {
        let bg = Background {
            name: "Soldier".into(),
            skill_proficiencies: vec![Skill::Athletics, Skill::Intimidation],
            gold: 10,
            ..Default::default()
        };
        let json = serde_json::to_string(&bg).unwrap();
        let back: Background = serde_json::from_str(&json).unwrap();
        assert_eq!(bg, back);
    }
}
