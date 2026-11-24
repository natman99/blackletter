use std::collections::BTreeSet;

use enumflags2::{BitFlags, bitflags};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[bitflags]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
/// Armor proficiency categories (SRD Proficiencies, `Wiki/01`/`02`).
pub enum ArmorProf {
    Light,
    Medium,
    Heavy,
    Shields,
}

#[bitflags]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
/// Weapon proficiency categories. Specific weapons (longswords, rapiers,
/// ...) live in `Proficiencies::specific` — the SRD grants those by name
/// (rogue proficiencies, Weapon Master feat).
pub enum WeaponProf {
    Simple,
    Martial,
}

/// Typed weapon/armor/tool proficiencies on the sheet. Skills and saving
/// throws have their own fields (`Skills`, `SavingThrows`); everything
/// the SRD lists as a proficiency reference lands here.
///
/// `specific` is the custom/homebrew seam: it holds specific weapon and
/// tool indexes ("longswords", "thieves-tools") and accepts ANY free-form
/// string for house rules ("brewer's supplies", "custom:firearms") —
/// nothing validates it against the corpus.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Proficiencies {
    pub armor: BitFlags<ArmorProf>,
    pub weapons: BitFlags<WeaponProf>,
    /// Specific weapons, tools, and custom proficiencies by SRD index or
    /// free-form name. Sorted set; never validated.
    pub specific: BTreeSet<String>,
}

/// BitFlags has no JsonSchema impl (same trick as `SkillsJson` in
/// skills.rs): the agent-facing schema shows plain string lists.
#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[allow(dead_code)] // schema-only shadow: fields shape the JSON schema, never read
struct ProficienciesSchema {
    armor: Vec<String>,
    weapons: Vec<String>,
    specific: Vec<String>,
}

#[cfg(feature = "schema")]
impl JsonSchema for Proficiencies {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Proficiencies".into()
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::schema_for!(ProficienciesSchema)
    }
}

impl Proficiencies {
    /// Add one proficiency from a corpus reference index (class
    /// `proficiencies`, `multi_classing.proficiencies`, background
    /// `starting_proficiencies`, ...). Skill and saving-throw indexes
    /// belong to their own sheet fields and are ignored here. Returns
    /// true if this proficiency was newly added (false = already had it
    /// or it was not this field's kind).
    pub fn add_index(&mut self, index: &str) -> bool {
        match index {
            "light-armor" => {
                let before = self.armor;
                self.armor.insert(ArmorProf::Light);
                before != self.armor
            }
            "medium-armor" => {
                let before = self.armor;
                self.armor.insert(ArmorProf::Medium);
                before != self.armor
            }
            "heavy-armor" => {
                let before = self.armor;
                self.armor.insert(ArmorProf::Heavy);
                before != self.armor
            }
            "shields" => {
                let before = self.armor;
                self.armor.insert(ArmorProf::Shields);
                before != self.armor
            }
            "all-armor" => {
                // Fighter's row spells it "all-armor" + "shields"
                // separately; the category means light+medium+heavy.
                let before = self.armor;
                self.armor
                    .insert(ArmorProf::Light | ArmorProf::Medium | ArmorProf::Heavy);
                before != self.armor
            }
            "simple-weapons" => {
                let before = self.weapons;
                self.weapons.insert(WeaponProf::Simple);
                before != self.weapons
            }
            "martial-weapons" => {
                let before = self.weapons;
                self.weapons.insert(WeaponProf::Martial);
                before != self.weapons
            }
            other if other.starts_with("skill-") || other.starts_with("saving-throw-") => false,
            other => self.specific.insert(other.to_string()),
        }
    }

    #[must_use]
    pub fn has_armor(&self, prof: ArmorProf) -> bool {
        self.armor.contains(prof)
    }

    #[must_use]
    pub fn has_weapon(&self, prof: WeaponProf) -> bool {
        self.weapons.contains(prof)
    }

    /// True if the sheet has this specific proficiency (exact index or
    /// free-form name match).
    #[must_use]
    pub fn has(&self, index: &str) -> bool {
        self.specific.contains(index)
    }

    /// Human-readable labels for the bitflag categories, for display.
    #[must_use]
    pub fn category_labels(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (flag, label) in [
            (ArmorProf::Light, "light armor"),
            (ArmorProf::Medium, "medium armor"),
            (ArmorProf::Heavy, "heavy armor"),
            (ArmorProf::Shields, "shields"),
        ] {
            if self.armor.contains(flag) {
                out.push(label);
            }
        }
        for (flag, label) in [
            (WeaponProf::Simple, "simple weapons"),
            (WeaponProf::Martial, "martial weapons"),
        ] {
            if self.weapons.contains(flag) {
                out.push(label);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_and_multiclass_proficiency_indexes_land_typed() {
        let mut p = Proficiencies::default();
        // Fighter single-class row.
        for idx in [
            "all-armor",
            "shields",
            "simple-weapons",
            "martial-weapons",
            "saving-throw-str",
            "saving-throw-con",
        ] {
            p.add_index(idx);
        }
        assert!(p.has_armor(ArmorProf::Heavy) && p.has_armor(ArmorProf::Shields));
        assert!(p.has_weapon(WeaponProf::Martial));
        assert!(
            p.specific.is_empty(),
            "saves/skills never reach the specific set"
        );
        // Rogue's specific weapons + tools.
        for idx in ["longswords", "rapiers", "thieves-tools"] {
            p.add_index(idx);
        }
        assert!(p.has("longswords") && p.has("thieves-tools"));
        // Custom/homebrew strings are accepted unvalidated.
        p.add_index("custom:firearms");
        assert!(p.has("custom:firearms"));
    }

    #[test]
    fn add_index_reports_new_only() {
        let mut p = Proficiencies::default();
        assert!(p.add_index("shields"));
        assert!(!p.add_index("shields"), "already had it");
        assert!(!p.add_index("skill-arcana"), "not this field's kind");
    }
}
