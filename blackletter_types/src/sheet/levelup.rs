//! Level-up flow (plan Task 5.2): compute what the next level grants
//! from the corpus `Levels` rows, then apply it to a `Character`.
//!
//! The plan is pure data (roll-vs-fixed HP is the caller's choice);
//! `Character::apply_level_up` mutates the sheet: level, HP, prose
//! feature grants, ASI bumps, and the subclass choice at the class's
//! archetype level.

use crate::database::corpus::Corpus;
use crate::sheet::AbilityScores;
use crate::sheet::class::ClassType;
use crate::sheet::feature::Feature;
use crate::sheet::source::Source;
use crate::sheet::{Character, Score};

/// What the next level grants for one class in the stack.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelUpPlan {
    /// SRD class index ("fighter").
    pub class_index: String,
    /// The level being entered.
    pub new_level: i64,
    pub hit_die_max: i64,
    /// Fixed HP gain: half the die + 1 + CON modifier (PHB).
    pub hp_fixed: i64,
    /// Feature names granted at this level (corpus rows, display names).
    pub feature_names: Vec<String>,
    /// ASI points at this level (2 per ASI; 0 = no ASI here).
    pub asi_points: i32,
    /// Non-empty when this is the class's subclass level and the sheet
    /// has no subclass yet: (subclass index, name) options.
    pub subclass_options: Vec<(String, String)>,
    /// Gain in spells known / cantrips known (known-caster bookkeeping).
    pub spells_known_gain: i32,
    pub cantrips_known_gain: i32,
}

/// Compute the plan for `current_level + 1` of one class. `con_mod`
/// must already include every source (race grants included). Returns
/// `None` past level 20 or for classes with no level rows.
#[must_use]
pub fn plan_level_up(
    corpus: &Corpus,
    class: &ClassType,
    current_level: i64,
    subclass_index: Option<&str>,
    con_mod: i64,
) -> Option<LevelUpPlan> {
    let class_index = class.index().to_string();
    let rows = corpus.levels_for(&class_index, None);
    let new_level = current_level + 1;
    let row = rows.iter().find(|l| i64::from(l.level) == new_level)?;
    let prev = rows.iter().find(|l| i64::from(l.level) == current_level);

    let hit_die_max = i64::from(class.get_hit_dice().max());
    let feature_names = row
        .features
        .iter()
        .filter_map(|f| corpus.feature(&f.index).map(|f| f.name.clone()))
        .collect();

    let asi_delta = row.ability_score_bonuses - prev.map_or(0, |p| p.ability_score_bonuses);
    debug_assert!(
        asi_delta == 0 || asi_delta == 1,
        "unexpected ASI delta {asi_delta}"
    );

    // Subclass prompt: the class's first subclass level, if none chosen.
    let mut subclass_options = Vec::new();
    if subclass_index.unwrap_or("").is_empty()
        && let Some(db) = corpus.class(&class_index)
    {
        for sub in &db.subclasses {
            let first = corpus
                .levels_for(&class_index, Some(&sub.index))
                .first()
                .map_or(i64::MAX, |l| i64::from(l.level));
            if first == new_level {
                subclass_options.push((sub.index.clone(), sub.name.clone()));
            }
        }
    }

    let gain = |field: fn(&crate::database::level::SpellcastingProgression) -> Option<i32>| {
        let new = row.spellcasting.as_ref().and_then(field).unwrap_or(0);
        let old = prev
            .and_then(|p| p.spellcasting.as_ref())
            .and_then(field)
            .unwrap_or(0);
        new - old
    };

    Some(LevelUpPlan {
        class_index,
        new_level,
        hit_die_max,
        hp_fixed: hit_die_max / 2 + 1 + con_mod,
        feature_names,
        asi_points: asi_delta * 2,
        subclass_options,
        spells_known_gain: gain(|p| p.spells_known),
        cantrips_known_gain: gain(|p| p.cantrips_known),
    })
}

/// SRD multiclassing prerequisites (Wiki/03:52). Multiple entries are
/// AND ("Dexterity 13 and Wisdom 13"). The fighter's "Strength 13 or
/// Dexterity 13" is the only OR — and the SRD JSON carries NO fighter
/// prerequisites at all, so the table's rule is special-cased here.
#[must_use]
pub fn meets_multiclass_prerequisites(
    scores: &AbilityScores,
    class_index: &str,
    mc: Option<&crate::database::class::MultiClassing>,
) -> bool {
    if class_index == "fighter" {
        return scores.str.get() >= 13 || scores.dex.get() >= 13;
    }
    mc.is_none_or(|mc| {
        mc.prerequisites.iter().all(|p| {
            p.ability_score
                .index
                .parse::<Score>()
                .is_ok_and(|score| scores.get(&score).get() >= i64::from(p.minimum_score))
        })
    })
}

impl Character {
    /// Apply a level-up plan. `hp_gain` is the rolled or fixed amount
    /// (caller decides; `plan.hp_fixed` is the fixed option). `asi`
    /// distributes ASI points onto score bases (PHB: 2 to one or 1+1).
    /// `new_subclass` picks from `plan.subclass_options` (index, name)
    /// and grants that subclass's features for all levels up to the
    /// new level. Returns false if the class slot is out of range.
    pub fn apply_level_up(
        &mut self,
        class_pos: usize,
        plan: &LevelUpPlan,
        hp_gain: i64,
        asi: &[(Score, i64)],
        new_subclass: Option<(&str, &str)>,
    ) -> bool {
        if class_pos >= self.classes.classes.len() {
            return false;
        }
        let cl = &mut self.classes[class_pos];
        cl.set_level(plan.new_level);
        if let Some((idx, _name)) = new_subclass {
            // The index is the machine truth; the UI prettifies via the
            // corpus (falling back to the index).
            cl.subclass = idx.to_string();
        }

        self.health.max += hp_gain;
        self.health.current += hp_gain;

        for name in &plan.feature_names {
            if !self.has_grant(name) {
                self.add_feature(Feature {
                    name: name.clone(),
                    source: Some(Source::Single("SRD (2014)".into())),
                    effects: Vec::new(),
                });
            }
        }
        for (score, n) in asi {
            let scores: &mut AbilityScores = &mut self.ability_scores;
            let s = match score {
                Score::Str => &mut scores.str,
                Score::Dex => &mut scores.dex,
                Score::Con => &mut scores.con,
                Score::Int => &mut scores.int,
                Score::Wis => &mut scores.wis,
                Score::Cha => &mut scores.cha,
            };
            s.base = (s.base + n).min(30);
        }
        // The sheet's derived numbers ride the new level: proficiency
        // bonus (off total level) and spell-slot tables (off caster
        // level, pact kept separate).
        self.sync_proficiency_bonus();
        self.sync_spell_slots();
        true
    }

    /// Grant the chosen subclass's prose features (levels up to
    /// `level`). Needs the corpus, so it's a separate step from
    /// `apply_level_up` to keep the sheet corpus-free.
    pub fn grant_subclass_features(
        &mut self,
        corpus: &Corpus,
        class_index: &str,
        subclass_index: &str,
        level: i64,
    ) {
        for row in corpus.levels_for(class_index, Some(subclass_index)) {
            if i64::from(row.level) > level {
                continue;
            }
            for f in &row.features {
                if let Some(f) = corpus.feature(&f.index)
                    && !self.has_grant(&f.name)
                {
                    self.add_feature(Feature {
                        name: f.name.clone(),
                        source: Some(Source::Single("SRD (2014)".into())),
                        effects: Vec::new(),
                    });
                }
            }
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;
    use crate::database::reference::Edition;
    use std::path::Path;

    fn corpus() -> Corpus {
        Corpus::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../srd/2014/en"),
            Edition::Y2014,
        )
        .expect("vendored corpus loads")
    }

    fn fighter(con_mod: i64) -> Character {
        let mut c = Character::new(
            "Testy".into(),
            1,
            crate::sheet::HitPoints {
                current: 10 + con_mod,
                max: 10 + con_mod,
                level_bonus: 0,
                bonus: 0,
            },
            ClassType::Fighter,
            AbilityScores::default(),
        );
        c.classes.classes[0].set_class("fighter");
        c
    }

    #[test]
    fn fighter_1_to_20_grants_match_levels_json() {
        let corpus = corpus();
        let mut c = fighter(2);
        // Level-1 grants come from creation (the wizard), not level-up.
        for f in corpus.level_features("fighter", 1) {
            c.add_feature(Feature {
                name: f.name.clone(),
                source: Some(Source::Single("SRD (2014)".into())),
                effects: Vec::new(),
            });
        }
        let mut expected: Vec<String> = corpus
            .levels_for("fighter", None)
            .iter()
            .flat_map(|r| r.features.iter())
            .filter_map(|f| corpus.feature(&f.index).map(|f| f.name.clone()))
            .collect();
        expected.sort();
        expected.dedup();

        for _ in 1..20 {
            let lvl = c.classes[0].get_level();
            let sub = c.classes[0].subclass.clone();
            let plan =
                plan_level_up(&corpus, &ClassType::Fighter, lvl, Some(&sub), 2).expect("plan");
            let asi: Vec<(Score, i64)> = if plan.asi_points == 2 {
                vec![(Score::Str, 2)]
            } else {
                vec![]
            };
            let sub_pick = plan
                .subclass_options
                .first()
                .map(|(i, n)| (i.as_str(), n.as_str()));
            assert!(c.apply_level_up(0, &plan, plan.hp_fixed, &asi, sub_pick));
            // Subclass rows keep granting at later levels too (idempotent
            // via has_grant) — e.g. Champion 15 Superior Critical.
            let sub_now = match &sub_pick {
                Some((i, _)) => i.to_string(),
                None => sub.clone(),
            };
            if !sub_now.is_empty() {
                c.grant_subclass_features(&corpus, "fighter", &sub_now, plan.new_level);
            }
        }

        assert_eq!(c.classes[0].get_level(), 20);
        assert_eq!(c.classes.total_level(), 20);
        // Six ASIs landed on STR (default base 12 -> 24).
        assert_eq!(c.ability_scores.str.base, 24);
        // HP: 12 at 1 + 19 levels x (10/2+1+2=8) = 164.
        assert_eq!(c.health.max, 12 + 19 * 8);

        let mut got: Vec<String> = c.features.inner.iter().map(|f| f.name.clone()).collect();
        got.sort();
        got.dedup();
        // Every Levels.json fighter feature name is granted…
        for name in &expected {
            assert!(got.contains(name), "missing grant: {name}");
        }
        // …and the PHB anchors are all present.
        for anchor in [
            "Fighting Style",
            "Second Wind",
            "Action Surge (1 use)",
            "Martial Archetype",
            "Extra Attack",
            "Indomitable (1 use)",
            "Ability Score Improvement",
            // NB: PHB's Brave (9) and Tactical Awareness (13) are not in
            // the SRD's fighter rows — the corpus is a subset (plan risk
            // note); Improved Critical arrives via Champion.
        ] {
            assert!(got.contains(&anchor.to_string()), "missing {anchor}");
        }
        // Champion picked at 3: its features rode in.
        assert_eq!(c.classes[0].subclass, "champion");
        assert!(got.iter().any(|n| n.contains("Superior Critical")));
    }

    #[test]
    fn subclass_prompted_only_at_archetype_level() {
        let corpus = corpus();
        let p3 = plan_level_up(&corpus, &ClassType::Fighter, 2, Some(""), 0).unwrap();
        assert!(
            p3.subclass_options.iter().any(|(i, _)| i == "champion"),
            "level 3 must prompt the archetype"
        );
        let p4 = plan_level_up(&corpus, &ClassType::Fighter, 3, Some("champion"), 0).unwrap();
        assert!(p4.subclass_options.is_empty(), "already chosen");
    }

    #[test]
    fn asi_points_track_levels_json() {
        let corpus = corpus();
        let p4 = plan_level_up(&corpus, &ClassType::Fighter, 3, Some("champion"), 0).unwrap();
        assert_eq!(p4.asi_points, 2);
        let p5 = plan_level_up(&corpus, &ClassType::Fighter, 4, Some("champion"), 0).unwrap();
        assert_eq!(p5.asi_points, 0);
    }

    #[test]
    fn apply_level_up_resyncs_proficiency_bonus_and_slots() {
        let corpus = corpus();
        let mut wiz = Character::new(
            "W".into(),
            1,
            crate::sheet::HitPoints::new(8),
            ClassType::Wizard,
            AbilityScores::default(),
        );
        wiz.classes.classes[0].set_class("wizard");
        wiz.sync_spell_slots();
        assert_eq!(wiz.meters.meters["spell_slot_1"].slot_number, 2);
        for lvl in 2..=5 {
            let plan =
                plan_level_up(&corpus, &ClassType::Wizard, lvl - 1, Some(""), 1).expect("plan");
            wiz.apply_level_up(0, &plan, plan.hp_fixed, &[], None);
        }
        assert_eq!(wiz.classes.total_level(), 5);
        assert_eq!(wiz.skills.proficiency_bonus, 3, "PB follows total level");
        assert_eq!(wiz.meters.meters["spell_slot_1"].slot_number, 4);
        assert_eq!(wiz.meters.meters["spell_slot_3"].slot_number, 2);
    }

    #[test]
    fn warlock_known_spell_gains_track_levels_json() {
        let corpus = corpus();
        // SRD warlock: 2 known at 1-2, 3 at 3.
        let p3 = plan_level_up(&corpus, &ClassType::Warlock, 2, Some(""), 0).unwrap();
        assert_eq!(p3.spells_known_gain, 1);
        assert_eq!(p3.cantrips_known_gain, 0);
        let p4 = plan_level_up(&corpus, &ClassType::Warlock, 3, Some(""), 0).unwrap();
        assert_eq!(p4.cantrips_known_gain, 1);
    }

    #[test]
    fn multiclass_prerequisites_and_proficiencies_parse() {
        let corpus = corpus();
        let wiz = corpus.class("wizard").expect("wizard");
        let mc = wiz.multi_classing.as_ref().expect("multi_classing");
        assert_eq!(mc.prerequisites.len(), 1);
        assert_eq!(mc.prerequisites[0].ability_score.index, "int");
        assert_eq!(mc.prerequisites[0].minimum_score, 13);
        assert!(
            mc.proficiencies.is_empty(),
            "wizard grants none multiclassing"
        );
        let barb = corpus.class("barbarian").expect("barbarian");
        let profs: Vec<&str> = barb
            .multi_classing
            .as_ref()
            .unwrap()
            .proficiencies
            .iter()
            .map(|p| p.index.as_str())
            .collect();
        assert_eq!(profs, ["shields", "simple-weapons", "martial-weapons"]);
    }

    #[test]
    fn multiclass_prerequisite_checks_follow_the_srd_table() {
        let corpus = corpus();
        let mut scores = AbilityScores::default(); // all base 12
        let wiz = corpus.class("wizard").unwrap();
        assert!(
            !meets_multiclass_prerequisites(&scores, "wizard", wiz.multi_classing.as_ref()),
            "12 INT is not enough"
        );
        scores.int.base = 13;
        assert!(meets_multiclass_prerequisites(
            &scores,
            "wizard",
            wiz.multi_classing.as_ref()
        ));

        // Monk: Dex AND Wis.
        let monk = corpus.class("monk").unwrap();
        scores.int.base = 12;
        scores.dex.base = 13;
        assert!(
            !meets_multiclass_prerequisites(&scores, "monk", monk.multi_classing.as_ref()),
            "Wis still short"
        );
        scores.wis.base = 13;
        assert!(meets_multiclass_prerequisites(
            &scores,
            "monk",
            monk.multi_classing.as_ref()
        ));

        // Fighter: the SRD JSON row is empty, but the table says Str OR
        // Dex 13 — special-cased.
        let fighter = corpus.class("fighter").unwrap();
        scores.dex.base = 12;
        scores.wis.base = 12;
        scores.str.base = 13;
        assert!(
            meets_multiclass_prerequisites(&scores, "fighter", fighter.multi_classing.as_ref()),
            "STR 13 alone qualifies the fighter"
        );
    }

    #[test]
    fn fixed_hp_is_half_die_plus_one_plus_con() {
        let corpus = corpus();
        let p = plan_level_up(&corpus, &ClassType::Fighter, 1, Some(""), 2).unwrap();
        assert_eq!(p.hp_fixed, 8); // d10/2+1+2
        let p = plan_level_up(&corpus, &ClassType::Wizard, 1, Some(""), 2).unwrap();
        assert_eq!(p.hp_fixed, 6); // d6/2+1+2
    }
}
