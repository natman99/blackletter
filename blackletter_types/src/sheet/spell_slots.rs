//! SRD spell slot progression tables (2014).
//!
//! Slot counts are DERIVED from class levels; `Character::spell_slots_override`
//! and direct meter edits are the house-rules seams.

use std::collections::BTreeMap;

/// How a class contributes to the spellcaster level table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CasterProgression {
    #[default]
    None,
    /// Bard, Cleric, Druid, Sorcerer, Wizard.
    Full,
    /// Paladin, Ranger: half your class level, rounded up.
    Half,
    /// Artificer: one-third your class level, rounded up.
    Third,
    /// Warlock: Pact Magic, tracked separately from other casters.
    Pact,
}

/// Full caster slots by class level: `FULL_CASTER_TABLE[level-1]` is the
/// list of slot counts for spell levels 1,2,3,... (trailing zeros omitted).
/// Wiki/02 classes.md (Cleric table); identical across the full casters.
const FULL_CASTER_TABLE: [[i64; 9]; 20] = [
    [2, 0, 0, 0, 0, 0, 0, 0, 0],
    [3, 0, 0, 0, 0, 0, 0, 0, 0],
    [4, 2, 0, 0, 0, 0, 0, 0, 0],
    [4, 3, 0, 0, 0, 0, 0, 0, 0],
    [4, 3, 2, 0, 0, 0, 0, 0, 0],
    [4, 3, 3, 0, 0, 0, 0, 0, 0],
    [4, 3, 3, 1, 0, 0, 0, 0, 0],
    [4, 3, 3, 2, 0, 0, 0, 0, 0],
    [4, 3, 3, 3, 1, 0, 0, 0, 0],
    [4, 3, 3, 3, 2, 0, 0, 0, 0],
    [4, 3, 3, 3, 2, 1, 0, 0, 0],
    [4, 3, 3, 3, 2, 1, 0, 0, 0],
    [4, 3, 3, 3, 2, 1, 1, 0, 0],
    [4, 3, 3, 3, 2, 1, 1, 0, 0],
    [4, 3, 3, 3, 2, 1, 1, 1, 0],
    [4, 3, 3, 3, 2, 1, 1, 1, 0],
    [4, 3, 3, 3, 2, 1, 1, 1, 1],
    [4, 3, 3, 3, 3, 1, 1, 1, 0],
    [4, 3, 3, 3, 3, 2, 1, 0, 0],
    [4, 3, 3, 3, 3, 2, 2, 0, 0],
];

/// Pact Magic (Warlock): `PACT_TABLE[level-1] = (slot_count, slot_level)`.
/// Values from Wiki/02 classes.md (the SRD table). NOTE: the free SRD table
/// has a known transcription quirk at 17th-20th (5th/8th slot levels); the
/// printed PHB shows 4 slots of 9th level there. The vault SRD is this
/// project's source of truth; override via meters if your table differs.
const PACT_TABLE: [(i64, i64); 20] = [
    (1, 1),
    (2, 1),
    (2, 2),
    (2, 2),
    (2, 3),
    (2, 3),
    (2, 4),
    (2, 4),
    (2, 5),
    (2, 5),
    (3, 5),
    (3, 6),
    (3, 6),
    (3, 6),
    (3, 7),
    (3, 7),
    (4, 5),
    (4, 8),
    (4, 8),
    (4, 8),
];

/// Slots for a given effective caster level (1..=20): spell level -> count.
#[must_use]
pub fn full_caster_slots(level: i64) -> BTreeMap<i64, i64> {
    let level = level.clamp(1, 20) as usize - 1;
    let mut out = BTreeMap::new();
    for (i, &count) in FULL_CASTER_TABLE[level].iter().enumerate() {
        if count > 0 {
            out.insert(i as i64 + 1, count);
        }
    }
    out
}

/// Pact Magic slots for a Warlock class level: (count, slot_level).
#[must_use]
pub fn pact_magic(level: i64) -> Option<(i64, i64)> {
    if level < 1 {
        return None;
    }
    Some(PACT_TABLE[level.clamp(1, 20) as usize - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_caster_cleric_table_srd() {
        // Wiki/02 classes.md cleric rows.
        assert_eq!(full_caster_slots(1).get(&1), Some(&2));
        assert_eq!(full_caster_slots(3).get(&2), Some(&2));
        assert_eq!(full_caster_slots(5).get(&3), Some(&2));
        assert_eq!(full_caster_slots(9).get(&5), Some(&1));
        assert_eq!(full_caster_slots(17).get(&9), Some(&1));
        assert_eq!(full_caster_slots(18).get(&5), Some(&3));
        assert_eq!(full_caster_slots(20).get(&7), Some(&2));
        // 7th-level slots start at 13th, 9th-level slots at 17th.
        assert_eq!(full_caster_slots(12).get(&7), None);
        assert_eq!(full_caster_slots(13).get(&7), Some(&1));
        assert_eq!(full_caster_slots(16).get(&9), None);
    }

    #[test]
    fn pact_magic_table_srd() {
        // Wiki/02 classes.md warlock rows.
        assert_eq!(pact_magic(1), Some((1, 1)));
        assert_eq!(pact_magic(3), Some((2, 2)));
        assert_eq!(pact_magic(9), Some((2, 5)));
        assert_eq!(pact_magic(11), Some((3, 5)));
        assert_eq!(pact_magic(15), Some((3, 7)));
        assert_eq!(pact_magic(20), Some((4, 8)));
        assert_eq!(pact_magic(0), None);
    }
}
