use std::fmt::Display;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

use crate::{AbilityScores, class::Classes};

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// The hit points of the character. Default display displays the current health.
pub struct HitPoints {
    /// Current health. Must never be higher than the max.
    pub current: i64,
    /// Max health.
    pub max: i64,
    /// Bonus health gained per level.
    pub level_bonus: i64,
    /// Temporary bonus health.
    pub bonus: i64,
}

impl Default for HitPoints {
    fn default() -> Self {
        Self {
            current: 12,
            max: 12,
            level_bonus: Default::default(),
            bonus: Default::default(),
        }
    }
}

impl HitPoints {
    pub fn new(max: u32) -> Self {
        Self {
            current: max as i64,
            max: max as i64,
            level_bonus: 0,
            bonus: 0,
        }
    }
    /// Calculate new max hp. Only modifies max hp.
    pub fn fixed(self, scores: &AbilityScores, classes: &Classes) -> Self {
        let base_class = &classes[0];

        let base = classes[0].hit_dice.max();
        let die_max = base;
        let con_mod = scores.con.modifier();

        let mut total: i64 =
            base as i64 + con_mod + self.level_bonus + base_class.health_bonus_per_level;

        let fixed_per_level = (die_max / 2 + 1) as i64
            + scores.con.modifier()
            + self.level_bonus
            + base_class.health_bonus_per_level;

        if base_class.level.0 >= 2 {
            total += fixed_per_level * (base_class.level.0 - 1);
        }

        for i in &classes.classes[1..] {
            let die_max = i.hit_dice.max();

            let fixed_per_level = (die_max / 2 + 1) as i64
                + scores.con.modifier()
                + self.level_bonus
                + i.health_bonus_per_level;

            for _ in 0..i.level.0 {
                total += fixed_per_level
            }
        }

        let mut s = self;

        s.max = total.max(1);

        s
    }
    /// Apply damage. Damage is subtracted.
    pub fn hit(&mut self, dmg: i64) {
        if self.bonus > 0 {
            self.bonus -= dmg;
            if self.bonus < 0 {
                self.current -= self.bonus.abs();
                self.bonus = 0;
            }
        } else {
            self.current -= dmg;
        }
        self.current = self.current.max(0);
    }
    pub fn heal(&mut self, healing: i64) {
        self.current += healing;
        self.current = self.current.min(self.max);
    }
}

impl Display for HitPoints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        class::{Class, ClassType, Level},
        roll::Die,
    };

    fn class(class: ClassType, level: i64, die: Die) -> Class {
        Class {
            class,
            subclass: String::new(),
            level: Level(level),
            hit_dice: die,
            health_bonus_per_level: 0,
            healing_die_remaining: 0,
            max_healing_die: 0,
        }
    }

    #[test]
    fn test_fixed_matches_srd_average_method() {
        // PHB: level 1 = die max + CON, each later level = average (die/2+1) + CON.
        // Level 5 fighter with CON 14: 10 + 2 + 4 * (6 + 2) = 44.
        let mut scores = AbilityScores::default();
        scores.con.base = 14;
        let classes = Classes {
            classes: vec![class(ClassType::Fighter, 5, Die::D10)],
        };
        let hp = HitPoints::new(1).fixed(&scores, &classes);
        assert_eq!(hp.max, 44);

        // Level 1 fighter, CON 14: 12.
        let classes = Classes {
            classes: vec![class(ClassType::Fighter, 1, Die::D10)],
        };
        let hp = HitPoints::new(1).fixed(&scores, &classes);
        assert_eq!(hp.max, 12);
    }

    #[test]
    fn test_fixed_multiclass() {
        // Fighter 2 / wizard 1, CON 10: 10 + 6 + 4 = 20.
        let scores = AbilityScores::default();
        let classes = Classes {
            classes: vec![
                class(ClassType::Fighter, 2, Die::D10),
                class(ClassType::Wizard, 1, Die::D6),
            ],
        };
        let hp = HitPoints::new(1).fixed(&scores, &classes);
        assert_eq!(hp.max, 20);
    }

    #[test]
    fn test_temp_hp_absorbs_first() {
        let mut hp = HitPoints::new(10);
        hp.bonus = 3;
        hp.hit(5);
        assert_eq!(hp.bonus, 0);
        assert_eq!(hp.current, 8);
    }

    #[test]
    fn test_hit_clamps_at_zero() {
        let mut hp = HitPoints::new(10);
        hp.hit(15);
        assert_eq!(hp.current, 0);
    }

    #[test]
    fn test_heal_caps_at_max_and_ignores_temp() {
        let mut hp = HitPoints::new(10);
        hp.hit(4);
        hp.heal(100);
        assert_eq!(hp.current, 10);
        // Healing never adds temporary hit points.
        assert_eq!(hp.bonus, 0);
    }
}
