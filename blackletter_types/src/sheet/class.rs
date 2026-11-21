use std::{
    fmt::Display,
    ops::{Deref, Index, IndexMut},
    str::FromStr,
};

use rand::RngExt;
#[cfg(feature = "rhai")]
use rhai::CustomType;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{AbilityScores, HitPoints, roll::Die};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
pub struct Classes {
    pub classes: Vec<Class>,
}

impl Classes {
    /// Total character level across all classes. SRD: proficiency bonus and
    /// cantrip scaling use total character level, not per-class level.
    #[must_use]
    pub fn total_level(&self) -> i64 {
        self.classes.iter().map(|c| c.level.0).sum()
    }

    /// SRD PHB Character Advancement table: proficiency bonus by total
    /// character level (levels 1-4: +2, 5-8: +3, 9-12: +4, 13-16: +5, 17-20: +6).
    #[must_use]
    pub fn proficiency_bonus(&self) -> i64 {
        match self.total_level().clamp(1, 20) {
            1..=4 => 2,
            5..=8 => 3,
            9..=12 => 4,
            13..=16 => 5,
            _ => 6,
        }
    }

    pub fn heal(
        &mut self,
        index: usize,
        roll: bool,
        health: &mut HitPoints,
        ability_scores: &AbilityScores,
    ) {
        self[index].heal(roll, health, ability_scores);
    }

    pub fn heal_first(
        &mut self,
        roll: bool,
        health: &mut HitPoints,
        ability_scores: &AbilityScores,
    ) -> Option<()> {
        if let Some(class) = self
            .classes
            .iter_mut()
            .find(|f| f.healing_die_remaining > 0)
        {
            class.heal(roll, health, ability_scores);
            Some(())
        } else {
            None
        }
    }

    #[cfg(feature = "rhai")]
    pub fn get(&mut self, index: i64) -> Class {
        self.classes[index as usize].clone()
    }

    #[cfg(feature = "rhai")]
    pub fn set(&mut self, index: i64, value: Class) {
        self.classes[index as usize] = value;
    }
}

impl Default for Classes {
    fn default() -> Self {
        let class = Class::default();
        Self {
            classes: vec![class],
        }
    }
}

impl Display for Classes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        for i in &self.classes {
            s.push_str(&format!("{} {} ", i.class, i.level));
        }
        write!(f, "{}", s)
    }
}

impl Deref for Classes {
    type Target = Vec<Class>;

    fn deref(&self) -> &Self::Target {
        &self.classes
    }
}

impl Index<usize> for Classes {
    type Output = Class;

    fn index(&self, index: usize) -> &Self::Output {
        &self.classes[index]
    }
}

impl IndexMut<usize> for Classes {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.classes[index]
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
pub struct Class {
    pub class: ClassType,
    pub subclass: String,
    #[cfg_attr(feature = "rhai", rhai_type(get = Self::get_level), rhai_type(set = Self::set_level))]
    pub level: Level,
    pub hit_dice: Die,
    pub health_bonus_per_level: i64,
    pub healing_die_remaining: i64,
    pub max_healing_die: i64,
}

impl Default for Class {
    fn default() -> Self {
        let class: ClassType = Default::default();
        let hit_dice = class.get_hit_dice();
        Self {
            class,
            level: Default::default(),
            hit_dice,
            health_bonus_per_level: 0,
            healing_die_remaining: 1,
            subclass: "Battlemaster".to_string(),
            max_healing_die: 1,
        }
    }
}

impl Class {
    pub fn heal(&mut self, roll: bool, health: &mut HitPoints, ability_scores: &AbilityScores) {
        if self.healing_die_remaining == 0 {
            return;
        }
        // SRD: spend a Hit Die; recover either a rolled amount or the
        // average (die/2 + 1), plus the CON modifier either way.
        let healing_amount = if roll {
            let mut rng = rand::rng();
            rng.random_range(1..=self.hit_dice.max() as i64)
        } else {
            self.hit_dice.max() as i64 / 2 + 1
        } + ability_scores.con.modifier();
        health.heal(healing_amount.max(1));
        self.healing_die_remaining = self.healing_die_remaining.saturating_sub(1);
    }

    pub fn get_level(&self) -> i64 {
        self.level.0
    }

    pub fn set_level(&mut self, level: i64) {
        self.level.0 = level;
        self.max_healing_die = level;
    }

    pub fn set_class(&mut self, class: &str) {
        // Cannot fail
        let Ok(class) = ClassType::from_str(class) else {
            return;
        };
        self.class = class;
        self.hit_dice = self.class.get_hit_dice();
    }

    pub fn new(level: Level, class: &str) -> Self {
        let class = ClassType::from_str(class).expect("Cannot fail");
        Self {
            subclass: "".to_string(),
            level,
            hit_dice: class.get_hit_dice(),
            health_bonus_per_level: 0,
            healing_die_remaining: level.0,
            class,
            max_healing_die: level.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(from = "String"))]
/// The class of a character. `from_str` is infailable on this type.
/// Implements `From<&str>` and `From<String>`
pub enum ClassType {
    #[default]
    Fighter,
    Wizard,
    Sorcerer,
    Cleric,
    Barbarian,
    Monk,
    Ranger,
    Paladin,
    Rogue,
    Warlock,
    Bard,
    Druid,
    Artificer,
    #[cfg_attr(feature = "serde", serde(untagged))]
    Other(String),
}

/// How a class keeps its spells in mind, per SRD 2014
/// "Known and Prepared Spells" (`Wiki/08 spellcasting.md:23-25`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CastingMechanic {
    /// Cleric, paladin, ranger, druid, artificer: a prepared list chosen
    /// each day after a long rest.
    #[default]
    Prepared,
    /// Wizard: prepared, but from a spellbook — copying new spells into
    /// the book is its own resource loop.
    PreparedWithSpellbook,
    /// Bard, sorcerer, warlock: a fixed list of spells known.
    Known,
}

impl ClassType {
    /// The SRD 2014 casting mechanic for this class, or `None` for classes
    /// with no spellcasting (subclass features like the Eldritch Knight
    /// are handled by granting the mechanic at the feature layer later).
    #[must_use]
    pub fn casting_mechanic(&self) -> Option<CastingMechanic> {
        match self {
            Self::Cleric | Self::Paladin | Self::Ranger | Self::Druid | Self::Artificer => {
                Some(CastingMechanic::Prepared)
            }
            Self::Wizard => Some(CastingMechanic::PreparedWithSpellbook),
            Self::Bard | Self::Sorcerer | Self::Warlock => Some(CastingMechanic::Known),
            Self::Fighter | Self::Barbarian | Self::Monk | Self::Rogue | Self::Other(_) => None,
        }
    }

    /// How this class contributes to spellcaster level (SRD 2014).
    /// `Other(_)` defaults to no casting; grant it slots via
    /// `Character::spell_slots_override` or meters.
    #[must_use]
    pub fn caster_progression(&self) -> crate::spell_slots::CasterProgression {
        use crate::spell_slots::CasterProgression::*;
        match self {
            Self::Bard | Self::Cleric | Self::Druid | Self::Sorcerer | Self::Wizard => Full,
            Self::Paladin | Self::Ranger => Half,
            Self::Artificer => Third,
            Self::Warlock => Pact,
            _ => None,
        }
    }
}

impl From<String> for ClassType {
    fn from(value: String) -> Self {
        Self::from_str(&value).unwrap()
    }
}

impl From<&str> for ClassType {
    fn from(value: &str) -> Self {
        Self::from_str(value).unwrap()
    }
}

impl FromStr for ClassType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "fighter" => ClassType::Fighter,
            "wizard" => ClassType::Wizard,
            "sorcerer" => ClassType::Sorcerer,
            "cleric" => ClassType::Cleric,
            "barbarian" => ClassType::Barbarian,
            "monk" => ClassType::Monk,
            "ranger" => ClassType::Ranger,
            "paladin" => ClassType::Paladin,
            "rogue" => ClassType::Rogue,
            "warlock" => ClassType::Warlock,
            "bard" => ClassType::Bard,
            "druid" => ClassType::Druid,
            _ => ClassType::Other(s.to_string()),
        })
    }
}

impl ClassType {
    pub fn get_hit_dice(&self) -> Die {
        match self {
            ClassType::Fighter => Die::D10,
            ClassType::Wizard => Die::D6,
            ClassType::Sorcerer => Die::D6,
            ClassType::Cleric => Die::D8,
            ClassType::Barbarian => Die::D12,
            ClassType::Monk => Die::D8,
            ClassType::Ranger => Die::D10,
            ClassType::Paladin => Die::D10,
            ClassType::Rogue => Die::D8,
            ClassType::Warlock => Die::D8,
            ClassType::Bard => Die::D8,
            ClassType::Druid => Die::D8,
            ClassType::Artificer => Die::D8,
            ClassType::Other(_) => Die::D8,
        }
    }
}

impl Display for ClassType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &str = match &self {
            ClassType::Fighter => "Fighter",
            ClassType::Wizard => "Wizard",
            ClassType::Sorcerer => "Sorcerer",
            ClassType::Cleric => "Cleric",
            ClassType::Barbarian => "Barbarian",
            ClassType::Monk => "Monk",
            ClassType::Ranger => "Ranger",
            ClassType::Paladin => "Paladin",
            ClassType::Rogue => "Rogue",
            ClassType::Warlock => "Warlock",
            ClassType::Bard => "Bard",
            ClassType::Druid => "Druid",
            ClassType::Artificer => "Artificer",
            ClassType::Other(name) => name.as_str(),
        };

        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Level(pub i64);

impl Default for Level {
    fn default() -> Self {
        Self(1)
    }
}

impl Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        AbilityScores, HitPoints,
        class::{Class, ClassType, Classes, Level},
    };

    #[test]
    fn from_other() {
        ClassType::from_str("test").unwrap();
    }

    #[test]
    fn proficiency_bonus_matches_srd_table() {
        // Wiki/03 beyond1st, Character Advancement table.
        let expected = |level: i64| match level {
            1..=4 => 2,
            5..=8 => 3,
            9..=12 => 4,
            13..=16 => 5,
            17..=20 => 6,
            _ => unreachable!(),
        };
        for level in 1..=20 {
            let classes = Classes {
                classes: vec![Class::new(Level(level), "fighter")],
            };
            assert_eq!(
                classes.proficiency_bonus(),
                expected(level),
                "level {level}"
            );
        }
        // Multiclass uses total level: fighter 3 / wizard 2 -> level 5 -> +3.
        let classes = Classes {
            classes: vec![
                Class::new(Level(3), "fighter"),
                Class::new(Level(2), "wizard"),
            ],
        };
        assert_eq!(classes.proficiency_bonus(), 3);
    }

    #[test]
    fn heal_average_method_adds_con() {
        let mut scores = AbilityScores::default();
        scores.con.base = 14; // +2
        let mut c = Class::new(Level(1), "fighter");
        let mut hp = HitPoints::new(20);
        hp.hit(10);
        // d10 average (5 + 1) + CON 2 = 8.
        c.heal(false, &mut hp, &scores);
        assert_eq!(hp.current, 18);
        assert_eq!(c.healing_die_remaining, 0);
        // Out of dice: no-op.
        c.heal(false, &mut hp, &scores);
        assert_eq!(hp.current, 18);
    }
}
