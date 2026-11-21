//! The unified grant interface.
//!
//! Conditions, feats, features, items, and traits all implement [`Grant`]:
//! they carry a name, an optional source, and a list of [`Effect`]s. A grant
//! is never "applied" by mutating the sheet — [`Character::modifiers`]
//! aggregates the effects of everything currently granted, and derived
//! getters (`skill_check`, `armor_class_total`, `initiative`, ...) read that
//! aggregate. Applying a grant means adding it to the sheet; removing means
//! taking it off. Both are lossless: no state is consumed or overwritten,
//! and removing a grant returns the sheet to exactly its prior effective
//! values.

use crate::{
    Score,
    condition::{Condition, ConditionKind},
    damage::{DamageType, ResistanceState},
    feat::Feat,
    feature::{Effect, Feature},
    meter::Meter,
    senses::Sense,
    sheet::source::Source,
    skills::Skill,
    traits::Trait,
};

/// Anything that can be granted to (and revoked from) a sheet.
pub trait Grant {
    /// The grant's name, used to find and remove it.
    fn grant_name(&self) -> String;
    /// Where the grant comes from (for provenance display).
    fn grant_source(&self) -> Option<&Source>;
    /// The mechanical effects this grant contributes.
    fn effects(&self) -> Vec<Effect>;
}

impl Grant for Feature {
    fn grant_name(&self) -> String {
        self.name.clone()
    }
    fn grant_source(&self) -> Option<&Source> {
        self.source.as_ref()
    }
    fn effects(&self) -> Vec<Effect> {
        self.effects.clone()
    }
}

impl Grant for crate::race::Race {
    fn grant_name(&self) -> String {
        self.name.clone()
    }
    fn grant_source(&self) -> Option<&Source> {
        self.source.as_ref()
    }
    fn effects(&self) -> Vec<Effect> {
        let mut out: Vec<Effect> = self
            .ability_bonuses
            .iter()
            .map(|b| Effect::AbilityScoreBonus(*b))
            .collect();
        if let Some(f) = &self.feature {
            out.extend(f.effects.iter().cloned());
        }
        out
    }
}

impl Grant for Feat {
    fn grant_name(&self) -> String {
        self.name.clone()
    }
    fn grant_source(&self) -> Option<&Source> {
        self.source.as_ref()
    }
    fn effects(&self) -> Vec<Effect> {
        self.effects().to_vec()
    }
}

impl Grant for crate::Item {
    fn grant_name(&self) -> String {
        self.name.clone()
    }
    fn grant_source(&self) -> Option<&Source> {
        self.source.as_ref()
    }
    fn effects(&self) -> Vec<Effect> {
        self.effects().to_vec()
    }
}

impl Grant for Trait {
    fn grant_name(&self) -> String {
        self.name.clone()
    }
    fn grant_source(&self) -> Option<&Source> {
        self.source.as_ref()
    }
    fn effects(&self) -> Vec<Effect> {
        self.effects().to_vec()
    }
}

impl Grant for Condition {
    fn grant_name(&self) -> String {
        self.kind.to_string()
    }
    fn grant_source(&self) -> Option<&Source> {
        None
    }
    /// A condition's mechanics come from its kind, not its effect list.
    fn effects(&self) -> Vec<Effect> {
        Vec::new()
    }
}

/// Everything currently granted, aggregated.
///
/// Built by [`Character::modifiers`]; never stored. Lists (not sets) so
/// provenance survives; queries sum/union as needed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Modifiers {
    pub ac: i64,
    pub initiative: i64,
    pub speed: i64,
    pub score_bonuses: Vec<(Score, i64)>,
    pub skill_bonuses: Vec<(Skill, i64)>,
    pub save_bonuses: Vec<(Score, i64)>,
    pub proficiencies: Vec<Skill>,
    pub expertise: Vec<Skill>,
    pub senses: Vec<Sense>,
    pub meters: Vec<Meter>,
    pub spells: Vec<String>,
    pub condition_immunities: Vec<ConditionKind>,
    /// Condition kinds currently active on the sheet.
    pub condition_kinds: Vec<ConditionKind>,
    pub auto_fail_saves: Vec<Score>,
    pub check_disadvantage: bool,
    pub attack_disadvantage: bool,
    pub attacks_against_advantage: bool,
    pub attacks_against_disadvantage: bool,
    pub cant_take_actions: bool,
    pub speed_zero: bool,
    pub melee_auto_critical: bool,
    pub own_attack_advantage: bool,
    /// Exhaustion level (0-6); SRD effects folded in `finish`.
    pub exhaustion_level: u8,
    pub speed_halved: bool,
    pub all_saves_disadvantaged: bool,
    pub max_hp_halved: bool,

    /// Damage resistances/immunities/vulnerabilities from active grants.
    pub resistances: Vec<DamageType>,
    pub immunities: Vec<DamageType>,
    pub vulnerabilities: Vec<DamageType>,
    /// Resistance to bludgeoning/piercing/slashing from nonmagical attacks
    /// (the common SRD trait).
    pub nonmagical_weapon_resistance: bool,
    /// Resistance to ALL damage types (e.g. Petrified).
    pub all_damage_resistant: bool,

    /// Armor/armor-like AC formulas from grants: (base, dex cap).
    /// `dex_cap: None` = Dex applies fully (unarmored/light);
    /// `Some(0)` = heavy armor (no Dex). Highest base wins.
    pub armor_bases: Vec<(i64, Option<i64>)>,
    /// Scores whose modifier adds to AC (Barbarian/Monk unarmored defense).
    pub ability_ac_bonuses: Vec<Score>,
}

impl Modifiers {
    /// Fold one grant's effects into the aggregate.
    pub fn absorb(&mut self, grant: &dyn Grant) {
        for effect in grant.effects() {
            match effect {
                Effect::AcBonus(b) => self.ac += b,
                Effect::InitiativeBonus(b) => self.initiative += b,
                Effect::SpeedBonus(b) => self.speed += b,
                Effect::AbilityScoreBonus(b) => {
                    self.score_bonuses.push((b.score, b.bonus.into()));
                }
                Effect::SkillBonus { skill, bonus } => {
                    self.skill_bonuses.push((skill, bonus));
                }
                Effect::SaveBonus { score, bonus } => {
                    self.save_bonuses.push((score, bonus));
                }
                Effect::AddProficiency(skill) => self.proficiencies.push(skill),
                Effect::Expertise(skill) => self.expertise.push(skill),
                Effect::Sense(sense) => self.senses.push(sense),
                Effect::Meter(meter) => self.meters.push(meter),
                Effect::Spell(spell) => self.spells.push(spell.name),
                Effect::ConditionImmunity(kind) => {
                    self.condition_immunities.push(kind);
                }
                Effect::Condition(c) => self.condition_kinds.push(c.kind),
                Effect::Resistance(kind) => self.resistances.push(kind),
                Effect::Immunity(kind) => self.immunities.push(kind),
                Effect::Vulnerability(kind) => self.vulnerabilities.push(kind),
                Effect::NonmagicalWeaponResistance => {
                    self.nonmagical_weapon_resistance = true;
                }
                Effect::ArmorClassBase { base, dex_cap } => {
                    self.armor_bases.push((base, dex_cap));
                }
                Effect::AbilityAcBonus(score) => {
                    self.ability_ac_bonuses.push(score);
                }
                // Not computable: health-per-level is tracked on HitPoints,
                // Custom is display-only.
                Effect::HealthBonusPerLevel(_) | Effect::Custom(_) => {}
            }
        }
    }

    /// Fold an active condition's kind into the aggregate.
    pub fn absorb_condition(&mut self, condition: &Condition) {
        self.condition_kinds.push(condition.kind.clone());
    }

    /// Derive the boolean condition flags from the active kinds. Call once
    /// after all grants are absorbed.
    pub fn finish(&mut self) {
        self.condition_kinds.sort();
        self.condition_kinds.dedup();
        for kind in &self.condition_kinds {
            self.check_disadvantage |= kind.ability_checks_disadvantaged();
            self.attack_disadvantage |= kind.target_attack_rolls_disadvantaged();
            self.attacks_against_advantage |= kind.attacks_against_have_advantage();
            self.attacks_against_disadvantage |= kind.attacks_against_have_disadvantage();
            self.cant_take_actions |= kind.cant_take_actions();
            self.speed_zero |= kind.speed_zero();
            self.melee_auto_critical |= kind.melee_hits_are_auto_critical();
            self.own_attack_advantage |= kind.own_attacks_have_advantage();
            for &score in kind.auto_fail_saves() {
                if !self.auto_fail_saves.contains(&score) {
                    self.auto_fail_saves.push(score);
                }
            }
        }
        // SRD exhaustion (Wiki/12 conditions.md): each level includes all
        // lower levels' effects.
        if self.exhaustion_level >= 1 {
            self.check_disadvantage = true;
        }
        if self.exhaustion_level >= 2 {
            self.speed_halved = true;
        }
        if self.exhaustion_level >= 3 {
            self.attack_disadvantage = true;
            self.all_saves_disadvantaged = true;
        }
        if self.exhaustion_level >= 4 {
            self.max_hp_halved = true;
        }
        if self.exhaustion_level >= 5 {
            self.speed_zero = true;
        }

        // Petrified (Wiki/12 conditions.md): resistant to all damage,
        // immune to poison.
        if self.condition_kinds.contains(&ConditionKind::Petrified) {
            self.all_damage_resistant = true;
            if !self.immunities.contains(&DamageType::Poison) {
                self.immunities.push(DamageType::Poison);
            }
        }
    }

    /// The armor formula in effect: highest base wins (SRD: you can't
    /// stack armors, and Mage Armor vs. worn armor just picks the better
    /// formula). Default: unarmored (10, no Dex cap).
    #[must_use]
    pub fn best_armor(&self) -> (i64, Option<i64>) {
        self.armor_bases
            .iter()
            .max_by_key(|(base, _)| *base)
            .copied()
            .unwrap_or((10, None))
    }

    /// How the character takes a damage type. `magical` says whether the
    /// source is magical (relevant to nonmagical-weapon resistance).
    /// SRD: resistances don't stack; immunity wins; resistance +
    /// vulnerability cancel out.
    #[must_use]
    pub fn resistance_state(&self, kind: DamageType, magical: bool) -> ResistanceState {
        if self.immunities.contains(&kind) {
            return ResistanceState::Immune;
        }
        let resistant = self.all_damage_resistant
            || self.resistances.contains(&kind)
            || (self.nonmagical_weapon_resistance && !magical && kind.is_weapon_damage());
        let vulnerable = self.vulnerabilities.contains(&kind);
        match (resistant, vulnerable) {
            (true, true) => ResistanceState::Normal,
            (true, false) => ResistanceState::Resistant,
            (false, true) => ResistanceState::Vulnerable,
            (false, false) => ResistanceState::Normal,
        }
    }

    pub fn score_bonus(&self, score: Score) -> i64 {
        self.score_bonuses
            .iter()
            .filter(|(s, _)| *s == score)
            .map(|(_, b)| b)
            .sum()
    }

    pub fn skill_bonus(&self, skill: Skill) -> i64 {
        self.skill_bonuses
            .iter()
            .filter(|(s, _)| *s == skill)
            .map(|(_, b)| b)
            .sum()
    }

    pub fn save_bonus(&self, score: Score) -> i64 {
        self.save_bonuses
            .iter()
            .filter(|(s, _)| *s == score)
            .map(|(_, b)| b)
            .sum()
    }

    pub fn has_proficiency(&self, skill: Skill) -> bool {
        self.proficiencies.contains(&skill)
    }

    pub fn has_expertise(&self, skill: Skill) -> bool {
        self.expertise.contains(&skill)
    }

    pub fn has_sense(&self, sense: &Sense) -> bool {
        self.senses.contains(sense)
    }

    pub fn has_condition(&self, kind: &ConditionKind) -> bool {
        self.condition_kinds.contains(kind)
    }

    pub fn is_immune_to(&self, kind: &ConditionKind) -> bool {
        self.condition_immunities.contains(kind)
    }

    pub fn auto_fails_save(&self, score: Score) -> bool {
        self.auto_fail_saves.contains(&score)
    }

    /// Advantage state for this sheet's own attack rolls.
    #[must_use]
    pub fn attack_advantage(&self) -> crate::sheet::roll::Advantage {
        crate::sheet::roll::Advantage::resolve(self.own_attack_advantage, self.attack_disadvantage)
    }

    /// Advantage state for attack rolls made *against* this sheet.
    #[must_use]
    pub fn defense_advantage(&self) -> crate::sheet::roll::Advantage {
        crate::sheet::roll::Advantage::resolve(
            self.attacks_against_advantage,
            self.attacks_against_disadvantage,
        )
    }

    /// Advantage state for this sheet's ability checks.
    #[must_use]
    pub fn check_advantage(&self) -> crate::sheet::roll::Advantage {
        crate::sheet::roll::Advantage::resolve(false, self.check_disadvantage)
    }

    /// Advantage state for this sheet's saving throws with `score`
    /// (Restrained: Dex saves at disadvantage).
    #[must_use]
    pub fn save_advantage(&self, score: Score) -> crate::sheet::roll::Advantage {
        let dis = self.all_saves_disadvantaged
            || self
                .condition_kinds
                .iter()
                .any(|k| k.disadvantage_on_saves().contains(&score));
        crate::sheet::roll::Advantage::resolve(false, dis)
    }
}
