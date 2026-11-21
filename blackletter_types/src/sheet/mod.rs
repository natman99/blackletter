#[cfg(feature = "rhai")]
use rhai::CustomType;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
};
use strum::EnumString;

pub mod ability_score;
pub mod class;
mod hit_points;
mod item;
pub mod roll;
pub mod skills;
pub mod spells;

pub mod condition;
pub mod damage;
pub mod feat;
pub mod feature;
pub mod language;
pub mod lore;
pub mod meter;
pub mod saving_throws;
pub mod senses;
pub mod source;

pub use hit_points::HitPoints;
pub use item::Item;

use ability_score::AbilityScore;

use crate::{
    class::{Class, Classes, Level},
    condition::Conditions,
    feature::Features,
    language::Languages,
    lore::Lore,
    meter::Meters,
    saving_throws::SavingThrows,
    senses::Senses,
    sheet::{class::ClassType, skills::Skills, spells::Spells},
};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]

pub struct Character {
    pub name: String,
    pub race: String,
    pub classes: Classes,
    pub initiative: Initiative,
    pub armor_class: ArmorClass,
    pub health: HitPoints,
    pub inventory: Inventory,
    pub ability_scores: AbilityScores,
    pub skills: Skills,
    pub saving_throws: SavingThrows,
    pub spells: Spells,
    pub senses: Senses,
    pub features: Features,
    pub ability_modifier: Score,
    pub meters: Meters,
    pub lore: Lore,
    pub walking_speed: WalkingSpeed,
    pub languages: Languages,
    pub conditions: Conditions,
}

impl Character {
    pub fn new(
        name: String,
        level: i64,
        health: HitPoints,
        class: ClassType,
        ability_scores: AbilityScores,
    ) -> Self {
        let mut sheet = Self {
            name,
            health,
            classes: Classes {
                classes: vec![Class {
                    level: Level(level),
                    hit_dice: class.get_hit_dice(),
                    health_bonus_per_level: 0,
                    class,
                    healing_die_remaining: level,
                    subclass: String::new(),
                    max_healing_die: level,
                }],
            },
            skills: Skills::default(),
            inventory: Default::default(),
            spells: Default::default(),
            senses: Default::default(),
            initiative: Default::default(),
            armor_class: ArmorClass::new(&ability_scores),
            ability_modifier: Score::Str,
            meters: Default::default(),
            features: Default::default(),
            race: "Elf".to_string(),
            lore: Default::default(),
            walking_speed: Default::default(),
            languages: Default::default(),
            conditions: Default::default(),
            ability_scores,
            saving_throws: Default::default(),
        };
        sheet.sync_proficiency_bonus();
        sheet
    }

    pub fn ability_modifier(&self) -> i64 {
        self.ability_scores.get(&self.ability_modifier).modifier()
    }
    /// Attack bonus.
    /// Ability modifier + proficiency bonus.
    pub fn attack_bonus(&self) -> i32 {
        let f = self.ability_scores.get(&self.ability_modifier).modifier();
        self.skills.proficiency_bonus as i32 + f as i32
    }
    /// Spell attack bonus. Alias for `self.attack_bonus()`
    pub fn spell_bonus(&self) -> i32 {
        self.attack_bonus()
    }
    pub fn save_dc(&self) -> i64 {
        let a = self.ability_scores.get(&self.ability_modifier);
        8 + self.skills.proficiency_bonus + a.modifier()
    }

    pub fn initiative(&self) -> i64 {
        self.initiative.get(&self.ability_scores)
    }

    /// Refresh `skills.proficiency_bonus` from the SRD table for the
    /// character's total level. Call after any level change; a manually
    /// set value is treated as a house-rules override until the next sync.
    pub fn sync_proficiency_bonus(&mut self) {
        self.skills.proficiency_bonus = self.classes.proficiency_bonus();
    }

    /// Spend one hit die from the given class to heal (SRD short-rest
    /// healing; the player chooses each die). `roll` = roll the die,
    /// otherwise take the average. Returns the amount healed.
    pub fn spend_hit_die(&mut self, class_index: usize, roll: bool) -> Option<i64> {
        let before = self.health.current;
        self.classes.heal(
            class_index,
            roll,
            &mut self.health,
            &self.ability_scores,
        );
        let healed = self.health.current - before;
        (healed > 0).then_some(healed)
    }

    /// SRD short rest: meters that restore on a short rest refill.
    /// Hit dice spending is a separate player choice (`spend_hit_die`).
    pub fn short_rest(&mut self) {
        for m in self.meters.meters.values_mut() {
            if m.restore.short_rest {
                m.restore_all();
            }
        }
    }

    /// SRD (2014) long rest: regain all lost hit points, regain spent hit
    /// dice up to half your total (rounded up), and refill long-rest meters.
    pub fn long_rest(&mut self) {
        self.health.heal(self.health.max);
        for c in &mut self.classes.classes {
            let spent = (c.max_healing_die - c.healing_die_remaining).max(0);
            let regained = spent.min((c.max_healing_die + 1) / 2);
            c.healing_die_remaining += regained;
        }
        for m in self.meters.meters.values_mut() {
            // A short-rest resource also recovers after a long rest
            // (SRD: "short or long rest"); both-false meters never auto-restore.
            if m.restore.long_rest || m.restore.short_rest {
                m.restore_all();
            }
        }
    }

    pub fn passive_senses(&self) -> PassiveSenses {
        PassiveSenses {
            insight: self.senses.get_insight(&self.ability_scores, &self.skills),
            perception: self.senses.get_perception(&self.ability_scores, &self.skills),
            investigation: self.senses.get_investigation(&self.ability_scores, &self.skills),
        }
    }
}

impl Default for Character {
    fn default() -> Self {
        Self {
            name: "name".to_string(),
            race: "Elf".to_string(),
            classes: Default::default(),
            initiative: Default::default(),
            armor_class: Default::default(),
            health: Default::default(),
            inventory: Default::default(),
            ability_scores: Default::default(),
            skills: Default::default(),
            saving_throws: Default::default(),
            spells: Default::default(),
            senses: Default::default(),
            features: Default::default(),
            ability_modifier: Default::default(),
            meters: Default::default(),
            lore: Default::default(),
            walking_speed: Default::default(),
            languages: Default::default(),
            conditions: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassiveSenses {
    pub insight: i64,
    pub perception: i64,
    pub investigation: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// An inventory containing a hash map of `Item`s
pub struct Inventory(pub BTreeMap<String, Item>);

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// Ability scores. Defaults to ten.
pub struct AbilityScores {
    #[cfg_attr(feature = "rhai", rhai_type(get = Self::get_str, set = Self::set_str))]
    pub str: AbilityScore,

    #[cfg_attr(feature = "rhai", rhai_type(get = Self::get_dex, set = Self::set_dex))]
    pub dex: AbilityScore,

    #[cfg_attr(feature = "rhai", rhai_type(get = Self::get_con, set = Self::set_con))]
    pub con: AbilityScore,

    #[cfg_attr(feature = "rhai", rhai_type(get = Self::get_int, set = Self::set_int))]
    pub int: AbilityScore,

    #[cfg_attr(feature = "rhai", rhai_type(get = Self::get_wis, set = Self::set_wis))]
    pub wis: AbilityScore,

    #[cfg_attr(feature = "rhai", rhai_type(get = Self::get_cha, set = Self::set_cha))]
    pub cha: AbilityScore,
}

impl AbilityScores {
    /// Collect all scores into an iter
    pub fn iter(&self) -> [&AbilityScore; 6] {
        [
            &self.str, &self.dex, &self.con, &self.int, &self.wis, &self.cha,
        ]
    }

    pub fn get(&self, score: &Score) -> &AbilityScore {
        match score {
            Score::Str => &self.str,
            Score::Dex => &self.dex,
            Score::Con => &self.con,
            Score::Int => &self.int,
            Score::Wis => &self.wis,
            Score::Cha => &self.cha,
        }
    }

    pub fn get_mut(&mut self, score: &Score) -> &mut AbilityScore {
        match score {
            Score::Str => &mut self.str,
            Score::Dex => &mut self.dex,
            Score::Con => &mut self.con,
            Score::Int => &mut self.int,
            Score::Wis => &mut self.wis,
            Score::Cha => &mut self.cha,
        }
    }

    pub fn set_bonus(&mut self, score: &Score, num: i64) {
        match score {
            Score::Str => self.str.set_bonus(num),
            Score::Dex => self.dex.set_bonus(num),
            Score::Con => self.con.set_bonus(num),
            Score::Int => self.int.set_bonus(num),
            Score::Wis => self.wis.set_bonus(num),
            Score::Cha => self.cha.set_bonus(num),
        }
    }

    /// Reset all bonuses.
    pub fn reset(&mut self) {
        self.str.reset();
        self.dex.reset();
        self.con.reset();
        self.int.reset();
        self.cha.reset();
        self.wis.reset();
    }

    pub fn get_str(&self) -> i64 {
        self.str.base
    }

    pub fn set_str(&mut self, new: i64) {
        self.str.base = new;
    }

    pub fn get_dex(&self) -> i64 {
        self.dex.base
    }

    pub fn set_dex(&mut self, new: i64) {
        self.dex.base = new;
    }

    pub fn get_con(&self) -> i64 {
        self.con.base
    }

    pub fn set_con(&mut self, new: i64) {
        self.con.base = new;
    }

    pub fn get_int(&self) -> i64 {
        self.int.base
    }

    pub fn set_int(&mut self, new: i64) {
        self.int.base = new;
    }

    pub fn get_wis(&self) -> i64 {
        self.wis.base
    }

    pub fn set_wis(&mut self, new: i64) {
        self.wis.base = new;
    }

    pub fn get_cha(&self) -> i64 {
        self.cha.base
    }

    pub fn set_cha(&mut self, new: i64) {
        self.cha.base = new;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, EnumString, Hash, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[strum(ascii_case_insensitive)]
/// Scores enum.
pub enum Score {
    #[default]
    Str,
    Dex,
    Con,
    Int,
    Wis,
    Cha,
}

impl Score {
    pub const ALL: [Self; 6] = [
        Self::Str,
        Self::Dex,
        Self::Con,
        Self::Int,
        Self::Wis,
        Self::Cha,
    ];

    pub const ALL_STR: [&str; 6] = ["str", "dex", "con", "int", "wis", "cha"];
}

impl Display for Score {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Score::Str => "Str",
            Score::Dex => "Dex",
            Score::Con => "Con",
            Score::Int => "Int",
            Score::Wis => "Wis",
            Score::Cha => "Cha",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// The armor class of a character.
pub struct ArmorClass(
    #[cfg_attr(feature = "rhai", rhai_type(set = Self::set, get = Self::get))] pub i64,
);

impl ArmorClass {
    /// 10 + dex
    fn new(scores: &AbilityScores) -> Self {
        // safety: negative modifier can never be above 10.
        Self(10 + scores.dex.modifier())
    }

    pub fn get(&self) -> i64 {
        self.0
    }

    pub fn set(&mut self, s: i64) {
        self.0 = s;
    }
}

impl Default for ArmorClass {
    fn default() -> Self {
        Self(10)
    }
}

impl Display for ArmorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// Initiative score.
#[derive(Default)]
pub struct Initiative {
    pub bonus: i64,
}

impl Initiative {
    pub fn get(&self, scores: &AbilityScores) -> i64 {
        scores.get(&Score::Dex).modifier() + self.bonus
    }
}

impl Display for Initiative {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.bonus == 0 {
            write!(f, "0")
        } else {
            write!(f, "{:+}", self.bonus)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// Walking speed in feet.
pub struct WalkingSpeed(pub i64);

impl Default for WalkingSpeed {
    fn default() -> Self {
        Self(30)
    }
}

impl Display for WalkingSpeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ft", &self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meter::{Meter, RestoreTime};
    use std::str::FromStr;

    #[test]
    fn test_scores() {
        Score::from_str("str").unwrap();
        Score::from_str("Str").unwrap();
    }

    #[test]
    fn test_passive_senses_srd_example() {
        // PHB passive checks: 10 + all applicable modifiers.
        // Wis 15, proficient in Perception -> passive 14.
        let mut sheet = Character::default();
        sheet.ability_scores.wis.base = 15;
        sheet
            .skills
            .proficiencies
            .set(skills::Skill::Perception, true);
        assert_eq!(sheet.passive_senses().perception, 14);
        // Investigation scales from Intelligence, not Perception.
        sheet.ability_scores.int.base = 12;
        assert_eq!(sheet.passive_senses().investigation, 11);
    }

    #[test]
    fn test_sync_proficiency_bonus() {
        let mut sheet = Character::new(
            "A".to_string(),
            5,
            HitPoints::new(10),
            ClassType::Fighter,
            AbilityScores::default(),
        );
        assert_eq!(sheet.skills.proficiency_bonus, 3); // level 5 -> +3
        sheet.classes[0].level = Level(17);
        sheet.sync_proficiency_bonus();
        assert_eq!(sheet.skills.proficiency_bonus, 6);
    }

    #[test]
    fn test_long_rest_srd_2014() {
        let mut sheet = Character::new(
            "A".to_string(),
            5,
            HitPoints::new(50),
            ClassType::Fighter,
            AbilityScores::default(),
        );
        for _ in 0..5 {
            sheet.spend_hit_die(0, false);
        }
        sheet.health.hit(30);
        assert_eq!(sheet.health.current, 20);
        assert_eq!(sheet.classes[0].healing_die_remaining, 0);

        sheet.meters.meters.insert(
            "ki".to_string(),
            Meter::new(
                "ki".to_string(),
                3,
                RestoreTime {
                    short_rest: true,
                    long_rest: false,
                },
            ),
        );
        sheet.meters.meters.insert(
            "slots".to_string(),
            Meter::new("slots".to_string(), 4, RestoreTime::default()),
        );
        for _ in 0..3 {
            sheet.meters.meters.get_mut("ki").unwrap().spend();
        }
        for _ in 0..4 {
            sheet.meters.meters.get_mut("slots").unwrap().spend();
        }

        sheet.long_rest();
        assert_eq!(sheet.health.current, 50); // full HP
        // Half of 5 total dice, rounded up: 3 regained.
        assert_eq!(sheet.classes[0].healing_die_remaining, 3);
        // Both meters refill (ki is a "short or long rest" resource).
        assert_eq!(sheet.meters.meters["ki"].spent, 0);
        assert_eq!(sheet.meters.meters["slots"].spent, 0);
    }

    #[test]
    fn test_short_rest_only_refills_short_meters() {
        let mut sheet = Character::default();
        sheet.meters.meters.insert(
            "ki".to_string(),
            Meter::new(
                "ki".to_string(),
                1,
                RestoreTime {
                    short_rest: true,
                    long_rest: false,
                },
            ),
        );
        sheet.meters.meters.insert(
            "slots".to_string(),
            Meter::new(
                "slots".to_string(),
                1,
                RestoreTime {
                    short_rest: false,
                    long_rest: true,
                },
            ),
        );
        sheet.meters.meters.get_mut("ki").unwrap().spend();
        sheet.meters.meters.get_mut("slots").unwrap().spend();

        sheet.short_rest();
        assert_eq!(sheet.meters.meters["ki"].spent, 0);
        assert_eq!(sheet.meters.meters["slots"].spent, 1);
    }
}
