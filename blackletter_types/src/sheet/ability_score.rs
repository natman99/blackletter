#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

#[derive(Debug, Clone, PartialEq, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// An ability score. Values default to ten.
pub struct AbilityScore {
    /// Base score.
    pub base: i64,
    /// Bonus (from class or otherwise).
    #[cfg_attr(feature = "rhai", rhai_type(skip))]
    bonus: i64,
}

impl Default for AbilityScore {
    fn default() -> Self {
        Self { base: 10, bonus: 0 }
    }
}

impl AbilityScore {
    pub fn new(base: i64, bonus: i64) -> Self {
        Self {
            base: base.max(1),
            bonus,
        }
    }
    /// Get the total score.
    pub fn get(&self) -> i64 {
        self.base + self.bonus
    }

    pub fn get_bonus(&self) -> i64 {
        self.bonus
    }

    pub fn set_bonus(&mut self, num: i64) {
        self.bonus = num;
    }

    pub fn add_bonus(&mut self, num: i64) {
        self.bonus += num;
    }

    /// Remove all bonuses.
    pub fn reset(&mut self) {
        self.bonus = 0;
    }

    pub fn modifier(&self) -> i64 {
        match self.base + self.bonus {
            1 => -5,
            2..=3 => -4,
            4..=5 => -3,
            6..=7 => -2,
            8..=9 => -1,
            10..=11 => 0,
            12..=13 => 1,
            14..=15 => 2,
            16..=17 => 3,
            18..=19 => 4,
            20..=21 => 5,
            22..=23 => 6,
            24..=25 => 7,
            26..=27 => 8,
            28..=29 => 9,
            30 => 10,
            _ => {
                eprintln!("Invalid ability score, returning 0");
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modifier_matches_srd_formula() {
        // SRD: modifier = (score - 10) / 2, rounded down, for scores 1..=30.
        for score in 1..=30 {
            let a = AbilityScore { base: score, bonus: 0 };
            assert_eq!(a.modifier(), (score - 10).div_euclid(2), "score {score}");
        }
    }

    #[test]
    fn test_bonus_math() {
        let mut a = AbilityScore::new(8, 2);
        assert_eq!(a.get(), 10);
        assert_eq!(a.modifier(), 0);
        a.add_bonus(2);
        assert_eq!(a.get(), 12);
        assert_eq!(a.modifier(), 1);
        // set_bonus REPLACES the bonus, it does not add to it.
        a.set_bonus(-2);
        assert_eq!(a.get(), 6);
        assert_eq!(a.modifier(), -2);
        a.reset();
        assert_eq!(a.get_bonus(), 0);
        assert_eq!(a.get(), 8);
    }

    #[test]
    fn test_new_clamps_base_to_one() {
        assert_eq!(AbilityScore::new(0, 0).get(), 1);
        assert_eq!(AbilityScore::new(-5, 0).get(), 1);
    }

    #[test]
    fn test_out_of_range_score_falls_back_to_zero() {
        // Current documented fallback: scores outside 1..=30 warn and return 0.
        let a = AbilityScore { base: 31, bonus: 0 };
        assert_eq!(a.modifier(), 0);
    }
}
