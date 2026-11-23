#[cfg(feature = "rhai")]
use rhai::CustomType;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use std::{collections::BTreeMap, fmt::Display};
use strum::EnumString;

pub mod ability_score;
pub mod background;
pub mod class;
mod hit_points;
mod item;
pub mod roll;
pub mod skills;
pub mod spells;

pub mod condition;
pub mod damage;
pub mod death;
pub mod feat;
pub mod feature;
pub mod grant;
pub mod language;
pub mod levelup;
pub mod lore;
pub mod meter;
pub mod race;
pub mod saving_throws;
pub mod senses;
pub mod source;
pub mod spell_slots;
pub mod traits;

pub use background::Background;
pub use hit_points::HitPoints;
pub use item::Item;

use ability_score::AbilityScore;

use crate::{
    class::{Class, Classes, Level},
    condition::{Condition, Conditions},
    feat::Feat,
    feature::Features,
    grant::Modifiers,
    language::Languages,
    lore::Lore,
    meter::{Meter, Meters, RestoreTime},
    race::Race,
    saving_throws::SavingThrows,
    senses::Senses,
    sheet::{
        class::ClassType,
        skills::{Skill, Skills},
        spells::Spells,
    },
    traits::Trait,
};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]

pub struct Character {
    pub name: String,
    pub race: Option<Race>,
    /// Gold pieces: pays consumed material component costs.
    pub gold: i64,
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
    pub feats: Vec<Feat>,
    pub traits: Vec<Trait>,
    pub ability_modifier: Score,
    pub meters: Meters,
    pub lore: Lore,
    pub walking_speed: WalkingSpeed,
    pub languages: Languages,
    pub conditions: Conditions,
    pub death_saves: crate::death::DeathSaves,
    /// Exhaustion level 0..=6 (SRD). Set directly for house rules; the
    /// effects are derived in `Modifiers::finish`.
    pub exhaustion: u8,
    /// The character's background, if chosen. Its mechanical effects
    /// (skill proficiencies) flow through `modifiers()` like other grants.
    #[cfg_attr(feature = "serde", serde(default))]
    pub background: Option<Background>,
    /// The spell currently being concentrated on, if any. SRD
    /// "Concentration" (`Wiki/08 spellcasting.md:120-129`): casting
    /// another concentration spell breaks the old; damage forces a CON
    /// save vs DC 10 or half the damage (min 10).
    #[cfg_attr(feature = "serde", serde(default))]
    pub concentrating: Option<Concentration>,
    /// Set by `long_rest` for prepared casters: the day's prepared list is
    /// spent and the UI should prompt "prepare spells" (SRD: a wizard
    /// needs 1 minute of prayer after a long rest to prepare; a cleric
    /// prepares after finishing a long rest). Plain bookkeeping — no
    /// timer modeling. Cleared by `finish_spell_prep`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub needs_spell_prep: bool,
    /// House-rules seam for spell slots: slot counts for spell levels
    /// 1..=9 (index 0 = 1st level). When set, replaces the derived table
    /// until cleared; Pact Magic is unaffected (edit its meter directly).
    pub spell_slots_override: Option<Vec<i64>>,
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
            needs_spell_prep: false,
            concentrating: None,
            background: None,
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
            race: Some(Race::new("Elf")),
            gold: 0,
            lore: Default::default(),
            walking_speed: Default::default(),
            languages: Default::default(),
            conditions: Default::default(),
            feats: Vec::new(),
            traits: Vec::new(),
            death_saves: Default::default(),
            exhaustion: 0,
            spell_slots_override: None,
            ability_scores,
            saving_throws: Default::default(),
        };
        sheet.sync_proficiency_bonus();
        sheet
    }

    pub fn ability_modifier(&self) -> i64 {
        self.ability_modifier_for(self.ability_modifier)
    }
    /// Ability modifier including any score bonuses granted by feats,
    /// items, features, or traits.
    pub fn ability_modifier_for(&self, score: Score) -> i64 {
        self.effective_modifier(score, &self.modifiers())
    }
    fn effective_modifier(&self, score: Score, m: &Modifiers) -> i64 {
        let total = self.ability_scores.get(&score).get() + m.score_bonus(score);
        (total - 10).div_euclid(2)
    }
    /// Attack bonus.
    /// Ability modifier + proficiency bonus.
    pub fn attack_bonus(&self) -> i32 {
        let f = self.ability_modifier_for(self.ability_modifier);
        self.skills.proficiency_bonus as i32 + f as i32
    }
    /// Spell attack bonus. Alias for `self.attack_bonus()`
    pub fn spell_bonus(&self) -> i32 {
        self.attack_bonus()
    }
    pub fn save_dc(&self) -> i64 {
        8 + self.skills.proficiency_bonus + self.ability_modifier_for(self.ability_modifier)
    }

    pub fn initiative(&self) -> i64 {
        let m = self.modifiers();
        let dex_delta =
            self.effective_modifier(Score::Dex, &m) - self.ability_scores.dex.modifier();
        self.initiative.get(&self.ability_scores) + dex_delta + m.initiative
    }

    /// Aggregate every currently-granted effect (features, feats, equipped
    /// items, traits, active conditions). Never stored, so removing a grant
    /// can't leave residue behind.
    #[must_use]
    pub fn modifiers(&self) -> Modifiers {
        let mut m = Modifiers::default();
        for f in &self.features.inner {
            m.absorb(f);
        }
        for f in &self.feats {
            m.absorb(f);
        }
        for item in self.inventory.0.values() {
            if item.equipped {
                m.absorb(item);
            }
        }
        for t in &self.traits {
            m.absorb(t);
        }
        if let Some(race) = &self.race {
            m.absorb(race);
        }
        if let Some(bg) = &self.background {
            m.absorb(bg);
        }
        for c in &self.conditions.conditions {
            m.absorb_condition(c);
        }
        m.exhaustion_level = self.exhaustion;
        m.finish();
        m
    }

    /// Skill check including granted bonuses, proficiencies, and expertise.
    #[must_use]
    pub fn skill_check(&self, skill: &Skill) -> i64 {
        let m = self.modifiers();
        let score = self.skills.score_for(skill);
        let score_delta =
            self.effective_modifier(score, &m) - self.ability_scores.get(&score).modifier();
        let mut total =
            self.skills.check(skill, &self.ability_scores) + score_delta + m.skill_bonus(*skill);
        // Granted proficiency/expertise only add what the base sheet lacks.
        if m.has_proficiency(*skill) && !self.skills.proficiencies.contains(*skill) {
            total += self.skills.proficiency_bonus;
        }
        if m.has_expertise(*skill) && !self.skills.expertise.contains(*skill) {
            total += self.skills.proficiency_bonus;
        }
        total
    }

    /// Saving throw total including granted bonuses. Does not apply
    /// condition auto-fails; check `auto_fails_save` at the roll site.
    #[must_use]
    pub fn save_total(&self, score: &Score) -> i64 {
        let m = self.modifiers();
        let score_delta =
            self.effective_modifier(*score, &m) - self.ability_scores.get(score).modifier();
        self.saving_throws
            .check(score, &self.ability_scores, &self.skills)
            + score_delta
            + m.save_bonus(*score)
    }

    /// True if an active condition makes this save an automatic failure.
    #[must_use]
    pub fn auto_fails_save(&self, score: Score) -> bool {
        self.modifiers().auto_fails_save(score)
    }

    /// True if the sheet can currently be subjected to this condition.
    #[must_use]
    pub fn can_be_conditioned(&self, kind: &crate::condition::ConditionKind) -> bool {
        !self.modifiers().is_immune_to(kind)
    }

    #[must_use]
    pub fn can_take_actions(&self) -> bool {
        !self.modifiers().cant_take_actions
    }

    /// Effective armor class: the best armor formula in effect
    /// (unarmored 10 + Dex by default; armor/Mage Armor grants set bases
    /// with Dex caps), plus the modifier of any scores granted via
    /// `AbilityAcBonus` (Barbarian Con, Monk Wis), plus flat AC bonuses
    /// (shields, rings). `armor_class.flat` overrides the formula outright
    /// (monsters, house rules) but still receives the bonuses.
    #[must_use]
    pub fn armor_class_total(&self) -> i64 {
        let m = self.modifiers();
        let ability_ac: i64 = m
            .ability_ac_bonuses
            .iter()
            .map(|s| self.effective_modifier(*s, &m))
            .sum();
        let base = match self.armor_class.flat {
            Some(flat) => flat,
            None => {
                let (base, cap) = m.best_armor();
                let dex = self.effective_modifier(Score::Dex, &m);
                base + dex.min(cap.unwrap_or(i64::MAX))
            }
        };
        base + m.ac + ability_ac
    }

    /// Walking speed including granted speed bonuses.
    #[must_use]
    pub fn speed_total(&self) -> i64 {
        let m = self.modifiers();
        if m.speed_zero {
            0
        } else {
            let base = self
                .race
                .as_ref()
                .and_then(|r| r.speed)
                .unwrap_or(self.walking_speed.0);
            let total = base + m.speed;
            if m.speed_halved { total / 2 } else { total }
        }
    }

    /// Add a feature grant (its `Effect::Meter` grants materialize into
    /// live meters immediately).
    pub fn add_feature(&mut self, feature: crate::feature::Feature) {
        self.features.inner.push(feature);
        self.sync_grant_meters();
    }
    /// Add a feat grant.
    pub fn add_feat(&mut self, feat: Feat) {
        self.feats.push(feat);
        self.sync_grant_meters();
    }
    /// Add a trait grant.
    pub fn add_trait(&mut self, trait_: Trait) {
        self.traits.push(trait_);
        self.sync_grant_meters();
    }
    /// Add a condition (respects condition immunities; returns whether it
    /// was applied).
    pub fn add_condition(&mut self, condition: Condition) -> bool {
        if !self.can_be_conditioned(&condition.kind) {
            return false;
        }
        self.conditions.conditions.push(condition);
        true
    }
    /// Equip an inventory item by name so its effects count.
    pub fn equip(&mut self, name: &str) -> bool {
        match self.inventory.0.get_mut(name) {
            Some(item) => {
                item.equipped = true;
                true
            }
            None => false,
        }
    }
    /// Unequip an inventory item by name, revoking its effects.
    pub fn unequip(&mut self, name: &str) -> bool {
        match self.inventory.0.get_mut(name) {
            Some(item) if item.equipped => {
                item.equipped = false;
                true
            }
            _ => false,
        }
    }
    /// Remove the first grant matching `name` (case-insensitive): feature,
    /// feat, trait, or condition. Equipped items are unequipped rather than
    /// deleted. Returns whether anything was removed.
    pub fn remove_grant(&mut self, name: &str) -> bool {
        if let Some(i) = self
            .features
            .inner
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(name))
        {
            self.features.inner.remove(i);
            return true;
        }
        if let Some(i) = self
            .feats
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(name))
        {
            self.feats.remove(i);
            return true;
        }
        if let Some(i) = self
            .traits
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name))
        {
            self.traits.remove(i);
            return true;
        }
        if let Some(i) = self
            .conditions
            .conditions
            .iter()
            .position(|c| c.kind.to_string().eq_ignore_ascii_case(name))
        {
            self.conditions.conditions.remove(i);
            return true;
        }
        // Items: match the inventory key case-insensitively, then unequip.
        if let Some(key) = self
            .inventory
            .0
            .keys()
            .find(|k| k.eq_ignore_ascii_case(name))
            .cloned()
        {
            return self.unequip(&key);
        }
        false
    }
    /// Whether any grant (feature, feat, trait, condition, equipped item)
    /// matches `name` (case-insensitive).
    #[must_use]
    pub fn has_grant(&self, name: &str) -> bool {
        self.features
            .inner
            .iter()
            .any(|f| f.name.eq_ignore_ascii_case(name))
            || self.feats.iter().any(|f| f.name.eq_ignore_ascii_case(name))
            || self
                .traits
                .iter()
                .any(|t| t.name.eq_ignore_ascii_case(name))
            || self
                .conditions
                .conditions
                .iter()
                .any(|c| c.kind.to_string().eq_ignore_ascii_case(name))
            || self
                .inventory
                .0
                .values()
                .any(|i| i.equipped && i.name.eq_ignore_ascii_case(name))
    }

    /// Advance condition durations by one turn, removing expired ones.
    pub fn tick_conditions(&mut self) {
        for c in &mut self.conditions.conditions {
            c.tick();
        }
        self.conditions.conditions.retain(|c| c.duration != 0);
    }

    // ---- d20 rolls resolved against the sheet ----

    /// Roll a d20 with `bonus`, applying the resolved advantage state.
    fn roll_d20(
        &self,
        rng: &mut impl rand::Rng,
        bonus: i64,
        advantage: crate::sheet::roll::Advantage,
    ) -> crate::sheet::roll::RollResult {
        crate::sheet::roll::Roll::new(&[crate::sheet::roll::Die::D20], bonus as i32)
            .roll(rng, advantage.to_modifier())
    }

    /// Ability check: granted bonuses folded in, condition disadvantage
    /// resolved, combined with any situational `extra` advantage.
    #[must_use]
    pub fn roll_check(
        &self,
        rng: &mut impl rand::Rng,
        skill: &Skill,
        extra: crate::sheet::roll::Advantage,
    ) -> RollOutcome {
        let m = self.modifiers();
        let advantage = m.check_advantage().combine(extra);
        let bonus = self.skill_check(skill);
        let result = self.roll_d20(rng, bonus, advantage);
        RollOutcome {
            result,
            bonus,
            advantage,
            auto_failed: false,
        }
    }

    /// Saving throw: condition auto-fails (Stunned et al.) and condition
    /// disadvantage (Restrained: Dex) are resolved here. Per the SRD an
    /// auto-fail needs no roll; the die is rolled anyway for transparency
    /// and `auto_failed` marks the outcome.
    #[must_use]
    pub fn roll_save(
        &self,
        rng: &mut impl rand::Rng,
        score: Score,
        extra: crate::sheet::roll::Advantage,
    ) -> RollOutcome {
        let m = self.modifiers();
        let advantage = m.save_advantage(score).combine(extra);
        let bonus = self.save_total(&score);
        let auto_failed = m.auto_fails_save(score);
        let result = self.roll_d20(rng, bonus, advantage);
        RollOutcome {
            result,
            bonus,
            advantage,
            auto_failed,
        }
    }

    /// Attack roll with the granted attack bonus. Crit detection:
    /// `result.is_critical_hit()` (natural 20), or any hit counts as a
    /// crit when the defender `auto_critical_against()` and the attacker
    /// is within 5 feet — the caller owns that context.
    #[must_use]
    pub fn roll_attack(
        &self,
        rng: &mut impl rand::Rng,
        extra: crate::sheet::roll::Advantage,
    ) -> RollOutcome {
        let m = self.modifiers();
        let advantage = m.attack_advantage().combine(extra);
        let bonus = self.attack_bonus() as i64;
        let result = self.roll_d20(rng, bonus, advantage);
        RollOutcome {
            result,
            bonus,
            advantage,
            auto_failed: false,
        }
    }

    /// True while melee attacks that hit this sheet within 5 feet are
    /// automatic critical hits (Paralyzed, Unconscious — SRD).
    #[must_use]
    pub fn auto_critical_against(&self) -> bool {
        self.modifiers().melee_auto_critical
    }

    // ---- spell slots (SRD-derived, overridable) ----

    /// Effective spellcaster level (PHB 2014 multiclass spellcasting):
    /// full levels + ceil(half/2) + ceil(third/3). Pact Magic excluded.
    #[must_use]
    pub fn caster_level(&self) -> i64 {
        use crate::spell_slots::CasterProgression::*;
        let mut level = 0i64;
        for c in &self.classes.classes {
            match c.class.caster_progression() {
                Full => level += c.level.0,
                Half => level += (c.level.0 + 1) / 2,
                Third => level += (c.level.0 + 2) / 3,
                None | Pact => {}
            }
        }
        level
    }

    /// Spell slots (spell level -> count): the override if set, else the
    /// SRD table for `caster_level`.
    #[must_use]
    pub fn spell_slots(&self) -> BTreeMap<i64, i64> {
        if let Some(v) = &self.spell_slots_override {
            let mut out = BTreeMap::new();
            for (i, &count) in v.iter().enumerate() {
                if count > 0 {
                    out.insert(i as i64 + 1, count);
                }
            }
            return out;
        }
        crate::spell_slots::full_caster_slots(self.caster_level())
    }

    /// Pact Magic (Warlock): (slot count, shared slot level).
    #[must_use]
    pub fn pact_slots(&self) -> Option<(i64, i64)> {
        let warlock_levels: i64 = self
            .classes
            .classes
            .iter()
            .filter(|c| c.class.caster_progression() == crate::spell_slots::CasterProgression::Pact)
            .map(|c| c.level.0)
            .sum();
        crate::spell_slots::pact_magic(warlock_levels)
    }

    /// Materialize spell slots as meters: `spell_slot_N` (long-rest
    /// restore) and `pact_slot_N` (short-rest restore, SRD Pact Magic).
    /// Manual meter edits persist as house rules until the next sync
    /// (same pattern as `skills.proficiency_bonus`).
    pub fn sync_spell_slots(&mut self) {
        let slots = self.spell_slots();
        for (level, count) in &slots {
            let name = format!("spell_slot_{level}");
            let m = self.meters.meters.entry(name.clone()).or_insert_with(|| {
                Meter::new(
                    name,
                    0,
                    RestoreTime {
                        short_rest: false,
                        long_rest: true,
                    },
                )
            });
            m.slot_number = *count as u32;
            m.spent = m.spent.min(m.slot_number);
            m.restore.long_rest = true;
        }
        self.meters.meters.retain(|name, _| {
            if let Some(lvl) = name.strip_prefix("spell_slot_") {
                slots.contains_key(&lvl.parse().unwrap_or(0))
            } else {
                true
            }
        });
        match self.pact_slots() {
            Some((count, level)) => {
                let name = format!("pact_slot_{level}");
                let m = self.meters.meters.entry(name.clone()).or_insert_with(|| {
                    Meter::new(
                        name,
                        0,
                        RestoreTime {
                            short_rest: true,
                            long_rest: true,
                        },
                    )
                });
                m.slot_number = count as u32;
                m.spent = m.spent.min(m.slot_number);
                m.restore.short_rest = true;
            }
            None => {
                self.meters
                    .meters
                    .retain(|name, _| !name.starts_with("pact_slot_"));
            }
        }
    }

    /// Spend one slot of the given spell level (classic or Pact).
    pub fn spend_spell_slot(&mut self, level: i64) -> bool {
        for name in [format!("spell_slot_{level}"), format!("pact_slot_{level}")] {
            if let Some(m) = self.meters.meters.get_mut(&name)
                && m.check()
            {
                m.spend();
                return true;
            }
        }
        false
    }

    // ---- death saving throws (Wiki/07 combat.md:440) ----

    /// Roll a death saving throw: d20 + `death_saves.bonus` only (SRD:
    /// not tied to an ability score). 10+ succeeds; natural 1 is two
    /// failures; natural 20 revives at 1 HP; three failures die; three
    /// successes stabilize.
    pub fn roll_death_save(
        &mut self,
        rng: &mut impl rand::Rng,
    ) -> (
        crate::sheet::roll::RollResult,
        crate::death::DeathSaveOutcome,
    ) {
        use crate::death::DeathSaveOutcome::*;
        if self.health.current > 0 {
            self.death_saves = Default::default();
        }
        let result = crate::sheet::roll::Roll::new(
            &[crate::sheet::roll::Die::D20],
            self.death_saves.bonus as i32,
        )
        .roll(rng, None);
        let nat = result.natural.unwrap_or(0);

        let outcome = if nat == 20 {
            self.health.current = 1;
            self.death_saves = Default::default();
            Revived
        } else {
            if nat == 1 {
                self.death_saves.failures += 2;
            } else if result.total >= 10 {
                self.death_saves.successes += 1;
            } else {
                self.death_saves.failures += 1;
            }
            if self.death_saves.failures >= 3 {
                Dead
            } else if self.death_saves.successes >= 3 {
                self.death_saves = Default::default();
                self.death_saves.stable = true;
                Stable
            } else if nat == 1 || result.total < 10 {
                Failure
            } else {
                Success
            }
        };
        (result, outcome)
    }

    /// SRD instant death (Wiki/07 combat.md:430): when damage reduces you
    /// to 0 hit points and the REMAINING damage equals or exceeds your hit
    /// point maximum, you die outright. Call with the damage dealt while
    /// still above 0 HP; on a massive hit this zeroes health and sets three
    /// failures (`is_dead()` becomes true). Returns whether it killed.
    pub fn check_instant_death(&mut self, dealt: i64) -> bool {
        if self.health.current > 0 && dealt - self.health.current >= self.max_health_total() {
            self.health.current = 0;
            self.death_saves.failures = 3;
            return true;
        }
        false
    }

    /// Damage taken at 0 HP: one death save failure (two on a crit), or
    /// instant death if the damage meets or exceeds max HP. Also breaks
    /// stability.
    pub fn damage_while_down(&mut self, damage: i64, critical: bool) {
        if self.health.current > 0 {
            return;
        }
        self.death_saves.stable = false;
        if damage >= self.max_health_total() {
            self.death_saves.failures = 3;
            return;
        }
        self.death_saves.failures += if critical { 2 } else { 1 };
    }

    #[must_use]
    pub fn is_dead(&self) -> bool {
        self.exhaustion >= 6 || self.death_saves.failures >= 3
    }

    pub fn reset_death_saves(&mut self) {
        self.death_saves = Default::default();
    }

    // ---- exhaustion (Wiki/12 conditions.md) ----

    /// Set exhaustion directly (house-rules seam); clamped to 6.
    pub fn set_exhaustion(&mut self, level: u8) {
        self.exhaustion = level.min(6);
    }

    /// Gain `n` levels of exhaustion; level 6 is death.
    pub fn add_exhaustion(&mut self, n: u8) {
        self.exhaustion = self.exhaustion.saturating_add(n).min(6);
    }

    /// How this character takes `kind` damage from a source that is
    /// `magical` or not.
    #[must_use]
    pub fn resistance_state(
        &self,
        kind: crate::damage::DamageType,
        magical: bool,
    ) -> crate::damage::ResistanceState {
        self.modifiers().resistance_state(kind, magical)
    }

    /// Reduce/double incoming damage per resistances (PHB 2014: half
    /// rounded down, double if vulnerable, zero if immune).
    #[must_use]
    pub fn apply_resistance(
        &self,
        kind: crate::damage::DamageType,
        magical: bool,
        amount: i64,
    ) -> i64 {
        self.resistance_state(kind, magical).apply(amount)
    }

    /// Whether the sheet can cover a spell's consumed material cost.
    /// Free components (cost 0) and non-consumed components always pass
    /// (the component-pouch abstraction: pay nothing, track nothing).
    #[must_use]
    pub fn can_afford_material(&self, spell: &spells::Spell) -> bool {
        match &spell.material {
            Some(m) if m.consumed && m.cost > 0 => self.gold >= m.cost,
            _ => true,
        }
    }

    /// Deduct a consumed material component's cost from gold. Returns
    /// false and pays nothing if the cost can't be covered.
    pub fn pay_material_cost(&mut self, spell: &spells::Spell) -> bool {
        match &spell.material {
            Some(m) if m.consumed && m.cost > 0 => {
                if self.gold < m.cost {
                    return false;
                }
                self.gold -= m.cost;
                true
            }
            _ => true,
        }
    }

    /// Max hit points including exhaustion's level-4 halving.
    #[must_use]
    pub fn max_health_total(&self) -> i64 {
        let m = self.modifiers();
        if m.max_hp_halved {
            self.health.max / 2
        } else {
            self.health.max
        }
    }

    /// Refresh `skills.proficiency_bonus` from the SRD table for the
    /// character's total level. Call after any level change; a manually
    /// set value is treated as a house-rules override until the next sync.
    pub fn sync_proficiency_bonus(&mut self) {
        self.skills.proficiency_bonus = self.classes.proficiency_bonus();
        // PB-scaled grant meters ("uses = your proficiency bonus") follow.
        self.sync_grant_meters();
    }

    /// Materialize `Effect::Meter` grants (features/feats/traits/items) into
    /// the live `meters` map — the wiring that makes a feat or subclass
    /// feature's resource pool spendable and rest-restorable. Spent state
    /// persists across syncs; slot counts follow the grant, and
    /// `pb_scaled` meters track proficiency bonus. Meters left behind by a
    /// removed grant linger (they may carry manual edits); delete them
    /// directly from `meters` if unwanted.
    pub fn sync_grant_meters(&mut self) {
        for grant in self.modifiers().meters {
            let entry = self
                .meters
                .meters
                .entry(grant.name.clone())
                .or_insert_with(|| grant.clone());
            entry.restore = grant.restore;
            entry.pb_scaled = grant.pb_scaled;
            if grant.pb_scaled {
                entry.slot_number = self.skills.proficiency_bonus.max(0) as u32;
            } else {
                entry.slot_number = grant.slot_number;
            }
            entry.spent = entry.spent.min(entry.slot_number);
        }
    }

    /// Spend one hit die from the given class to heal (SRD short-rest
    /// healing; the player chooses each die). `roll` = roll the die,
    /// otherwise take the average. Returns the amount healed.
    pub fn spend_hit_die(&mut self, class_index: usize, roll: bool) -> Option<i64> {
        let before = self.health.current;
        self.classes
            .heal(class_index, roll, &mut self.health, &self.ability_scores);
        let healed = self.health.current - before;
        if self.health.current > 0 {
            self.death_saves = Default::default();
        }
        (healed > 0).then_some(healed)
    }

    /// SRD short rest: meters that restore on a short rest refill.
    /// Hit dice spending is a separate player choice (`spend_hit_die`).
    /// The casting mechanic of the character's (first) casting class, or
    /// `None` for a non-caster. SRD "Known and Prepared Spells"
    /// (`Wiki/08 spellcasting.md:23`).
    #[must_use]
    pub fn casting_mechanic(&self) -> Option<crate::sheet::class::CastingMechanic> {
        self.classes
            .classes
            .iter()
            .find_map(|c| c.class.casting_mechanic())
    }

    /// Flip a known spell's prepared flag. Returns the new value, or `None`
    /// when the sheet doesn't have that spell.
    pub fn toggle_prepared(&mut self, spell_name: &str) -> Option<bool> {
        let spell = self.spells.spells.get_mut(spell_name)?;
        spell.prepared = !spell.prepared;
        Some(spell.prepared)
    }

    /// Begin concentrating on a spell. SRD: "You can't concentrate on
    /// two spells at once" — the previous concentration breaks, and its
    /// name is returned so the UI can say so (`save_dc` is the caster's
    /// spell save DC, kept for display).
    pub fn start_concentration(
        &mut self,
        spell_name: impl Into<String>,
        save_dc: i64,
    ) -> Option<String> {
        let previous = self.concentrating.replace(Concentration {
            spell_name: spell_name.into(),
            save_dc,
        });
        previous.map(|c| c.spell_name)
    }

    /// End concentration voluntarily (SRD: "You can end concentration at
    /// any time (no action required)").
    pub fn end_concentration(&mut self) -> Option<Concentration> {
        self.concentrating.take()
    }

    /// Taking damage while concentrating forces a CON save: DC is 10 or
    /// half the damage, whichever is higher (SRD `Wiki/08
    /// spellcasting.md:128`). Returns `None` when nothing is being
    /// concentrated on; on a failed save the concentration is cleared
    /// and `broken` reports it.
    pub fn concentration_save(
        &mut self,
        rng: &mut impl rand::Rng,
        damage: i64,
    ) -> Option<ConcentrationSave> {
        let dc = 10.max(damage / 2);
        let outcome = self.roll_save(rng, Score::Con, crate::sheet::roll::Advantage::Normal);
        let broken = outcome.result.total < dc;
        let spell_name = self.concentrating.as_ref()?.spell_name.clone();
        if broken {
            self.concentrating = None;
        }
        Some(ConcentrationSave {
            spell_name,
            dc,
            outcome,
            broken,
        })
    }

    /// The "prepare spells" prompt is answered: the day's list is set.
    pub fn finish_spell_prep(&mut self) {
        self.needs_spell_prep = false;
    }

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
        // SRD: finishing a long rest reduces exhaustion by 1
        // (assuming food and drink; set `exhaustion` directly otherwise).
        self.exhaustion = self.exhaustion.saturating_sub(1);
        self.death_saves = Default::default();
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

        // SRD: finishing a long rest restores spell slots (:35); prepared
        // casters must also choose a fresh list — flag it for the UI.
        if matches!(
            self.casting_mechanic(),
            Some(crate::sheet::class::CastingMechanic::Prepared)
                | Some(crate::sheet::class::CastingMechanic::PreparedWithSpellbook)
        ) {
            self.needs_spell_prep = true;
        }
    }

    pub fn passive_senses(&self) -> PassiveSenses {
        PassiveSenses {
            insight: 10 + self.skill_check(&Skill::Insight) + self.senses.insight_bonus,
            perception: 10 + self.skill_check(&Skill::Perception) + self.senses.perception_bonus,
            investigation: 10
                + self.skill_check(&Skill::Investigation)
                + self.senses.investigation_bonus,
        }
    }
}

impl Default for Character {
    fn default() -> Self {
        Self {
            name: "name".to_string(),
            race: Some(Race::new("Elf")),
            needs_spell_prep: false,
            concentrating: None,
            background: None,
            gold: 0,
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
            feats: Vec::new(),
            traits: Vec::new(),
            death_saves: Default::default(),
            exhaustion: 0,
            spell_slots_override: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassiveSenses {
    pub insight: i64,
    pub perception: i64,
    pub investigation: i64,
}

/// A spell being concentrated on: the name plus the caster's spell save
/// DC at cast time, for display.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
pub struct Concentration {
    pub spell_name: String,
    pub save_dc: i64,
}

/// The result of the CON save forced by damage while concentrating.
#[derive(Debug, Clone, PartialEq)]
pub struct ConcentrationSave {
    pub spell_name: String,
    /// The DC that was set: `max(10, damage / 2)`.
    pub dc: i64,
    pub outcome: RollOutcome,
    /// The save failed: concentration ended.
    pub broken: bool,
}

/// A d20 roll resolved against the sheet: the dice, the modifier that was
/// added, the resolved advantage state, and whether a condition forced a
/// failure regardless of the die.
#[derive(Debug, Clone, PartialEq)]
pub struct RollOutcome {
    pub result: crate::sheet::roll::RollResult,
    /// Total modifier added to the d20 (from sheet + grants).
    pub bonus: i64,
    /// Resolved advantage (conditions combined with situational input).
    pub advantage: crate::sheet::roll::Advantage,
    /// A condition auto-failed this roll (SRD: no roll needed; the die is
    /// rolled anyway for transparency).
    pub auto_failed: bool,
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

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// Armor class. Normally `flat` is `None` and AC is *derived*: best armor
/// formula among grants (default unarmored 10 + Dex) + Dex + bonuses.
/// Setting `flat` overrides the formula outright (monster statblocks,
/// house rules); flat bonuses (shields, `AbilityAcBonus`) still apply.
pub struct ArmorClass {
    #[cfg_attr(feature = "rhai", rhai_type(set = Self::set, get = Self::get))]
    pub flat: Option<i64>,
}

impl ArmorClass {
    fn new(_scores: &AbilityScores) -> Self {
        Self { flat: None }
    }

    /// The flat override, or the unarmored default if derived.
    pub fn get(&self) -> i64 {
        self.flat.unwrap_or(10)
    }

    pub fn set(&mut self, s: i64) {
        self.flat = Some(s);
    }
}

impl Display for ArmorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.flat {
            Some(v) => write!(f, "{v}"),
            None => write!(f, "derived"),
        }
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
    use crate::feature::{AbilityScoreBonus, Effect, Feature};
    use crate::meter::{Meter, RestoreTime};
    use crate::senses::Sense;
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
    fn casting_mechanic_per_class_srd() {
        // SRD "Known and Prepared Spells" (Wiki/08 spellcasting.md:23-25).
        use crate::sheet::class::CastingMechanic::*;
        assert_eq!(ClassType::Cleric.casting_mechanic(), Some(Prepared));
        assert_eq!(ClassType::Paladin.casting_mechanic(), Some(Prepared));
        assert_eq!(ClassType::Ranger.casting_mechanic(), Some(Prepared));
        assert_eq!(ClassType::Druid.casting_mechanic(), Some(Prepared));
        assert_eq!(
            ClassType::Wizard.casting_mechanic(),
            Some(PreparedWithSpellbook)
        );
        assert_eq!(ClassType::Bard.casting_mechanic(), Some(Known));
        assert_eq!(ClassType::Sorcerer.casting_mechanic(), Some(Known));
        assert_eq!(ClassType::Warlock.casting_mechanic(), Some(Known));
        assert_eq!(ClassType::Fighter.casting_mechanic(), None);
        // Multiclass: the first casting class decides the UI mechanic.
        let mut sheet = Character::default();
        sheet.classes.classes[0].class = ClassType::Fighter;
        sheet.classes.classes.push(Class {
            class: ClassType::Cleric,
            ..Default::default()
        });
        assert_eq!(
            sheet.casting_mechanic(),
            Some(crate::sheet::class::CastingMechanic::Prepared)
        );
    }

    #[test]
    fn long_rest_flags_prepared_casters_for_prep() {
        // SRD: a long rest restores slots (:35) and prepared casters pick a
        // fresh list; the flag drives the UI prompt, nothing else.
        let mut cleric = Character::new(
            "C".to_string(),
            1,
            HitPoints::new(8),
            ClassType::Cleric,
            AbilityScores::default(),
        );
        assert!(!cleric.needs_spell_prep);
        cleric.long_rest();
        assert!(cleric.needs_spell_prep);
        cleric.finish_spell_prep();
        assert!(!cleric.needs_spell_prep);

        let mut fighter = Character::new(
            "F".to_string(),
            1,
            HitPoints::new(10),
            ClassType::Fighter,
            AbilityScores::default(),
        );
        fighter.long_rest();
        assert!(!fighter.needs_spell_prep, "non-casters get no prompt");
    }

    #[test]
    fn background_grants_skill_proficiencies() {
        use crate::background::Background;
        use crate::skills::Skill;
        let mut sheet = Character::default();
        let before = sheet.skill_check(&Skill::Insight);
        sheet.background = Some(Background {
            name: "Acolyte".into(),
            skill_proficiencies: vec![Skill::Insight, Skill::Religion],
            gold: 15,
            ..Default::default()
        });
        let after = sheet.skill_check(&Skill::Insight);
        assert_eq!(
            after,
            before + sheet.skills.proficiency_bonus as i64,
            "proficient"
        );
        // Lossless removal.
        sheet.background = None;
        assert_eq!(sheet.skill_check(&Skill::Insight), before);
    }

    #[test]
    fn concentration_rules_srd_2014() {
        // Wiki/08 spellcasting.md:120-129.
        use rand::SeedableRng;
        let mut sheet = Character::default();
        sheet.ability_scores.con.base = 20;

        // One concentration at a time: the old one breaks and is reported.
        assert_eq!(sheet.start_concentration("Haste", 13), None);
        assert_eq!(
            sheet.start_concentration("Fly", 13),
            Some("Haste".to_string())
        );
        assert_eq!(sheet.concentrating.as_ref().unwrap().spell_name, "Fly");

        // DC is 10 or half the damage, whichever is higher.
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let light = sheet.concentration_save(&mut rng, 5).unwrap();
        assert_eq!(light.dc, 10, "half of 5 is below the floor");
        let heavy = sheet.concentration_save(&mut rng, 30).unwrap();
        assert_eq!(heavy.dc, 15, "half of 30 beats the floor");

        // State agrees with the roll either way.
        assert_eq!(heavy.broken, heavy.outcome.result.total < 15);
        assert_eq!(
            sheet.concentrating.is_none(),
            heavy.broken,
            "a failed save ends it, a saved one keeps it",
        );

        // Voluntary end, and no save when nothing is up.
        assert!(sheet.end_concentration().is_some() || heavy.broken);
        assert!(sheet.concentration_save(&mut rng, 10).is_none());
    }

    #[test]
    fn prepared_flag_toggles() {
        let mut sheet = Character::default();
        let spell = crate::spells::Spell {
            name: "Shield of Faith".to_string(),
            ..Default::default()
        };
        sheet.spells.spells.insert(spell.name.clone(), spell);
        assert_eq!(sheet.toggle_prepared("Shield of Faith"), Some(true));
        assert!(sheet.spells.spells["Shield of Faith"].prepared);
        assert_eq!(sheet.toggle_prepared("Shield of Faith"), Some(false));
        assert_eq!(sheet.toggle_prepared("Not a Spell"), None);
    }

    /// A save written before `prepared` existed still loads (serde default).
    #[cfg(feature = "serde")]
    #[test]
    fn prepared_flag_survives_old_saves() {
        let mut sheet = Character::default();
        let spell = crate::spells::Spell {
            name: "Shield of Faith".to_string(),
            ..Default::default()
        };
        sheet.spells.spells.insert(spell.name.clone(), spell);
        sheet.toggle_prepared("Shield of Faith");
        let json = serde_json::to_string(&sheet).unwrap();
        let stripped = json.replace("\"prepared\":true,", "");
        assert_ne!(json, stripped, "the field was in the save to strip");
        let back: Character = serde_json::from_str(&stripped).unwrap();
        assert!(!back.spells.spells["Shield of Faith"].prepared);
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

    // ---- grant system: lossless apply/remove ----

    fn granted_feat(name: &str, effects: Vec<crate::feature::Effect>) -> Feat {
        Feat {
            name: name.to_string(),
            desc: String::new(),
            feature: Some(crate::feature::Feature {
                name: name.to_string(),
                source: None,
                effects,
            }),
            source: None,
        }
    }

    #[test]
    fn test_grant_apply_remove_is_lossless() {
        use crate::feature::Effect;
        let mut sheet = Character::default();
        let snapshot = sheet.clone();

        sheet.add_feat(granted_feat(
            "Alertness",
            vec![
                Effect::InitiativeBonus(2),
                Effect::AcBonus(1),
                Effect::AddProficiency(skills::Skill::Perception),
                Effect::SkillBonus {
                    skill: skills::Skill::Athletics,
                    bonus: 1,
                },
                Effect::AbilityScoreBonus(crate::feature::AbilityScoreBonus {
                    score: Score::Wis,
                    bonus: 2,
                }),
            ],
        ));

        // All default scores 10 (+0), PB +2.
        assert_eq!(sheet.initiative(), 2);
        // PB +2 from granted proficiency, +1 from the granted Wis bonus.
        assert_eq!(sheet.skill_check(&skills::Skill::Perception), 3);
        assert_eq!(sheet.skill_check(&skills::Skill::Athletics), 1);
        assert_eq!(sheet.passive_senses().perception, 13); // 10 + 3
        assert_eq!(sheet.ability_modifier_for(Score::Wis), 1); // Wis 12
        assert_eq!(sheet.modifiers().meters.len(), 0);

        assert!(sheet.has_grant("alertness")); // case-insensitive
        assert!(sheet.remove_grant("alertness"));

        // The sheet is byte-identical to before: no residue, no side effects.
        assert_eq!(sheet, snapshot);
        assert_eq!(sheet.initiative(), 0);
        assert_eq!(sheet.skill_check(&skills::Skill::Perception), 0);
        assert_eq!(sheet.passive_senses().perception, 10);
    }

    #[test]
    fn test_granted_proficiency_is_a_union() {
        use crate::feature::{Effect, Feature};
        let mut sheet = Character::default();
        sheet.add_feature(Feature {
            name: "Feature A".to_string(),
            source: None,
            effects: vec![Effect::AddProficiency(skills::Skill::Perception)],
        });
        sheet.add_feature(Feature {
            name: "Feature B".to_string(),
            source: None,
            effects: vec![Effect::AddProficiency(skills::Skill::Perception)],
        });
        // Two sources, one proficiency: counted once (PB +2, not +4).
        assert_eq!(sheet.skill_check(&skills::Skill::Perception), 2);
        // Removing one still leaves the other.
        assert!(sheet.remove_grant("Feature A"));
        assert_eq!(sheet.skill_check(&skills::Skill::Perception), 2);
        assert!(sheet.remove_grant("Feature B"));
        assert_eq!(sheet.skill_check(&skills::Skill::Perception), 0);
    }

    #[test]
    fn test_granted_expertise_doubles() {
        use crate::feature::{Effect, Feature};
        let mut sheet = Character::default();
        sheet.add_feature(Feature {
            name: "Expert".to_string(),
            source: None,
            effects: vec![
                Effect::AddProficiency(skills::Skill::Perception),
                Effect::Expertise(skills::Skill::Perception),
            ],
        });
        assert_eq!(sheet.skill_check(&skills::Skill::Perception), 4);
        assert!(sheet.remove_grant("Expert"));
        assert_eq!(sheet.skill_check(&skills::Skill::Perception), 0);
    }

    #[test]
    fn test_condition_effects_and_removal() {
        let mut sheet = Character::default();
        let snapshot = sheet.clone();

        assert!(sheet.add_condition(Condition::new("Stunned", 2)));
        assert!(sheet.auto_fails_save(Score::Dex));
        assert!(sheet.auto_fails_save(Score::Str));
        assert!(!sheet.auto_fails_save(Score::Con));
        assert!(!sheet.can_take_actions());
        // SRD: Stunned gives advantage vs attacks, but melee auto-crits
        // apply only to Paralyzed/Unconscious.
        assert!(!sheet.modifiers().melee_auto_critical);
        assert!(sheet.modifiers().attacks_against_advantage);

        assert!(sheet.remove_grant("stunned")); // case-insensitive
        assert!(!sheet.auto_fails_save(Score::Dex));
        assert!(sheet.can_take_actions());
        assert_eq!(sheet, snapshot);
    }

    #[test]
    fn test_condition_immunity_blocks_condition() {
        use crate::feature::Effect;
        let mut sheet = Character::default();
        sheet.add_feat(granted_feat(
            "Brave Heart",
            vec![Effect::ConditionImmunity(
                crate::condition::ConditionKind::Frightened,
            )],
        ));
        assert!(!sheet.add_condition(Condition::new("Frightened", 3)));
        assert!(sheet.add_condition(Condition::new("Poisoned", 3)));
        assert_eq!(sheet.conditions.conditions.len(), 1);
    }

    #[test]
    fn test_item_equip_unequip_is_lossless() {
        use crate::feature::Effect;
        let mut sheet = Character::default();
        let base_ac = sheet.armor_class_total();

        let mut cloak = Item::new("Cloak of Protection".to_string());
        cloak.add(Effect::AcBonus(1));
        sheet
            .inventory
            .0
            .insert("Cloak of Protection".to_string(), cloak);

        // In the backpack: no effect.
        assert_eq!(sheet.armor_class_total(), base_ac);
        assert!(sheet.equip("Cloak of Protection"));
        assert_eq!(sheet.armor_class_total(), base_ac + 1);
        // "Removing" the grant unequips rather than deleting.
        assert!(sheet.remove_grant("cloak of protection"));
        assert_eq!(sheet.armor_class_total(), base_ac);
        assert!(sheet.inventory.0.contains_key("Cloak of Protection"));
    }

    #[test]
    fn test_tick_conditions_removes_expired() {
        let mut sheet = Character::default();
        sheet.add_condition(Condition::new("Stunned", 1));
        sheet.add_condition(Condition::new("Poisoned", -1)); // permanent
        sheet.tick_conditions();
        assert_eq!(sheet.conditions.conditions.len(), 1);
        assert_eq!(sheet.conditions.conditions[0].kind.to_string(), "Poisoned");
        sheet.tick_conditions();
        assert_eq!(sheet.conditions.conditions.len(), 1);
    }

    #[test]
    fn test_speed_and_save_bonuses() {
        use crate::feature::Effect;
        let mut sheet = Character::default();
        let base_speed = sheet.speed_total();
        let base_save = sheet.save_total(&Score::Str);

        sheet.add_feat(granted_feat(
            "Swift",
            vec![
                Effect::SpeedBonus(10),
                Effect::SaveBonus {
                    score: Score::Str,
                    bonus: 2,
                },
            ],
        ));
        assert_eq!(sheet.speed_total(), base_speed + 10);
        assert_eq!(sheet.save_total(&Score::Str), base_save + 2);
        assert!(sheet.remove_grant("Swift"));
        assert_eq!(sheet.speed_total(), base_speed);
        assert_eq!(sheet.save_total(&Score::Str), base_save);
    }

    // ---- rolls consume the grant/condition system ----

    #[test]
    fn test_roll_save_auto_fail_and_disadvantage() {
        use crate::sheet::roll::Advantage;
        let mut sheet = Character::default();
        let mut rng = rand::rng();

        // Stunned: Str and Dex saves auto-fail (SRD).
        sheet.add_condition(Condition::new("Stunned", 2));
        let out = sheet.roll_save(&mut rng, Score::Dex, Advantage::Normal);
        assert!(out.auto_failed);
        // Stunned auto-fails but adds no disadvantage die.
        assert!(out.result.failures.is_empty());

        // Restrained: disadvantage on Dex saves only.
        sheet.remove_grant("stunned");
        sheet.add_condition(Condition::new("Restrained", 2));
        let out = sheet.roll_save(&mut rng, Score::Dex, Advantage::Normal);
        assert_eq!(out.advantage, Advantage::Disadvantage);
        assert!(!out.auto_failed);
        assert_eq!(out.result.failures.len(), 1);
        let out = sheet.roll_save(&mut rng, Score::Wis, Advantage::Normal);
        assert_eq!(out.advantage, Advantage::Normal);
    }

    #[test]
    fn test_roll_attack_advantage_cancels() {
        use crate::sheet::roll::Advantage;
        let mut sheet = Character::default();
        let mut rng = rand::rng();

        // Poisoned: disadvantage on the sheet's attack rolls.
        sheet.add_condition(Condition::new("Poisoned", 2));
        let out = sheet.roll_attack(&mut rng, Advantage::Normal);
        assert_eq!(out.advantage, Advantage::Disadvantage);
        assert_eq!(out.result.failures.len(), 1);

        // Invisible grants own-attack advantage: cancels Poisoned.
        sheet.add_condition(Condition::new("Invisible", 2));
        let out = sheet.roll_attack(&mut rng, Advantage::Normal);
        assert_eq!(out.advantage, Advantage::Normal);
        assert!(out.result.failures.is_empty());

        // Remove the disadvantage: advantage shows through.
        sheet.remove_grant("poisoned");
        let out = sheet.roll_attack(&mut rng, Advantage::Normal);
        assert_eq!(out.advantage, Advantage::Advantage);
    }

    #[test]
    fn test_roll_check_uses_granted_bonuses() {
        use crate::feature::{Effect, Feature};
        use crate::sheet::roll::Advantage;
        let mut sheet = Character::default();
        sheet.add_feature(Feature {
            name: "Expert".to_string(),
            source: None,
            effects: vec![
                Effect::AddProficiency(skills::Skill::Perception),
                Effect::Expertise(skills::Skill::Perception),
            ],
        });
        let mut rng = rand::rng();
        let out = sheet.roll_check(&mut rng, &skills::Skill::Perception, Advantage::Normal);
        assert_eq!(out.bonus, 4); // PB +2 doubled by expertise
        let nat = out.result.natural.expect("d20 natural exposed");
        assert_eq!(out.result.total, nat + 4);
    }

    #[test]
    fn test_auto_critical_against() {
        let mut sheet = Character::default();
        assert!(!sheet.auto_critical_against());
        sheet.add_condition(Condition::new("Unconscious", -1));
        assert!(sheet.auto_critical_against());
        sheet.remove_grant("unconscious");
        assert!(!sheet.auto_critical_against());
    }

    // ---- spell slots ----

    fn leveled(class: ClassType, level: i64) -> Character {
        Character::new(
            "A".to_string(),
            level,
            HitPoints::new(30),
            class,
            AbilityScores::default(),
        )
    }

    #[test]
    fn grant_meters_materialize_and_scale_with_pb() {
        use crate::sheet::meter::RestoreTime;
        let mut c = Character::new(
            "Test".into(),
            5,
            HitPoints {
                current: 34,
                max: 34,
                level_bonus: 0,
                bonus: 0,
            },
            ClassType::Sorcerer,
            AbilityScores::default(),
        );
        c.sync_proficiency_bonus();
        let mut meter = Meter::new("restore_balance".into(), 0, RestoreTime::default());
        meter.pb_scaled = true;
        c.add_feature(crate::feature::Feature {
            name: "Restore Balance".into(),
            source: None,
            effects: vec![crate::feature::Effect::Meter(meter)],
        });
        // PB-scaled: 3 uses at level 5, and the long-rest restore flag rides.
        let m = c.meters.meters.get("restore_balance").unwrap();
        assert_eq!(m.slot_number, 3);
        assert!(m.restore.long_rest);
        // Spend all three, long rest refills them.
        for _ in 0..3 {
            assert!(c.meters.meters["restore_balance"].check());
            c.meters.meters.get_mut("restore_balance").unwrap().spend();
        }
        assert!(!c.meters.meters["restore_balance"].check());
        c.long_rest();
        assert_eq!(c.meters.meters.get("restore_balance").unwrap().spent, 0);
        // Level 9: PB rises to +4, the meter follows, spent state survives.
        c.classes.classes[0].level = Level(9);
        c.sync_proficiency_bonus();
        assert_eq!(
            c.meters.meters.get("restore_balance").unwrap().slot_number,
            4
        );
    }
    #[test]
    fn test_spell_slots_full_caster() {
        let sheet = leveled(ClassType::Cleric, 5);
        assert_eq!(sheet.caster_level(), 5);
        let slots = sheet.spell_slots();
        assert_eq!(slots.get(&1), Some(&4));
        assert_eq!(slots.get(&2), Some(&3));
        assert_eq!(slots.get(&3), Some(&2));
        assert_eq!(slots.get(&4), None);
    }

    #[test]
    fn test_spell_slots_half_and_multiclass() {
        // Paladin 5: half caster -> effective level 3.
        let sheet = leveled(ClassType::Paladin, 5);
        assert_eq!(sheet.caster_level(), 3);
        assert_eq!(sheet.spell_slots().get(&2), Some(&2));
        assert_eq!(sheet.spell_slots().get(&3), None);

        // Wizard 3 + Paladin 3: 3 + ceil(3/2) = 5.
        let mut sheet = leveled(ClassType::Wizard, 3);
        sheet.classes.classes.push(Class {
            level: Level(3),
            hit_dice: ClassType::Paladin.get_hit_dice(),
            health_bonus_per_level: 0,
            class: ClassType::Paladin,
            healing_die_remaining: 3,
            subclass: String::new(),
            max_healing_die: 3,
        });
        assert_eq!(sheet.caster_level(), 5);
        assert_eq!(sheet.spell_slots().get(&3), Some(&2));
    }

    #[test]
    fn test_spell_slots_sync_spend_rest() {
        let mut sheet = leveled(ClassType::Cleric, 5);
        sheet.sync_spell_slots();
        assert_eq!(sheet.meters.meters["spell_slot_1"].slot_number, 4);
        assert!(sheet.spend_spell_slot(1));
        assert_eq!(sheet.meters.meters["spell_slot_1"].spent, 1);
        assert!(sheet.spend_spell_slot(3));
        // No 9th-level slots at level 5.
        assert!(!sheet.spend_spell_slot(9));
        sheet.long_rest();
        assert_eq!(sheet.meters.meters["spell_slot_1"].spent, 0);
        assert_eq!(sheet.meters.meters["spell_slot_3"].spent, 0);
    }

    #[test]
    fn test_spell_slots_override_roundtrip() {
        let mut sheet = leveled(ClassType::Cleric, 5);
        // House rule: one 1st-level slot, nothing else.
        sheet.spell_slots_override = Some(vec![1, 0, 0, 0, 0, 0, 0, 0, 0]);
        sheet.sync_spell_slots();
        assert_eq!(sheet.meters.meters["spell_slot_1"].slot_number, 1);
        assert!(!sheet.meters.meters.contains_key("spell_slot_2"));
        // Clearing the override restores the SRD table on next sync.
        sheet.spell_slots_override = None;
        sheet.sync_spell_slots();
        assert_eq!(sheet.meters.meters["spell_slot_1"].slot_number, 4);
        assert_eq!(sheet.meters.meters["spell_slot_2"].slot_number, 3);
    }

    #[test]
    fn test_pact_magic_short_rest() {
        let mut sheet = leveled(ClassType::Warlock, 5);
        assert_eq!(sheet.pact_slots(), Some((2, 3)));
        assert_eq!(sheet.caster_level(), 0); // Pact doesn't combine
        sheet.sync_spell_slots();
        let m = &sheet.meters.meters["pact_slot_3"];
        assert_eq!(m.slot_number, 2);
        assert!(m.restore.short_rest);
        assert!(sheet.spend_spell_slot(3));
        assert!(sheet.spend_spell_slot(3));
        assert!(!sheet.spend_spell_slot(3));
        sheet.short_rest();
        assert_eq!(sheet.meters.meters["pact_slot_3"].spent, 0);
    }

    // ---- death saves ----

    #[test]
    fn test_damage_while_down() {
        let mut sheet = leveled(ClassType::Fighter, 1);
        sheet.health.current = 0;
        sheet.damage_while_down(1, false);
        assert_eq!(sheet.death_saves.failures, 1);
        sheet.damage_while_down(1, true); // crit: two failures
        assert_eq!(sheet.death_saves.failures, 3);
        assert!(sheet.is_dead());

        // SRD instant death from FULL HP: overflow >= max HP kills outright
        // (Wiki/07 combat.md:434 example: 12 max, at 6, takes 18 -> dead).
        let mut c = leveled(ClassType::Fighter, 1);
        c.health.max = 12;
        c.health.current = 6;
        assert!(c.check_instant_death(18));
        assert!(c.is_dead());
        let mut c = leveled(ClassType::Fighter, 1);
        c.health.max = 12;
        c.health.current = 6;
        assert!(
            !c.check_instant_death(17),
            "overflow 11 < max 12: down, not dead"
        );
        assert_eq!(c.health.current, 6, "non-lethal check mutates nothing");

        // Massive damage (>= max HP): instant death.
        let mut sheet2 = leveled(ClassType::Fighter, 1);
        sheet2.health.current = 0;
        sheet2.damage_while_down(30, false);
        assert!(sheet2.is_dead());
    }

    #[test]
    fn test_death_saves_terminate() {
        use crate::death::DeathSaveOutcome::*;
        use rand::SeedableRng;
        // Deterministic seeds: every run of saves must end in one of the
        // terminal outcomes, and counts must stay SRD-consistent.
        let mut saw_dead = false;
        let mut saw_stable = false;
        let mut saw_revived = false;
        for seed in 0..40u64 {
            let mut sheet = Character::default();
            sheet.health.current = 0;
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            for _ in 0..40 {
                let (r, o) = sheet.roll_death_save(&mut rng);
                match o {
                    Dead => {
                        assert!(sheet.is_dead());
                        saw_dead = true;
                        break;
                    }
                    Stable => {
                        assert!(sheet.death_saves.stable);
                        saw_stable = true;
                        break;
                    }
                    Revived => {
                        assert_eq!(r.natural, Some(20));
                        assert_eq!(sheet.health.current, 1);
                        saw_revived = true;
                        break;
                    }
                    Success | Failure => {
                        assert!(sheet.death_saves.failures < 3);
                        assert!(sheet.death_saves.successes < 3);
                    }
                }
            }
        }
        assert!(saw_dead && saw_stable && saw_revived);
    }

    // ---- exhaustion ----

    #[test]
    fn test_exhaustion_srd_ladder() {
        let mut sheet = leveled(ClassType::Fighter, 1); // max HP 30, speed 30
        assert_eq!(sheet.speed_total(), 30);

        sheet.set_exhaustion(1);
        assert!(sheet.modifiers().check_disadvantage);
        assert!(!sheet.modifiers().attack_disadvantage);

        sheet.set_exhaustion(2);
        assert_eq!(sheet.speed_total(), 15); // halved

        sheet.set_exhaustion(3);
        assert!(sheet.modifiers().attack_disadvantage);
        assert!(sheet.modifiers().all_saves_disadvantaged);
        assert_eq!(
            sheet.modifiers().save_advantage(Score::Con),
            crate::sheet::roll::Advantage::Disadvantage
        );

        sheet.set_exhaustion(4);
        assert_eq!(sheet.max_health_total(), 15); // halved

        sheet.set_exhaustion(5);
        assert_eq!(sheet.speed_total(), 0);

        sheet.set_exhaustion(6);
        assert!(sheet.is_dead()); // level 6: death

        // Long rest reduces by 1 (SRD, with food/drink assumed).
        sheet.set_exhaustion(3);
        sheet.long_rest();
        assert_eq!(sheet.exhaustion, 2);

        // add_exhaustion stacks and clamps at 6.
        sheet.set_exhaustion(0);
        sheet.add_exhaustion(2);
        sheet.add_exhaustion(10);
        assert_eq!(sheet.exhaustion, 6);
    }

    // ---- damage resistances ----

    use crate::damage::{DamageType, ResistanceState};

    fn res_feat(name: &str, effects: Vec<Effect>) -> Feat {
        Feat {
            name: name.to_string(),
            desc: String::new(),
            feature: Some(Feature {
                name: name.to_string(),
                effects,
                ..Feature::default()
            }),
            source: None,
        }
    }

    #[test]
    fn test_resistance_grant_remove_lossless() {
        let mut sheet = Character::default();
        assert_eq!(sheet.apply_resistance(DamageType::Fire, true, 11), 11);
        sheet.add_feat(res_feat(
            "Fire Resistance",
            vec![Effect::Resistance(DamageType::Fire)],
        ));
        assert_eq!(
            sheet.resistance_state(DamageType::Fire, true),
            ResistanceState::Resistant
        );
        assert_eq!(sheet.apply_resistance(DamageType::Fire, true, 11), 5); // halved, floor
        assert_eq!(sheet.apply_resistance(DamageType::Cold, true, 11), 11);
        sheet.remove_grant("fire resistance");
        assert_eq!(sheet.apply_resistance(DamageType::Fire, true, 11), 11);
    }

    #[test]
    fn test_immunity_and_vulnerability() {
        let mut sheet = Character::default();
        sheet.add_feat(res_feat(
            "Bait",
            vec![
                Effect::Immunity(DamageType::Poison),
                Effect::Vulnerability(DamageType::Radiant),
            ],
        ));
        assert_eq!(sheet.apply_resistance(DamageType::Poison, false, 20), 0);
        assert_eq!(sheet.apply_resistance(DamageType::Radiant, false, 7), 14);
    }

    #[test]
    fn test_nonmagical_weapon_resistance() {
        let mut sheet = Character::default();
        sheet.add_feat(res_feat(
            "Werewolf-ish",
            vec![Effect::NonmagicalWeaponResistance],
        ));
        assert_eq!(sheet.apply_resistance(DamageType::Slashing, false, 9), 4); // nonmagical
        assert_eq!(sheet.apply_resistance(DamageType::Slashing, true, 9), 9); // magical: full
        assert_eq!(sheet.apply_resistance(DamageType::Fire, false, 9), 9); // not weapon damage
    }

    #[test]
    fn test_petrified_resists_all_damage() {
        let mut sheet = Character::default();
        sheet.add_condition(Condition::new("Petrified", -1));
        assert_eq!(
            sheet.resistance_state(DamageType::Acid, true),
            ResistanceState::Resistant
        );
        assert_eq!(sheet.apply_resistance(DamageType::Acid, true, 8), 4);
        assert_eq!(
            sheet.resistance_state(DamageType::Poison, false),
            ResistanceState::Immune
        );
        sheet.remove_grant("petrified");
        assert_eq!(sheet.apply_resistance(DamageType::Acid, true, 8), 8);
    }

    #[test]
    fn test_resistance_and_vulnerability_cancel() {
        let mut sheet = Character::default();
        sheet.add_feat(res_feat(
            "Mixed",
            vec![
                Effect::Resistance(DamageType::Fire),
                Effect::Vulnerability(DamageType::Fire),
            ],
        ));
        assert_eq!(
            sheet.resistance_state(DamageType::Fire, true),
            ResistanceState::Normal
        );
        assert_eq!(sheet.apply_resistance(DamageType::Fire, true, 10), 10);
    }

    // ---- armor class ----

    fn armored_item(name: &str, effects: Vec<Effect>) -> Item {
        let mut item = Item::new(name.to_string());
        for e in effects {
            item.add(e);
        }
        item
    }

    #[test]
    fn test_armor_class_derivation_srd() {
        let mut sheet = Character::default(); // Dex 10 (+0)
        sheet.ability_scores.dex = AbilityScore::new(14, 0); // +2
        // Unarmored: 10 + Dex.
        assert_eq!(sheet.armor_class_total(), 12);

        // Leather: 11 + Dex (uncapped).
        sheet.inventory.0.insert(
            "Leather Armor".to_string(),
            armored_item(
                "Leather Armor",
                vec![Effect::ArmorClassBase {
                    base: 11,
                    dex_cap: None,
                }],
            ),
        );
        sheet.equip("Leather Armor");
        assert_eq!(sheet.armor_class_total(), 13);

        // Shield: +2 flat, stacks with armor.
        sheet.inventory.0.insert(
            "Shield".to_string(),
            armored_item("Shield", vec![Effect::AcBonus(2)]),
        );
        sheet.equip("Shield");
        assert_eq!(sheet.armor_class_total(), 15);

        // Chain mail: 16, no Dex (heavy). Highest base wins over leather.
        sheet.inventory.0.insert(
            "Chain Mail".to_string(),
            armored_item(
                "Chain Mail",
                vec![Effect::ArmorClassBase {
                    base: 16,
                    dex_cap: Some(0),
                }],
            ),
        );
        sheet.equip("Chain Mail");
        assert_eq!(sheet.armor_class_total(), 18); // 16 + 0 + 2

        // Unequipping chain falls back to leather's formula.
        sheet.remove_grant("chain mail");
        assert_eq!(sheet.armor_class_total(), 15);

        // Flat override (statblock/house rule): formula bypassed, bonuses
        // still apply.
        sheet.armor_class.set(15);
        assert_eq!(sheet.armor_class_total(), 17); // 15 + shield 2
        sheet.armor_class.flat = None; // back to derived
        assert_eq!(sheet.armor_class_total(), 15);
    }

    #[test]
    fn test_unarmored_defense_features() {
        // Monk-style: 10 + Dex + Wis.
        let mut sheet = Character::default();
        sheet.ability_scores.dex = AbilityScore::new(16, 0); // +3
        sheet.ability_scores.wis = AbilityScore::new(14, 0); // +2
        assert_eq!(sheet.armor_class_total(), 13); // unarmored 10 + Dex
        sheet.add_feature(Feature {
            name: "Unarmored Defense".to_string(),
            effects: vec![Effect::AbilityAcBonus(Score::Wis)],
            ..Feature::default()
        });
        assert_eq!(sheet.armor_class_total(), 15); // + Wis
        sheet.remove_grant("unarmored defense");
        assert_eq!(sheet.armor_class_total(), 13);
    }

    // ---- race ----

    #[test]
    fn test_race_grants_are_lossless() {
        let mut sheet = Character::default();
        assert_eq!(sheet.ability_modifier_for(Score::Dex), 0);
        sheet.race = Some(Race {
            name: "Elf".to_string(),
            ability_bonuses: vec![AbilityScoreBonus {
                score: Score::Dex,
                bonus: 2,
            }],
            feature: Some(Feature {
                name: "Darkvision".to_string(),
                effects: vec![Effect::Sense(Sense::DarkVision)],
                ..Feature::default()
            }),
            ..Race::default()
        });
        assert_eq!(sheet.ability_modifier_for(Score::Dex), 1); // +2 score = +1 mod
        assert!(sheet.modifiers().has_sense(&Sense::DarkVision));

        // Racial speed overrides the sheet default.
        sheet.race = Some(Race {
            name: "Dwarf".to_string(),
            speed: Some(25),
            ..Race::default()
        });
        assert_eq!(sheet.speed_total(), 25);
        assert!(!sheet.modifiers().has_sense(&Sense::DarkVision));

        // Removing the race removes everything.
        sheet.race = None;
        assert_eq!(sheet.ability_modifier_for(Score::Dex), 0);
        assert_eq!(sheet.speed_total(), 30);
    }

    // ---- material components ----

    #[test]
    fn test_material_component_costs() {
        use crate::spells::{MaterialComponent, Spell};
        let mut sheet = Character::default();
        assert_eq!(sheet.gold, 0);

        // Identify-style: 100 gp pearl, consumed.
        let identify = Spell {
            name: "identify".to_string(),
            material: Some(MaterialComponent {
                name: "a pearl worth at least 100 gp".to_string(),
                cost: 100,
                consumed: true,
            }),
            ..Spell::default()
        };
        assert!(!sheet.can_afford_material(&identify));
        assert!(!sheet.pay_material_cost(&identify));
        assert_eq!(sheet.gold, 0); // failed payment pays nothing
        sheet.gold = 150;
        assert!(sheet.can_afford_material(&identify));
        assert!(sheet.pay_material_cost(&identify));
        assert_eq!(sheet.gold, 50);

        // Pouch components (cost 0 / not consumed) are free by design.
        let fireball = Spell {
            name: "fireball".to_string(),
            material: Some(MaterialComponent {
                name: "a tiny ball of bat guano and sulfur".to_string(),
                cost: 0,
                consumed: false,
            }),
            ..Spell::default()
        };
        assert!(sheet.pay_material_cost(&fireball));
        assert_eq!(sheet.gold, 50);
    }
}
