use std::{fmt::Display, str::FromStr};

use enumflags2::{BitFlags, bitflags};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

use crate::{Score, sheet::AbilityScores};

#[bitflags]
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
/// Skills enum.
pub enum Skill {
    Acrobatics,
    AnimalHandling,
    Arcana,
    Athletics,
    Deception,
    History,
    Insight,
    Intimidation,
    Investigation,
    Medicine,
    Nature,
    Perception,
    Performance,
    Persuasion,
    Religion,
    SleightOfHand,
    Stealth,
    Survival,
}

impl Skill {
    pub const ALL_STR: [&'static str; 18] = [
        "acrobatics",
        "animalHandling",
        "arcana",
        "athletics",
        "deception",
        "history",
        "insight",
        "intimidation",
        "investigation",
        "medicine",
        "nature",
        "perception",
        "performance",
        "persuasion",
        "religion",
        "sleightOfHand",
        "stealth",
        "survival",
    ];

    pub const ALL_STR_PRETTY: [&'static str; 18] = [
        "Acrobatics",
        "Animal Handling",
        "Arcana",
        "Athletics",
        "Deception",
        "History",
        "Insight",
        "Intimidation",
        "Investigation",
        "Medicine",
        "Nature",
        "Perception",
        "Performance",
        "Persuasion",
        "Religion",
        "Sleight Of Hand",
        "Stealth",
        "Survival",
    ];

    pub const ALL: [Skill; 18] = [
        Skill::Acrobatics,
        Skill::AnimalHandling,
        Skill::Arcana,
        Skill::Athletics,
        Skill::Deception,
        Skill::History,
        Skill::Insight,
        Skill::Intimidation,
        Skill::Investigation,
        Skill::Medicine,
        Skill::Nature,
        Skill::Perception,
        Skill::Performance,
        Skill::Persuasion,
        Skill::Religion,
        Skill::SleightOfHand,
        Skill::Stealth,
        Skill::Survival,
    ];
}

impl Display for Skill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Skill::Athletics => "Athletics",
            Skill::Acrobatics => "Acrobatics",
            Skill::SleightOfHand => "Sleight Of Hand",
            Skill::Stealth => "Stealth",
            Skill::Arcana => "Arcana",
            Skill::History => "History",
            Skill::Investigation => "Investigation",
            Skill::Nature => "Nature",
            Skill::Religion => "Religion",
            Skill::AnimalHandling => "Animal Handling",
            Skill::Insight => "Insight",
            Skill::Medicine => "Medicine",
            Skill::Perception => "Perception",
            Skill::Survival => "Survival",
            Skill::Deception => "Deception",
            Skill::Intimidation => "Intimidation",
            Skill::Performance => "Performance",
            Skill::Persuasion => "Persuasion",
        };

        write!(f, "{}", s)
    }
}

impl FromStr for Skill {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl TryFrom<&str> for Skill {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_lowercase().as_str() {
            "athletics" => Ok(Skill::Athletics),
            "acrobatics" => Ok(Skill::Acrobatics),
            "sleightofhand" => Ok(Skill::SleightOfHand),
            "stealth" => Ok(Skill::Stealth),
            "arcana" => Ok(Skill::Arcana),
            "history" => Ok(Skill::History),
            "investigation" => Ok(Skill::Investigation),
            "nature" => Ok(Skill::Nature),
            "religion" => Ok(Skill::Religion),
            "animalhandling" | "animal_handling" => Ok(Skill::AnimalHandling),
            "insight" => Ok(Skill::Insight),
            "medicine" => Ok(Skill::Medicine),
            "perception" => Ok(Skill::Perception),
            "survival" => Ok(Skill::Survival),
            "deception" => Ok(Skill::Deception),
            "intimidation" => Ok(Skill::Intimidation),
            "performance" => Ok(Skill::Performance),
            "persuasion" => Ok(Skill::Persuasion),
            _ => Err(()),
        }
    }
}
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
struct SkillsJson {
    athletics: bool,
    acrobatics: bool,
    sleightofhand: bool,
    stealth: bool,
    arcana: bool,
    history: bool,
    investigation: bool,
    nature: bool,
    religion: bool,
    animalhandling: bool,
    insight: bool,
    medicine: bool,
    perception: bool,
    survival: bool,
    deception: bool,
    intimidation: bool,
    performance: bool,
    persuasion: bool,
    #[cfg_attr(feature = "serde", serde(default = "default_proficiency_bonus"))]
    proficiency_bonus: i64,
    expertise: Vec<Skill>,
    overrides: Overrides,
}

/// SRD PHB Character Advancement: proficiency bonus starts at +2.
const fn default_proficiency_bonus() -> i64 {
    2
}
#[derive(Debug, Clone, PartialEq, Default, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
pub struct Overrides {
    pub athletics: Option<Score>,
    pub acrobatics: Option<Score>,
    pub sleight_of_hand: Option<Score>,
    pub stealth: Option<Score>,
    pub arcana: Option<Score>,
    pub history: Option<Score>,
    pub investigation: Option<Score>,
    pub nature: Option<Score>,
    pub religion: Option<Score>,
    pub animal_handling: Option<Score>,
    pub insight: Option<Score>,
    pub medicine: Option<Score>,
    pub perception: Option<Score>,
    pub survival: Option<Score>,
    pub deception: Option<Score>,
    pub intimidation: Option<Score>,
    pub performance: Option<Score>,
    pub persuasion: Option<Score>,
}

impl Overrides {
    fn get(&self, s: &Skill) -> Option<Score> {
        match s {
            Skill::Acrobatics => self.acrobatics,
            Skill::AnimalHandling => self.animal_handling,
            Skill::Arcana => self.arcana,
            Skill::Athletics => self.athletics,
            Skill::Deception => self.deception,
            Skill::History => self.history,
            Skill::Insight => self.insight,
            Skill::Intimidation => self.intimidation,
            Skill::Investigation => self.investigation,
            Skill::Medicine => self.medicine,
            Skill::Nature => self.nature,
            Skill::Perception => self.perception,
            Skill::Performance => self.performance,
            Skill::Persuasion => self.persuasion,
            Skill::Religion => self.religion,
            Skill::SleightOfHand => self.sleight_of_hand,
            Skill::Stealth => self.stealth,
            Skill::Survival => self.survival,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "serde", serde(from = "SkillsJson"))]
#[cfg_attr(feature = "serde", serde(into = "SkillsJson"))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// The characters proficiency skills.
pub struct Skills {
    pub proficiency_bonus: i64,
    pub proficiencies: BitFlags<Skill>,
    pub expertise: BitFlags<Skill>,
    pub overrides: Overrides,
}

impl Default for Skills {
    fn default() -> Self {
        Self {
            proficiency_bonus: 2,
            proficiencies: Default::default(),
            expertise: Default::default(),
            overrides: Default::default(),
        }
    }
}

impl From<SkillsJson> for Skills {
    fn from(value: SkillsJson) -> Self {
        let mut out = Skills::default();
        out.proficiencies.set(Skill::Athletics, value.athletics);
        out.proficiencies.set(Skill::Acrobatics, value.acrobatics);
        out.proficiencies
            .set(Skill::SleightOfHand, value.sleightofhand);
        out.proficiencies.set(Skill::Stealth, value.stealth);
        out.proficiencies.set(Skill::Arcana, value.arcana);
        out.proficiencies.set(Skill::History, value.history);
        out.proficiencies
            .set(Skill::Investigation, value.investigation);
        out.proficiencies.set(Skill::Nature, value.nature);
        out.proficiencies.set(Skill::Religion, value.religion);
        out.proficiencies
            .set(Skill::AnimalHandling, value.animalhandling);
        out.proficiencies.set(Skill::Insight, value.insight);
        out.proficiencies.set(Skill::Medicine, value.medicine);
        out.proficiencies.set(Skill::Perception, value.perception);
        out.proficiencies.set(Skill::Survival, value.survival);
        out.proficiencies.set(Skill::Deception, value.deception);
        out.proficiencies
            .set(Skill::Intimidation, value.intimidation);
        out.proficiencies.set(Skill::Performance, value.performance);
        out.proficiencies.set(Skill::Persuasion, value.persuasion);
        out.proficiency_bonus = value.proficiency_bonus;
        out.overrides = value.overrides;
        out.expertise = {
            let mut m: BitFlags<Skill> = BitFlags::empty();
            value.expertise.iter().for_each(|f| m.set(*f, true));
            m
        };
        out
    }
}

impl From<&Skills> for SkillsJson {
    fn from(value: &Skills) -> Self {
        let mut out = SkillsJson::default();
        out.proficiency_bonus = value.proficiency_bonus;
        out.overrides = value.overrides;
        value.proficiencies.iter().for_each(|f| match f {
            Skill::Athletics => out.athletics = true,
            Skill::Acrobatics => out.acrobatics = true,
            Skill::SleightOfHand => out.sleightofhand = true,
            Skill::Stealth => out.stealth = true,
            Skill::Arcana => out.arcana = true,
            Skill::History => out.history = true,
            Skill::Investigation => out.investigation = true,
            Skill::Nature => out.nature = true,
            Skill::Religion => out.religion = true,
            Skill::AnimalHandling => out.animalhandling = true,
            Skill::Insight => out.insight = true,
            Skill::Medicine => out.medicine = true,
            Skill::Perception => out.perception = true,
            Skill::Survival => out.survival = true,
            Skill::Deception => out.deception = true,
            Skill::Intimidation => out.intimidation = true,
            Skill::Performance => out.performance = true,
            Skill::Persuasion => out.persuasion = true,
        });

        out.expertise = {
            let mut m = vec![];
            value.expertise.into_iter().for_each(|f| m.push(f));
            m
        };

        out
    }
}
impl From<Skills> for SkillsJson {
    fn from(value: Skills) -> Self {
        Self::from(&value)
    }
}
impl Skills {
    /// The ability score that governs this skill (honoring overrides).
    #[must_use]
    pub fn score_for(&self, skill: &Skill) -> Score {
        if let Some(o) = self.overrides.get(skill) {
            o
        } else {
            match skill {
                Skill::Athletics => Score::Str,
                Skill::Acrobatics | Skill::SleightOfHand | Skill::Stealth => Score::Dex,
                Skill::Arcana
                | Skill::History
                | Skill::Investigation
                | Skill::Nature
                | Skill::Religion => Score::Int,
                Skill::AnimalHandling
                | Skill::Insight
                | Skill::Medicine
                | Skill::Perception
                | Skill::Survival => Score::Wis,
                Skill::Deception | Skill::Intimidation | Skill::Performance | Skill::Persuasion => {
                    Score::Cha
                }
            }
        }
    }

    /// Get the skill bonus.
    pub fn check(&self, skill: &Skill, ability_scores: &AbilityScores) -> i64 {
        let base_modifier = ability_scores.get(&self.score_for(skill)).modifier();

        let bonus = if self.proficiencies.contains(*skill) {
            self.proficiency_bonus
        } else {
            0
        };

        let bonus = if self.expertise.contains(*skill) {
            bonus * 2
        } else {
            bonus
        };

        base_modifier + bonus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_proficiency_bonus_is_two() {
        // SRD PHB Character Advancement table: level 1 proficiency bonus is +2.
        assert_eq!(Skills::default().proficiency_bonus, 2);
    }

    #[test]
    fn test_check_proficiency_and_expertise() {
        let scores = AbilityScores::default(); // all 10 -> +0 modifiers
        let mut skills = Skills::default();
        assert_eq!(skills.check(&Skill::Athletics, &scores), 0);
        skills.proficiencies.set(Skill::Athletics, true);
        assert_eq!(skills.check(&Skill::Athletics, &scores), 2);
        skills.expertise.set(Skill::Athletics, true);
        assert_eq!(skills.check(&Skill::Athletics, &scores), 4);
    }

    #[test]
    fn test_check_override_uses_other_ability() {
        let mut scores = AbilityScores::default();
        scores.cha.base = 16; // +3
        let mut skills = Skills::default();
        skills.overrides.acrobatics = Some(Score::Cha);
        assert_eq!(skills.check(&Skill::Acrobatics, &scores), 3);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_roundtrip_preserves_overrides() {
        let mut skills = Skills::default();
        skills.proficiency_bonus = 4;
        skills.proficiencies.set(Skill::Perception, true);
        skills.expertise.set(Skill::Perception, true);
        skills.overrides.perception = Some(Score::Int);

        let json = serde_json::to_string(&skills).unwrap();
        let back: Skills = serde_json::from_str(&json).unwrap();
        assert_eq!(back, skills);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_missing_proficiency_bonus_defaults_to_two() {
        let back: Skills = serde_json::from_str("{}").unwrap();
        assert_eq!(back.proficiency_bonus, 2);
    }
}
