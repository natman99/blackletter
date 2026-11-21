use std::fmt::Display;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

use crate::{
    Score,
    condition::{Condition, ConditionKind},
    meter::Meter,
    senses::Sense,
    sheet::source::{HasSource, Source},
    skills::Skill,
    spells::Spell,
};

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
pub struct Feature {
    pub name: String,
    pub source: Option<Source>,
    pub effects: Vec<Effect>,
}

impl HasSource for Feature {
    fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    fn add_source(&mut self, source: Source) {
        self.source = Some(source)
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
/// The effect the feature has. Effects are never applied by mutation:
/// they are aggregated into `Modifiers` and read at query time, so a
/// grant carrying them can be added or removed losslessly.
pub enum Effect {
    /// Flat armor class (ac) bonus.
    AcBonus(i64),
    /// Add a spell.
    Spell(Spell),
    /// Add a meter. e.g. spell slots or other limited resources.
    Meter(Meter),
    /// Bonus max health that is applied per level.
    HealthBonusPerLevel(i64),
    /// Ability score bonus (raw stat bonus).
    AbilityScoreBonus(AbilityScoreBonus),
    /// An extra sense type.
    Sense(Sense),
    /// Proficiency in a skill.
    AddProficiency(Skill),
    /// Bonus to initiative.
    InitiativeBonus(i64),
    /// A condition effect for the conditions page
    Condition(Condition),
    /// Expertise in a skill (proficiency doubled).
    Expertise(Skill),
    /// Bonus to walking speed in feet.
    SpeedBonus(i64),
    /// Flat bonus to a specific skill check.
    SkillBonus { skill: Skill, bonus: i64 },
    /// Flat bonus to a specific saving throw.
    SaveBonus { score: Score, bonus: i64 },
    /// The creature can't be subjected to this condition.
    ConditionImmunity(ConditionKind),
    /// Homebrew effect with no mechanical interpretation; shown, not computed.
    Custom(String),
}

impl Display for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Effect::AcBonus(a) => format!("AC: {a:+}"),
            Effect::Spell(spell) => format!("Spell {}", spell.name),
            Effect::Meter(meter) => format!("Meter {}", meter.name),
            Effect::HealthBonusPerLevel(h) => format!("Hp/lvl {h}"),
            Effect::AbilityScoreBonus(ability_score_bonus) => format!(
                "Score {} {:+}",
                ability_score_bonus.score, ability_score_bonus.bonus
            ),
            Effect::Sense(sense) => format!("{sense}"),
            Effect::AddProficiency(skill) => format!("{skill}"),
            Effect::InitiativeBonus(b) => format!("Initiative {:+}", b),
            Effect::Condition(condition) => format!("{}", condition.kind),
            Effect::Expertise(skill) => format!("Expertise {skill}"),
            Effect::SpeedBonus(b) => format!("Speed {b:+} ft"),
            Effect::SkillBonus { skill, bonus } => format!("{skill} {bonus:+}"),
            Effect::SaveBonus { score, bonus } => format!("{score} save {bonus:+}"),
            Effect::ConditionImmunity(kind) => format!("Immune to {kind}"),
            Effect::Custom(s) => s.clone(),
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
pub struct Features {
    pub inner: Vec<Feature>,
}

#[derive(Debug, Clone, PartialEq, Copy, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AbilityScoreBonus {
    /// The abilty score to boost.
    pub score: Score,
    /// The amount to increase by.
    pub bonus: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_display() {
        assert_eq!(Effect::AcBonus(2).to_string(), "AC: +2");
        assert_eq!(Effect::SpeedBonus(10).to_string(), "Speed +10 ft");
        assert_eq!(
            Effect::ConditionImmunity(ConditionKind::Poisoned).to_string(),
            "Immune to Poisoned"
        );
        assert_eq!(
            Effect::Custom("Your soul glows on Tuesdays".to_string()).to_string(),
            "Your soul glows on Tuesdays"
        );
    }
}
