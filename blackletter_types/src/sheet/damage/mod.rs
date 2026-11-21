#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use strum::EnumString;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq, EnumString)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[strum(ascii_case_insensitive)]
pub enum DamageType {
    Acid,
    Bludgeoning,
    Cold,
    Fire,
    Force,
    Lightning,
    Necrotic,
    Piercing,
    Poison,
    Psychic,
    Radiant,
    Slashing,
    Thunder,
    None,
}

impl DamageType {
    /// The 13 SRD damage types (excludes `None`).
    pub const ALL: [DamageType; 13] = [
        DamageType::Acid,
        DamageType::Bludgeoning,
        DamageType::Cold,
        DamageType::Fire,
        DamageType::Force,
        DamageType::Lightning,
        DamageType::Necrotic,
        DamageType::Piercing,
        DamageType::Poison,
        DamageType::Psychic,
        DamageType::Radiant,
        DamageType::Slashing,
        DamageType::Thunder,
    ];

    /// Bludgeoning/piercing/slashing: the types affected by the common
    /// "resistance to nonmagical weapon damage" trait.
    #[must_use]
    pub fn is_weapon_damage(&self) -> bool {
        matches!(
            self,
            DamageType::Bludgeoning | DamageType::Piercing | DamageType::Slashing
        )
    }
}

/// How a creature takes a given damage type (PHB 2014: resistant = half,
/// vulnerable = double, immune = none; multiple resistances of the same
/// kind don't stack, and resistance + vulnerability cancel out).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum ResistanceState {
    #[default]
    Normal,
    Resistant,
    Vulnerable,
    Immune,
}

impl ResistanceState {
    /// Apply this state to an incoming damage amount.
    #[must_use]
    pub fn apply(&self, amount: i64) -> i64 {
        match self {
            ResistanceState::Normal => amount,
            ResistanceState::Resistant => amount / 2,
            ResistanceState::Vulnerable => amount * 2,
            ResistanceState::Immune => 0,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::str::FromStr;
    #[test]
    fn test_damage() {
        DamageType::from_str("acid").unwrap();

        DamageType::from_str("Acid").unwrap();
    }
}
