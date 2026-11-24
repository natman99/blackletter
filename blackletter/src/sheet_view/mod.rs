//! Sheet mode: the editable character document + three-column layout.
//!
//! Layout (plan Task 3.1): left = section nav, center = the section form,
//! right = an always-visible summary card. The view never recomputes
//! modifiers — every number on screen comes from the engine's derived
//! getters (`armor_class_total()`, `skill_check()`, `passive_senses()`, …).

mod doc;

use std::path::PathBuf;
use std::sync::Arc;

use blackletter_types::{
    Item, Score,
    class::CastingMechanic,
    condition::{Condition, ConditionKind},
    damage::DamageType,
    database::corpus::Corpus,
    library::Library,
    roll::Advantage,
    sheet::proficiencies::{ArmorProf, WeaponProf},
    skills::Skill,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    KeyBinding, SharedString, StatefulInteractiveElement, WeakEntity, Window, actions, div,
    prelude::*,
};

pub use doc::SheetDoc;

use crate::field::{Field, FieldElement, FieldOwner};
use crate::picker::{PickKind, Picker, PickerOutcome};
use crate::root::RootView;

pub const SHEET_CTX: &str = "Sheet";

actions!(
    sheet,
    [
        Save,
        Undo,
        Redo,
        NextSection,
        PrevSection,
        EditName,
        SelectClassUp,
        SelectClassDown,
        LevelUp,
        LevelDown,
        AddClass,
        ToggleRow,
        QuickDamage,
        CycleDamageType,
        RollDeathSave,
        ShortRest,
        LongRest,
        TrayAttack,
        TraySave,
        TrayCheck,
        TrayDamage,
        OpenPicker,
        NewCustom,
    ]
);

pub fn keybindings() -> Vec<KeyBinding> {
    let ctx = Some(SHEET_CTX);
    let mut kb: Vec<KeyBinding> = Vec::new();
    macro_rules! bind {
        ($($key:literal => $action:ident),* $(,)?) => {
            $( kb.push(KeyBinding::new($key, $action, ctx)); )*
        };
    }
    bind!(
        // NB: not tab/shift-tab — gpui's default focus traversal fires even
        // when a context binding matches, which silently steals input focus
        // from the view's Field. Brackets keep the keyboard contract honest.
        "]" => NextSection,
        "[" => PrevSection,
        "j" => SelectClassDown,
        "k" => SelectClassUp,
        "plus" => LevelUp,
        "equal" => LevelUp,
        "minus" => LevelDown,
        "m" => AddClass,
        "enter" => EditName,
        "space" => ToggleRow,
        "d" => QuickDamage,
        "t" => CycleDamageType,
        "r" => RollDeathSave,
        "s" => ShortRest,
        "shift-s" => LongRest,
        "a" => TrayAttack,
        "v" => TraySave,
        "c" => TrayCheck,
        "x" => TrayDamage,
        "/" => OpenPicker,
        "n" => NewCustom,
        "ctrl-z" => Undo,
        "ctrl-shift-z" => Redo,
        "ctrl-s" => Save,
    );
    kb
}

/// The left-nav sections (plan Task 3.1 list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Basic,
    Abilities,
    Skills,
    Combat,
    Spells,
    Inventory,
    Features,
    Conditions,
    Notes,
}

impl Section {
    const ALL: [Section; 9] = [
        Section::Basic,
        Section::Abilities,
        Section::Skills,
        Section::Combat,
        Section::Spells,
        Section::Inventory,
        Section::Features,
        Section::Conditions,
        Section::Notes,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::Basic => "Basic",
            Section::Abilities => "Abilities",
            Section::Skills => "Skills & Saves",
            Section::Combat => "Combat",
            Section::Spells => "Spells",
            Section::Inventory => "Inventory",
            Section::Features => "Features & Feats",
            Section::Conditions => "Conditions",
            Section::Notes => "Notes",
        }
    }

    fn next(self) -> Section {
        let i = Self::ALL.iter().position(|s| *s == self).unwrap();
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn prev(self) -> Section {
        let i = Self::ALL.iter().position(|s| *s == self).unwrap();
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// What the one-line field is currently editing.
#[derive(Debug, Clone, Copy)]
enum FieldTarget {
    Name,
    /// Numeric prompt: damage or heal amount.
    Amount(bool),
    /// New inventory item name prompt.
    NewItem,
    /// Dice-tray roll prompt.
    Roll(RollKind),
}

/// What the dice tray is prompting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollKind {
    Attack,
    Save,
    Check,
    Damage,
}

/// The six ability rows in the Abilities section.
const SCORE_ROWS: [Score; 6] = [
    Score::Str,
    Score::Dex,
    Score::Con,
    Score::Int,
    Score::Wis,
    Score::Cha,
];

/// The action rows in the Combat section.
const COMBAT_ROWS: usize = 11;
const COMBAT_LABELS: [&str; COMBAT_ROWS] = [
    "apply damage (amount via d)",
    "heal (amount via enter)",
    "HP bonus (+/-)",
    "AC flat override (space toggles)",
    "gold (+/-)",
    "exhaustion (+/-)",
    "short rest (space)",
    "long rest (space)",
    "spend hit die (space)",
    "roll death save (space)",
    "xp (+/- by 100)",
];

/// The six toggleable proficiency categories (Skills section rows
/// 24..30); specific/tool proficiencies display below, read-only.
const PROF_LABELS: [(&str, ProfKind); 6] = [
    ("light armor", ProfKind::Armor(ArmorProf::Light)),
    ("medium armor", ProfKind::Armor(ArmorProf::Medium)),
    ("heavy armor", ProfKind::Armor(ArmorProf::Heavy)),
    ("shields", ProfKind::Armor(ArmorProf::Shields)),
    ("simple weapons", ProfKind::Weapon(WeaponProf::Simple)),
    ("martial weapons", ProfKind::Weapon(WeaponProf::Martial)),
];

#[derive(Debug, Clone, Copy)]
enum ProfKind {
    Armor(ArmorProf),
    Weapon(WeaponProf),
}

pub struct SheetView {
    doc: SheetDoc,
    /// The campaign's `characters/` dir for first-saves of new sheets.
    characters_dir: Option<PathBuf>,
    /// The SRD corpus for the pickers (plan Task 4.2); `None` disables them.
    corpus: Option<Arc<Corpus>>,
    /// The custom-content library (plan Task 4.3), merged into every picker.
    library: Entity<Library>,
    root: WeakEntity<RootView>,
    section: Section,
    focus_handle: FocusHandle,
    /// Index of the class the +/- keys act on (multiclass editing).
    class_sel: usize,
    /// Row selection inside the active section (reset on section change).
    sel: usize,
    /// Damage type the quick-damage keys cycle through (for resistances).
    damage_type: DamageType,
    field: Option<(FieldTarget, Field)>,
    /// The open SRD picker overlay, if any.
    picker: Option<Entity<Picker>>,
    /// The open level-up overlay (plan Task 5.2).
    levelup: Option<Entity<crate::levelup_view::LevelUpView>>,
    /// The open custom-entry form overlay, if any (plan Task 4.3).
    form: Option<Entity<crate::form::Form>>,
    /// Dice-tray result log, newest first (max 5).
    roll_log: Vec<String>,
    blank: Field,
    /// Scroll viewport for the center column's section (user ruling
    /// 2026-11-24: every section tab must scroll at short window heights).
    scroll: gpui::ScrollHandle,
}

impl SheetView {
    pub fn new(
        doc: SheetDoc,
        characters_dir: Option<PathBuf>,
        corpus: Option<Arc<Corpus>>,
        library: Entity<Library>,
        root: WeakEntity<RootView>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            doc,
            characters_dir,
            corpus,
            library,
            root,
            section: Section::Basic,
            focus_handle: cx.focus_handle(),
            class_sel: 0,
            sel: 0,
            damage_type: DamageType::Bludgeoning,
            field: None,
            picker: None,
            levelup: None,
            form: None,
            roll_log: Vec::new(),
            blank: Field::new(""),
            scroll: gpui::ScrollHandle::new(),
        }
    }

    pub fn doc(&self) -> &SheetDoc {
        &self.doc
    }

    #[allow(dead_code)] // read by tests
    pub fn section(&self) -> Section {
        self.section
    }

    /// Current vertical scroll offset of the center column (negative =
    /// scrolled down). Read by the scroll test.
    #[allow(dead_code)] // read by tests
    pub fn scroll_offset_y(&self) -> f32 {
        self.scroll.offset().y.as_f32()
    }

    fn report(&self, line: impl Into<SharedString>, cx: &mut Context<Self>) {
        let line = line.into();
        let root = self.root.clone();
        cx.defer(move |cx| {
            root.update(cx, |root, cx| root.set_status(line, cx)).ok();
        });
    }

    // ---- mutations: snapshot -> mutate -> notify -> dirty (plan ruling) ----

    fn on_next_section(&mut self, _: &NextSection, _: &mut Window, cx: &mut Context<Self>) {
        self.section = self.section.next();
        self.sel = 0;
        // Top strategy (not minimal-scroll): the stale child_bounds from
        // the outgoing section make a FirstVisible pass a no-op.
        self.scroll.scroll_to_top_of_item(0);
        cx.notify();
    }

    fn on_prev_section(&mut self, _: &PrevSection, _: &mut Window, cx: &mut Context<Self>) {
        self.section = self.section.prev();
        self.sel = 0;
        self.scroll.scroll_to_top_of_item(0);
        cx.notify();
    }

    /// Number of selectable rows in the active section.
    fn rows(&self) -> usize {
        match self.section {
            Section::Basic => self.doc.character.classes.classes.len(),
            Section::Abilities => 6,
            Section::Skills => 30,
            Section::Combat => COMBAT_ROWS,
            Section::Spells => self.slot_rows().len() + self.spell_rows().len(),
            Section::Conditions => self.condition_rows(),
            Section::Inventory => self.doc.character.inventory.0.len() + 1,
            Section::Features => self.grant_rows().len(),
            _ => 0,
        }
    }

    fn on_select_class_up(&mut self, _: &SelectClassUp, _: &mut Window, cx: &mut Context<Self>) {
        match self.section {
            Section::Basic => self.class_sel = self.class_sel.saturating_sub(1),
            _ => self.sel = self.sel.saturating_sub(1),
        }
        self.scroll_sel_into_view();
        cx.notify();
    }

    fn on_select_class_down(
        &mut self,
        _: &SelectClassDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.section {
            Section::Basic => {
                let n = self.doc.character.classes.classes.len();
                if n > 0 {
                    self.class_sel = (self.class_sel + 1).min(n - 1);
                }
            }
            _ => {
                let n = self.rows();
                if n > 0 {
                    self.sel = (self.sel + 1).min(n - 1);
                }
            }
        }
        self.scroll_sel_into_view();
        cx.notify();
    }

    // ---- dice tray (Task 3.3) ----

    fn open_roll_field(&mut self, kind: RollKind, cx: &mut Context<Self>) {
        let prompt = match kind {
            RollKind::Attack => "attack — [a]dv [d]is enter to roll",
            RollKind::Save => "save: str dex con int wis cha [a/d]",
            RollKind::Check => "check: skill name [a/d]",
            RollKind::Damage => "damage: e.g. 2d6+3 fire",
        };
        self.field = Some((FieldTarget::Roll(kind), Field::new("")));
        self.report(prompt, cx);
    }

    fn on_tray_attack(&mut self, _: &TrayAttack, _: &mut Window, cx: &mut Context<Self>) {
        self.open_roll_field(RollKind::Attack, cx);
    }

    fn on_tray_save(&mut self, _: &TraySave, _: &mut Window, cx: &mut Context<Self>) {
        self.open_roll_field(RollKind::Save, cx);
    }

    fn on_tray_check(&mut self, _: &TrayCheck, _: &mut Window, cx: &mut Context<Self>) {
        self.open_roll_field(RollKind::Check, cx);
    }

    fn on_tray_damage(&mut self, _: &TrayDamage, _: &mut Window, cx: &mut Context<Self>) {
        self.open_roll_field(RollKind::Damage, cx);
    }

    /// Pull a trailing advantage token ("a"/"adv"/"d"/"dis") out of the
    /// prompt text.
    fn take_advantage(tokens: &mut Vec<String>) -> Advantage {
        if let Some(last) = tokens.last() {
            match last.as_str() {
                "a" | "adv" | "advantage" => {
                    tokens.pop();
                    return Advantage::Advantage;
                }
                "d" | "dis" | "disadv" | "disadvantage" => {
                    tokens.pop();
                    return Advantage::Disadvantage;
                }
                _ => {}
            }
        }
        Advantage::Normal
    }

    fn log_roll(&mut self, line: String, cx: &mut Context<Self>) {
        self.roll_log.insert(0, line.clone());
        self.roll_log.truncate(5);
        self.report(line, cx);
    }

    fn outcome_line(kind: &str, bonus: i64, out: &blackletter_types::RollOutcome) -> String {
        let adv = match out.advantage {
            Advantage::Normal => String::new(),
            Advantage::Advantage => " (adv)".to_string(),
            Advantage::Disadvantage => " (dis)".to_string(),
        };
        let nat = out
            .result
            .natural
            .map(|n| format!(" d20 {n}"))
            .unwrap_or_default();
        let crit = if out.result.is_critical_hit() {
            " CRIT!"
        } else {
            ""
        };
        let auto = if out.auto_failed { " AUTO-FAIL" } else { "" };
        format!(
            "{kind} {bonus:+}: {}{nat}{adv}{auto}{crit}",
            out.result.total
        )
    }

    fn commit_roll(&mut self, kind: RollKind, text: &str, cx: &mut Context<Self>) {
        let mut tokens: Vec<String> = text.split_whitespace().map(|t| t.to_lowercase()).collect();
        let adv = Self::take_advantage(&mut tokens);
        let rng = &mut rand::rng();
        let line = match kind {
            RollKind::Attack => {
                let out = self.doc.character.roll_attack(rng, adv);
                Self::outcome_line("attack", out.bonus, &out)
            }
            RollKind::Save => {
                let Some(tok) = tokens.first() else {
                    self.report("save needs str dex con int wis cha", cx);
                    return;
                };
                let score = match tok.as_str() {
                    t if t.starts_with("st") => Score::Str,
                    t if t.starts_with("de") => Score::Dex,
                    t if t.starts_with("co") => Score::Con,
                    t if t.starts_with("in") => Score::Int,
                    t if t.starts_with("wi") => Score::Wis,
                    t if t.starts_with("ch") => Score::Cha,
                    _ => {
                        self.report("unknown save — str dex con int wis cha", cx);
                        return;
                    }
                };
                let out = self.doc.character.roll_save(rng, score, adv);
                Self::outcome_line(&format!("{score} save"), out.bonus, &out)
            }
            RollKind::Check => {
                let Some(tok) = tokens.first() else {
                    self.report("check needs a skill name", cx);
                    return;
                };
                let Some((i, _)) = Skill::ALL_STR
                    .iter()
                    .enumerate()
                    .find(|&(_, s)| s.starts_with(tok.as_str()))
                else {
                    self.report("unknown skill", cx);
                    return;
                };
                let skill = Skill::ALL[i];
                let out = self.doc.character.roll_check(rng, &skill, adv);
                Self::outcome_line(&format!("{skill} check"), out.bonus, &out)
            }
            RollKind::Damage => {
                let Some(expr) = tokens.first() else {
                    self.report("damage needs an expression, e.g. 2d6+3", cx);
                    return;
                };
                let Ok(roll) = expr.parse::<blackletter_types::roll::Roll>() else {
                    self.report("could not parse dice expression", cx);
                    return;
                };
                let kind_tok = tokens.get(1).map(String::as_str);
                let dtype = match kind_tok.unwrap_or("") {
                    t if t.starts_with("ac") => Some(DamageType::Acid),
                    t if t.starts_with("bl") => Some(DamageType::Bludgeoning),
                    t if t.starts_with("co") => Some(DamageType::Cold),
                    t if t.starts_with("fi") => Some(DamageType::Fire),
                    t if t.starts_with("fo") => Some(DamageType::Force),
                    t if t.starts_with("li") => Some(DamageType::Lightning),
                    t if t.starts_with("ne") => Some(DamageType::Necrotic),
                    t if t.starts_with("pi") => Some(DamageType::Piercing),
                    t if t.starts_with("po") => Some(DamageType::Poison),
                    t if t.starts_with("ps") => Some(DamageType::Psychic),
                    t if t.starts_with("ra") => Some(DamageType::Radiant),
                    t if t.starts_with("sl") => Some(DamageType::Slashing),
                    t if t.starts_with("th") => Some(DamageType::Thunder),
                    _ => None,
                };
                let result = roll.roll(rng, None);
                let raw = result.total;
                match dtype {
                    Some(dt) => {
                        let after = self.doc.character.apply_resistance(dt, false, raw);
                        let verb = match after.cmp(&raw) {
                            std::cmp::Ordering::Less => "resisted",
                            std::cmp::Ordering::Greater => "vulnerable",
                            std::cmp::Ordering::Equal => "no effect",
                        };
                        format!("{expr} {dt:?}: {raw} -> {after} ({verb})")
                    }
                    None => format!("{expr}: {raw}"),
                }
            }
        };
        self.log_roll(line, cx);
    }

    fn on_level_up(&mut self, _: &LevelUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.levelup.is_some() || self.picker.is_some() || self.form.is_some() {
            return;
        }
        // No corpus means no level rows to drive the flow: fall back to
        // the quick +1 bump so the key never becomes a dead end.
        let Some(corpus) = self.corpus.clone() else {
            self.on_adjust(1, cx);
            return;
        };
        let Some((plan, _con)) = crate::levelup_view::LevelUpView::plan_for(
            &corpus,
            &self.doc.character,
            self.class_sel,
        ) else {
            self.report(
                "no level-up available (level 20, or class not in corpus)",
                cx,
            );
            return;
        };
        let cl = &self.doc.character.classes.classes[self.class_sel];
        let label = format!("{} {}", cl.class, cl.get_level());
        let scores = &self.doc.character.ability_scores;
        let bases = [
            scores.str.base,
            scores.dex.base,
            scores.con.base,
            scores.int.base,
            scores.wis.base,
            scores.cha.base,
        ];
        let sheet = cx.entity().downgrade();
        let view = cx.new(|cx| {
            crate::levelup_view::LevelUpView::new(
                plan,
                label,
                bases,
                move |outcome, window, cx| {
                    sheet
                        .update(cx, |v, cx| v.on_levelup_done(outcome, window, cx))
                        .ok();
                },
                cx,
            )
        });
        self.levelup = Some(view.clone());
        window.focus(&view.read(cx).focus_handle(cx), cx);
        self.report("level up: follow the steps (enter next, esc cancel)", cx);
        cx.notify();
    }

    /// The level-up overlay's answer: apply the plan to the sheet in one
    /// undo snapshot, then chain the feat picker if the ASI was swapped.
    fn on_levelup_done(
        &mut self,
        outcome: crate::levelup_view::LevelUpOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Focus back BEFORE releasing the overlay entity (gpui clears
        // window focus after the update frame when the focused entity
        // is dropped mid-frame).
        window.focus(&self.focus_handle, cx);
        self.levelup = None;
        let crate::levelup_view::LevelUpOutcome::Apply {
            hp_gain,
            asi,
            subclass,
            feat_instead_of_asi,
        } = outcome
        else {
            self.report("level-up cancelled", cx);
            cx.notify();
            return;
        };
        let Some(corpus) = self.corpus.clone() else {
            cx.notify();
            return;
        };
        let class_pos = self.class_sel;
        let sub = subclass.clone();
        let res = self.doc.try_edit(|c| {
            let plan = crate::levelup_view::LevelUpView::plan_for(&corpus, c, class_pos)
                .ok_or(())?
                .0;
            if !c.apply_level_up(
                class_pos,
                &plan,
                hp_gain,
                &asi,
                sub.as_ref().map(|(i, n)| (i.as_str(), n.as_str())),
            ) {
                return Err(());
            }
            if let Some((i, _)) = &sub {
                c.grant_subclass_features(&corpus, &plan.class_index, i, plan.new_level);
            }
            c.sync_proficiency_bonus();
            c.sync_spell_slots();
            Ok(())
        });
        match &res {
            Ok(()) => {
                let c = &self.doc.character;
                let cl = &c.classes.classes[class_pos];
                self.report(
                    format!(
                        "{} is level {} (+{} HP, PB {})",
                        cl.class,
                        cl.get_level(),
                        hp_gain,
                        c.skills.proficiency_bonus
                    ),
                    cx,
                );
            }
            Err(_) => self.report("level-up could not be applied", cx),
        }
        cx.notify();
        if res.is_ok() && feat_instead_of_asi {
            self.open_picker(crate::picker::PickKind::Feat, window, cx);
        }
    }

    /// Multiclass: add a second class at level 1 (plan Task 5.2). The
    /// class picker's answer is gated by the SRD multiclass prerequisite
    /// table at pick time; PHB grants only the reduced proficiency set
    /// and the class's level-1 features (no skills, no equipment).
    fn on_add_class(&mut self, _: &AddClass, window: &mut Window, cx: &mut Context<Self>) {
        if self.levelup.is_some() || self.picker.is_some() || self.form.is_some() {
            return;
        }
        let Some(corpus) = self.corpus.clone() else {
            self.report("SRD corpus not found — pickers disabled", cx);
            return;
        };
        let lib = self.library.read(cx).clone();
        let sheet = cx.entity().downgrade();
        let picker = cx.new(|cx| {
            Picker::new(
                crate::picker::PickKind::Class,
                corpus,
                Some(&lib),
                move |outcome, window, cx| {
                    sheet
                        .update(cx, |v, cx| v.on_add_class_pick(outcome, window, cx))
                        .ok();
                },
                cx,
            )
        });
        self.picker = Some(picker.clone());
        window.focus(&picker.read(cx).focus_handle(cx), cx);
        self.report(
            "add class: type to filter, enter picks (prerequisites checked)",
            cx,
        );
        cx.notify();
    }

    fn on_add_class_pick(
        &mut self,
        outcome: crate::picker::PickerOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Focus back BEFORE releasing the picker entity (gpui drop rule).
        window.focus(&self.focus_handle, cx);
        self.picker = None;
        let crate::picker::PickerOutcome::Chosen(index) = outcome else {
            self.report("add class cancelled", cx);
            cx.notify();
            return;
        };
        self.add_multiclass(&index, cx);
        cx.notify();
    }

    fn add_multiclass(&mut self, class_index: &str, cx: &mut Context<Self>) {
        let Some(corpus) = self.corpus.clone() else {
            return;
        };
        let Some(db) = corpus.class(class_index) else {
            self.report("corpus class vanished", cx);
            return;
        };
        if !blackletter_types::sheet::levelup::meets_multiclass_prerequisites(
            &self.doc.character.ability_scores,
            class_index,
            db.multi_classing.as_ref(),
        ) {
            let need: Vec<String> = db
                .multi_classing
                .as_ref()
                .map(|mc| {
                    mc.prerequisites
                        .iter()
                        .map(|p| format!("{} {}", p.ability_score.index, p.minimum_score))
                        .collect()
                })
                .unwrap_or_default();
            self.report(
                format!(
                    "{class_index} needs {} — multiclass prerequisite not met",
                    if need.is_empty() {
                        "the class rule".to_string()
                    } else {
                        need.join(" and ")
                    }
                ),
                cx,
            );
            return;
        }
        let res = self.doc.try_edit(|c| {
            let fresh = blackletter_types::sheet::class::Class::try_from_database(db);
            if c.classes.classes.iter().any(|cl| cl.class == fresh.class) {
                anyhow::bail!("{class_index} is already in the class stack");
            }
            // Level-1 HP rule for the new class: full die + CON mod.
            let hp = i64::from(fresh.hit_dice.max()) + c.ability_scores.con.modifier();
            c.classes.classes.push(fresh);
            c.health.max += hp;
            c.health.current += hp;
            // REDUCED multiclass proficiencies only (Wiki/03:87); PHB
            // grants no skill proficiencies or equipment to a new class.
            if let Some(mc) = &db.multi_classing {
                c.gain_multiclass_proficiencies(mc);
            }
            for f in corpus.level_features(class_index, 1) {
                if !c.has_grant(&f.name) {
                    c.add_feature(blackletter_types::feature::Feature {
                        name: f.name.clone(),
                        source: Some(blackletter_types::sheet::source::Source::Single(
                            "SRD (2014)".into(),
                        )),
                        effects: Vec::new(),
                    });
                }
            }
            c.sync_proficiency_bonus();
            c.sync_spell_slots();
            Ok(())
        });
        match &res {
            Ok(()) => {
                let c = &self.doc.character;
                let pos = c.classes.classes.len() - 1;
                let cl = &c.classes.classes[pos];
                self.class_sel = pos;
                let mut note = format!(
                    "{} {} added (total {}, PB {})",
                    cl.class,
                    cl.get_level(),
                    c.classes.total_level(),
                    c.skills.proficiency_bonus
                );
                if corpus
                    .class(class_index)
                    .is_some_and(|d| d.spellcasting.is_some())
                {
                    note.push_str(" — spell slots resynced");
                }
                self.report(note, cx);
            }
            Err(e) => self.report(e.to_string(), cx),
        }
    }

    fn on_level_down(&mut self, _: &LevelDown, _: &mut Window, cx: &mut Context<Self>) {
        self.on_adjust(-1, cx);
    }

    /// `+`/`-` is section-aware: class level in Basic, ability score in
    /// Abilities, exhaustion in Combat.
    fn on_adjust(&mut self, delta: i64, cx: &mut Context<Self>) {
        match self.section {
            Section::Basic => self.change_level(delta, cx),
            Section::Abilities => {
                let idx = self.sel.min(5);
                let _ = self.doc.try_edit(|c| {
                    let mut scores = [
                        &mut c.ability_scores.str,
                        &mut c.ability_scores.dex,
                        &mut c.ability_scores.con,
                        &mut c.ability_scores.int,
                        &mut c.ability_scores.wis,
                        &mut c.ability_scores.cha,
                    ];
                    let s = &mut scores[idx];
                    s.base = (s.base + delta).clamp(1, 30);
                    Ok::<_, ()>(Some(()))
                });
                cx.notify();
            }
            Section::Combat => self.combat_adjust(delta, cx),
            Section::Inventory => self.item_adjust(delta, cx),
            Section::Conditions => self.adjust_condition_duration(delta, cx),
            _ => {}
        }
    }

    fn change_level(&mut self, delta: i64, cx: &mut Context<Self>) {
        let changed = self.doc.try_edit(|c| {
            let Some(class) = c.classes.classes.get_mut(self.class_sel) else {
                return Err(());
            };
            let next = class.level.0 + delta;
            if !(1..=20).contains(&next) {
                return Err(());
            }
            class.level.0 = next;
            c.sync_proficiency_bonus();
            c.sync_spell_slots();
            Ok(())
        });
        match changed {
            Ok(()) => {
                let c = &self.doc.character;
                let name = c.classes.classes[self.class_sel].class.to_string();
                let lvl = c.classes.classes[self.class_sel].level.0;
                self.report(
                    format!("{name} is level {lvl} (PB {})", c.skills.proficiency_bonus),
                    cx,
                );
            }
            Err(()) => self.report("level unchanged (bounds 1-20, or no class)", cx),
        }
        cx.notify();
    }

    // ---- combat: damage/heal/rest/death saves (engine is the source) ----

    fn on_quick_damage(&mut self, _: &QuickDamage, _: &mut Window, cx: &mut Context<Self>) {
        self.field = Some((FieldTarget::Amount(true), Field::new("")));
        self.report(
            format!(
                "damage amount (type: {:?}) — enter applies",
                self.damage_type,
            ),
            cx,
        );
        cx.notify();
    }

    fn on_cycle_damage_type(
        &mut self,
        _: &CycleDamageType,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section == Section::Spells {
            self.cast_spell(cx);
            return;
        }
        if self.section == Section::Conditions {
            self.doc.edit(|c| c.tick_conditions());
            self.report("durations ticked; expired conditions removed", cx);
            cx.notify();
            return;
        }
        let all = DamageType::ALL;
        let i = all.iter().position(|d| *d == self.damage_type).unwrap_or(0);
        self.damage_type = all[(i + 1) % all.len()];
        self.report(format!("damage type: {:?}", self.damage_type), cx);
        cx.notify();
    }

    fn apply_damage(&mut self, amount: i64, cx: &mut Context<Self>) {
        let kind = self.damage_type;
        let outcome = self.doc.try_edit(|c| {
            if c.health.current == 0 {
                c.damage_while_down(amount, false);
                return Ok::<_, ()>(None);
            }
            let dealt = c.apply_resistance(kind, true, amount);
            if c.check_instant_death(dealt) {
                return Ok(Some((dealt, None)));
            }
            c.health.current = (c.health.current - dealt).max(0);
            let conc = c
                .concentration_save(&mut rand::rng(), dealt)
                .map(|s| (s.spell_name, s.broken));
            Ok(Some((dealt, conc)))
        });
        match outcome {
            Ok(Some((dealt, conc))) => {
                let mut line = if self.doc.character.is_dead() {
                    format!("{amount} {:?} -> {dealt} dealt — INSTANT DEATH", kind)
                } else {
                    format!(
                        "{amount} {:?} -> {dealt} dealt (HP {})",
                        kind, self.doc.character.health.current
                    )
                };
                if let Some((name, broken)) = conc {
                    line += &format!(
                        "; concentration on {name} {}",
                        if broken { "BROKEN" } else { "held" }
                    );
                }
                self.report(line, cx);
            }
            Ok(None) => self.report(format!("{amount} damage while down (HP 0)",), cx),
            Err(()) => unreachable!("try_edit closure never errors here"),
        }
        cx.notify();
    }

    fn apply_heal(&mut self, amount: i64, cx: &mut Context<Self>) {
        self.doc.edit(|c| {
            c.health.current = (c.health.current + amount).min(c.max_health_total());
        });
        self.report(
            format!("healed {amount} (HP {})", self.doc.character.health.current),
            cx,
        );
        cx.notify();
    }

    fn on_roll_death_save(&mut self, _: &RollDeathSave, _: &mut Window, cx: &mut Context<Self>) {
        self.do_death_save(cx);
    }

    fn do_death_save(&mut self, cx: &mut Context<Self>) {
        let outcome = self.doc.edit_return(|c| {
            let (roll, outcome) = c.roll_death_save(&mut rand::rng());
            (roll.natural, outcome)
        });
        let ds = &self.doc.character.death_saves;
        self.report(
            format!(
                "death save: nat {:?} -> {:?} ({}S/{}F{})",
                outcome.0,
                outcome.1,
                ds.successes,
                ds.failures,
                if ds.stable { ", stable" } else { "" }
            ),
            cx,
        );
        if self.doc.character.is_dead() {
            self.report("THE CHARACTER IS DEAD", cx);
        }
        cx.notify();
    }

    fn do_short_rest(&mut self, cx: &mut Context<Self>) {
        self.doc.edit(|c| c.short_rest());
        self.report("short rest — hit dice and short-rest meters restored", cx);
        cx.notify();
    }

    fn do_long_rest(&mut self, cx: &mut Context<Self>) {
        self.doc.edit(|c| c.long_rest());
        self.report("long rest — HP full, slots restored, exhaustion -1", cx);
        cx.notify();
    }

    fn on_short_rest(&mut self, _: &ShortRest, _: &mut Window, cx: &mut Context<Self>) {
        self.do_short_rest(cx);
    }

    fn on_long_rest(&mut self, _: &LongRest, _: &mut Window, cx: &mut Context<Self>) {
        self.do_long_rest(cx);
    }

    /// space: activate the selected row (skills/saves toggles, combat actions).
    fn on_toggle_row(&mut self, _: &ToggleRow, window: &mut Window, cx: &mut Context<Self>) {
        match self.section {
            Section::Skills => self.toggle_skill_or_save(cx),
            Section::Combat => self.combat_activate(cx),
            Section::Spells => self.toggle_spell_row(cx),
            Section::Conditions => self.toggle_condition_row(cx),
            Section::Inventory => self.toggle_item_row(cx),
            Section::Features => self.grant_activate(window, cx),
            _ => {}
        }
        cx.notify();
    }

    fn toggle_skill_or_save(&mut self, cx: &mut Context<Self>) {
        if self.sel < 18 {
            let skill = Skill::ALL[self.sel];
            self.doc.edit(|c| {
                c.skills.proficiencies.toggle(skill);
            });
            let prof = self.doc.character.skills.proficiencies.contains(skill);
            self.report(
                format!(
                    "{} {}",
                    skill,
                    if prof { "proficient" } else { "not proficient" }
                ),
                cx,
            );
        } else if let Some((label, kind)) = PROF_LABELS.get(self.sel.saturating_sub(24)) {
            let on = self.doc.try_edit(move |c| {
                match kind {
                    ProfKind::Armor(a) => c.proficiencies.armor.toggle(*a),
                    ProfKind::Weapon(w) => c.proficiencies.weapons.toggle(*w),
                }
                Ok::<_, ()>(match kind {
                    ProfKind::Armor(a) => c.proficiencies.armor.contains(*a),
                    ProfKind::Weapon(w) => c.proficiencies.weapons.contains(*w),
                })
            });
            if let Ok(on) = on {
                self.report(
                    format!(
                        "{label} {}",
                        if on { "proficient" } else { "not proficient" }
                    ),
                    cx,
                );
            }
        } else if let Some(score) = SCORE_ROWS.get(self.sel - 18).copied() {
            let now = self.doc.try_edit(move |c| {
                let flag = match score {
                    Score::Str => &mut c.saving_throws.str,
                    Score::Dex => &mut c.saving_throws.dex,
                    Score::Con => &mut c.saving_throws.con,
                    Score::Int => &mut c.saving_throws.int,
                    Score::Wis => &mut c.saving_throws.wis,
                    Score::Cha => &mut c.saving_throws.cha,
                };
                *flag = !*flag;
                Ok::<_, ()>(*flag)
            });
            if let Ok(on) = now {
                self.report(
                    format!(
                        "{score} save {}",
                        if on { "proficient" } else { "not proficient" }
                    ),
                    cx,
                );
            }
        }
    }

    fn combat_activate(&mut self, cx: &mut Context<Self>) {
        match self.sel {
            0 => {
                self.field = Some((FieldTarget::Amount(true), Field::new("")));
                self.report("damage amount — enter applies", cx);
            }
            1 => {
                self.field = Some((FieldTarget::Amount(false), Field::new("")));
                self.report("heal amount — enter applies", cx);
            }
            3 => {
                let on = self.doc.edit_return(|c| {
                    c.armor_class.flat = match c.armor_class.flat {
                        Some(_) => None,
                        None => Some(c.armor_class_total()),
                    };
                    c.armor_class.flat
                });
                self.report(
                    match on {
                        Some(v) => format!("AC pinned to {v}"),
                        None => "AC derived from gear and Dex again".to_string(),
                    },
                    cx,
                );
            }
            6 => self.do_short_rest(cx),
            7 => self.do_long_rest(cx),
            8 => self.do_spend_hit_die(cx),
            9 => self.do_death_save(cx),
            _ => {}
        }
    }

    /// Spend one hit die from the first class that has dice left.
    fn do_spend_hit_die(&mut self, cx: &mut Context<Self>) {
        let idx = self
            .doc
            .character
            .classes
            .classes
            .iter()
            .position(|k| k.healing_die_remaining > 0);
        let Some(idx) = idx else {
            self.report("no hit dice remaining", cx);
            return;
        };
        let healed = self.doc.edit_return(|c| c.spend_hit_die(idx, true));
        match healed {
            Some(amount) => {
                let c = &self.doc.character;
                self.report(
                    format!(
                        "hit die spent: +{amount} HP ({} / {}){}",
                        c.health.current,
                        c.max_health_total(),
                        if c.classes.classes[idx].healing_die_remaining == 0 {
                            " — last die"
                        } else {
                            ""
                        }
                    ),
                    cx,
                );
            }
            None => self.report("at max HP — no die spent", cx),
        }
    }

    /// +/- inside the Combat section, per selected row.
    /// +/- in the Inventory section: quantity up/down; a decrement that
    /// reaches 0 removes the item entirely.
    fn item_adjust(&mut self, delta: i64, cx: &mut Context<Self>) {
        let Some((name, qty, _)) = self.item_rows().get(self.sel).cloned() else {
            return;
        };
        let new = qty + delta as i32;
        if new <= 0 && qty > 0 && delta < 0 {
            let name_for_edit = name.clone();
            self.doc.edit(move |c| {
                c.inventory.0.remove(&name_for_edit);
            });
            self.report(format!("{name} removed from inventory"), cx);
            self.sel = self
                .sel
                .min(self.doc.character.inventory.0.len().saturating_sub(1));
        } else if new > 0 {
            let name_for_edit = name.clone();
            self.doc.edit(move |c| {
                if let Some(it) = c.inventory.0.get_mut(&name_for_edit) {
                    it.quantity = new;
                }
            });
            self.report(format!("{name} x{new}"), cx);
        }
    }

    fn combat_adjust(&mut self, delta: i64, cx: &mut Context<Self>) {
        match self.sel {
            2 => {
                self.doc.edit(|c| {
                    c.health.bonus = (c.health.bonus + delta).max(0);
                });
                self.report(format!("temp HP {}", self.doc.character.health.bonus), cx);
            }
            4 => {
                self.doc.edit(|c| c.gold = (c.gold + delta).max(0));
                self.report(format!("gold {}", self.doc.character.gold), cx);
            }
            5 => {
                let lvl = (self.doc.character.exhaustion as i64 + delta).clamp(0, 6) as u8;
                self.doc.edit(|c| c.set_exhaustion(lvl));
                self.report(format!("exhaustion {lvl}"), cx);
            }
            10 => {
                self.doc.edit(|c| {
                    c.xp = (c.xp + delta * 100).max(0);
                });
                let c = &self.doc.character;
                let hint = if c.xp_ready_to_level() {
                    format!(
                        " — enough for level {} (level up when you want)",
                        c.classes.total_level() + 1
                    )
                } else {
                    c.xp_next_threshold()
                        .map_or(String::new(), |t| format!(" (next level at {t})"))
                };
                self.report(format!("xp {}{}", c.xp, hint), cx);
            }
            _ => self.report("+/- does nothing on this row", cx),
        }
        cx.notify();
    }

    fn on_edit_name(&mut self, _: &EditName, _: &mut Window, cx: &mut Context<Self>) {
        let initial = self.doc.character.name.clone();
        let mut f = Field::new(&initial);
        f.select_all();
        self.field = Some((FieldTarget::Name, f));
        self.report("editing name — enter commits, esc cancels", cx);
        cx.notify();
    }

    fn commit_field(&mut self, cx: &mut Context<Self>) {
        let Some((target, field)) = self.field.take() else {
            return;
        };
        match target {
            FieldTarget::Name => {
                let text = field.text().trim().to_string();
                if !text.is_empty() {
                    self.doc.edit(|c| c.name = text.clone());
                    self.report(format!("renamed to {text}"), cx);
                }
            }
            FieldTarget::Roll(kind) => self.commit_roll(kind, field.text().trim(), cx),
            FieldTarget::NewItem => {
                let text = field.text().trim().to_string();
                if text.is_empty() {
                    self.report("item name needed", cx);
                } else {
                    let name = text.clone();
                    let added = self.doc.try_edit(move |c| {
                        Ok::<_, ()>(
                            c.inventory
                                .0
                                .insert(name.clone(), Item::new(name))
                                .is_none(),
                        )
                    });
                    self.report(
                        match added {
                            Ok(true) => format!("added {text}"),
                            _ => format!("{text} already in inventory"),
                        },
                        cx,
                    );
                }
            }
            FieldTarget::Amount(damage) => {
                let text = field.text().trim().to_string();
                match text.parse::<i64>() {
                    Ok(n) if n > 0 => {
                        if damage {
                            self.apply_damage(n, cx);
                        } else {
                            self.apply_heal(n, cx);
                        }
                    }
                    _ => self.report("enter a positive whole number", cx),
                }
            }
        }
        cx.notify();
    }

    fn on_field_submit(
        &mut self,
        _: &crate::field::FieldSubmit,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_field(cx);
    }

    fn on_field_cancel(
        &mut self,
        _: &crate::field::FieldCancel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.field.take().is_some() {
            self.report("edit cancelled", cx);
            cx.notify();
        }
    }

    // ---- SRD pickers (plan Task 4.2) ----

    /// Which picker `/` opens from each section.
    fn kind_for_section(section: Section) -> Option<PickKind> {
        match section {
            Section::Basic => Some(PickKind::Race),
            Section::Spells => Some(PickKind::Spell),
            Section::Inventory => Some(PickKind::Item),
            Section::Features => Some(PickKind::Feat),
            _ => None,
        }
    }

    fn on_open_picker(&mut self, _: &OpenPicker, window: &mut Window, cx: &mut Context<Self>) {
        match Self::kind_for_section(self.section) {
            Some(kind) => self.open_picker(kind, window, cx),
            None => self.report("no SRD picker in this section", cx),
        }
    }

    fn open_picker(&mut self, kind: PickKind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(corpus) = self.corpus.clone() else {
            self.report("SRD corpus not found — pickers disabled", cx);
            return;
        };
        let lib = self.library.read(cx).clone();
        let sheet = cx.entity().downgrade();
        let picker = cx.new(|cx| {
            Picker::new(
                kind,
                corpus,
                Some(&lib),
                move |outcome, window, cx| {
                    sheet
                        .update(cx, |v, cx| v.on_pick(kind, outcome, window, cx))
                        .ok();
                },
                cx,
            )
        });
        self.picker = Some(picker.clone());
        window.focus(&picker.read(cx).focus_handle(cx), cx);
        self.report(
            "picker: type to filter, up/down move, enter picks, esc closes",
            cx,
        );
        cx.notify();
    }

    /// Open the custom-entry creation form for the current section's kind
    /// (plan Task 4.3): the menu for adding homebrew content to the library.
    fn on_new_custom(&mut self, _: &NewCustom, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = Self::kind_for_section(self.section) else {
            self.report("no custom-entry menu in this section", cx);
            return;
        };
        if kind == PickKind::Race {
            // Races carry traits, bonuses, and languages — a flat form is
            // the wrong shape; creation stays deferred (plan note).
            self.report("custom races not supported yet", cx);
            return;
        }
        if self.picker.is_some() || self.form.is_some() {
            return;
        }
        let specs = form_specs(kind);
        let title = format!("New custom {} (saved to the library)", kind.label());
        let sheet = cx.entity().downgrade();
        let form = cx.new(|cx| {
            crate::form::Form::new(
                title,
                &specs,
                move |outcome, window, cx| {
                    sheet
                        .update(cx, |v, cx| v.on_form(kind, outcome, window, cx))
                        .ok();
                },
                cx,
            )
        });
        self.form = Some(form.clone());
        window.focus(&form.read(cx).focus_handle(cx), cx);
        self.report("fill the rows, then ctrl-s to save", cx);
        cx.notify();
    }

    /// The form's answer: dismiss it, turn the pairs into a typed entry,
    /// insert it into the library, and cache the library to disk.
    fn on_form(
        &mut self,
        kind: PickKind,
        outcome: crate::form::FormOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.form = None;
        window.focus(&self.focus_handle, cx);
        let crate::form::FormOutcome::Saved(pairs) = outcome else {
            self.report("form closed", cx);
            cx.notify();
            return;
        };
        use std::str::FromStr;
        let get = |label: &str| {
            pairs
                .iter()
                .find(|(l, _)| l == label)
                .map(|(_, v)| v.trim().to_owned())
                .unwrap_or_default()
        };
        let name = get("name");
        if name.is_empty() {
            self.report("a name is required — nothing saved", cx);
            cx.notify();
            return;
        }
        let entry = match kind {
            PickKind::Spell => {
                use blackletter_types::sheet::spells::Spell;
                Spell {
                    name: name.clone(),
                    desc: get("description"),
                    level: get("level").parse().unwrap_or(1),
                    school: blackletter_types::sheet::spells::School::from_str(&get("school"))
                        .unwrap_or_default(),
                    cast_time: get("cast time"),
                    range: get("range"),
                    duration: get("duration"),
                    ..Default::default()
                }
            }
            PickKind::Item => {
                let mut item = Item::new(name.clone());
                item.description = get("description");
                item.quantity = get("quantity").parse().unwrap_or(1).max(1);
                // Item branch returns the Item; Spell arm returns a Spell —
                // the match below dispatches per kind, so arms stay separate.
                let dup = self
                    .library
                    .update(cx, |lib, _| lib.items.insert(name.clone(), item).is_some());
                self.finish_library_add(kind, name, dup, cx);
                cx.notify();
                return;
            }
            PickKind::Feat => {
                let feat = blackletter_types::sheet::feat::Feat {
                    name: name.clone(),
                    desc: get("description"),
                    feature: None,
                    source: None,
                };
                let dup = self
                    .library
                    .update(cx, |lib, _| lib.feats.insert(name.clone(), feat).is_some());
                self.finish_library_add(kind, name, dup, cx);
                cx.notify();
                return;
            }
            PickKind::Feature => {
                let desc = get("description");
                let feat = blackletter_types::feature::Feature {
                    name: name.clone(),
                    source: None,
                    effects: if desc.is_empty() {
                        Vec::new()
                    } else {
                        vec![blackletter_types::sheet::feature::Effect::Custom(desc)]
                    },
                };
                let dup = self.library.update(cx, |lib, _| {
                    lib.features.insert(name.clone(), feat).is_some()
                });
                self.finish_library_add(kind, name, dup, cx);
                cx.notify();
                return;
            }
            PickKind::Trait => {
                let tr = blackletter_types::sheet::traits::Trait {
                    name: name.clone(),
                    desc: get("description"),
                    feature: None,
                    source: None,
                };
                let dup = self
                    .library
                    .update(cx, |lib, _| lib.traits.insert(name.clone(), tr).is_some());
                self.finish_library_add(kind, name, dup, cx);
                cx.notify();
                return;
            }
            PickKind::Race | PickKind::Class | PickKind::Background | PickKind::Subrace => {
                unreachable!("on_new_custom rejects these kinds")
            }
        };
        let dup = self.library.update(cx, |lib, _| {
            lib.spells.insert(name.clone(), entry).is_some()
        });
        self.finish_library_add(kind, name, dup, cx);
        cx.notify();
    }

    /// Shared tail of a library add: duplicate report or save-to-disk
    /// report (Task 4.3: added content is cached for future sessions).
    fn finish_library_add(
        &mut self,
        kind: PickKind,
        name: String,
        dup: bool,
        cx: &mut Context<Self>,
    ) {
        if dup {
            self.report(
                format!("{name} is already in the library — nothing saved"),
                cx,
            );
            return;
        }
        let saved = self
            .root
            .update(cx, |r, cx| r.save_library(cx))
            .unwrap_or(false);
        let noun = match kind {
            PickKind::Spell => "spell",
            PickKind::Item => "item",
            PickKind::Feat => "feat",
            PickKind::Feature => "feature",
            PickKind::Trait => "trait",
            PickKind::Race => "race",
            PickKind::Class | PickKind::Background | PickKind::Subrace => "entry",
        };
        if saved {
            self.report(
                format!("added custom {noun} {name} to the library (saved)"),
                cx,
            );
        } else {
            self.report(
                format!("added custom {noun} {name} (could not save library file)"),
                cx,
            );
        }
    }

    /// The picker's answer: dismiss it, refocus the sheet, apply the choice.
    fn on_pick(
        &mut self,
        kind: PickKind,
        outcome: PickerOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.picker = None;
        window.focus(&self.focus_handle, cx);
        match outcome {
            PickerOutcome::Chosen(index) => self.apply_pick(kind, &index, cx),
            PickerOutcome::Cancelled => self.report("picker closed", cx),
        }
        cx.notify();
    }

    /// Import the picked corpus entry into the sheet through the engine's
    /// conversions, reporting success and failure to the status bar.
    fn apply_pick(&mut self, kind: PickKind, index: &str, cx: &mut Context<Self>) {
        if Library::is_custom(index) {
            self.apply_custom_pick(kind, index, cx);
            return;
        }
        let Some(corpus) = self.corpus.clone() else {
            return;
        };
        let srd = || blackletter_types::sheet::source::Source::Single("SRD (2014)".into());
        match kind {
            PickKind::Spell => {
                let Some(db) = corpus.spell(index).cloned() else {
                    self.report("corpus entry vanished", cx);
                    return;
                };
                let outcome = self.doc.try_edit(|c| {
                    let sp =
                        blackletter_types::sheet::spells::Spell::try_from_database(db.clone(), c)?;
                    let name = sp.name.clone();
                    if c.spells.spells.insert(name.clone(), sp).is_some() {
                        anyhow::bail!("{name} is already in the spellbook");
                    }
                    Ok(name)
                });
                match outcome {
                    Ok(name) => self.report(format!("added {name} to the spellbook"), cx),
                    Err(e) => self.report(format!("could not add spell: {e}"), cx),
                }
            }
            PickKind::Item => {
                let Some(eq) = corpus.item(index).cloned() else {
                    self.report("corpus entry vanished", cx);
                    return;
                };
                let outcome = self.doc.try_edit(|c| {
                    let item = Item::try_from_database(eq.clone())?;
                    let name = item.name.clone();
                    if c.inventory.0.insert(name.clone(), item).is_some() {
                        anyhow::bail!("{name} is already in the inventory");
                    }
                    Ok(name)
                });
                match outcome {
                    Ok(name) => self.report(format!("added {name} to the inventory"), cx),
                    Err(e) => self.report(format!("could not add item: {e}"), cx),
                }
            }
            PickKind::Feat => {
                let Some(db) = corpus.feats.iter().find(|f| f.index == index).cloned() else {
                    self.report("corpus entry vanished", cx);
                    return;
                };
                let feat = blackletter_types::sheet::feat::Feat::try_from_database(db);
                let name = feat.name.clone();
                if self.doc.character.has_grant(&name) {
                    self.report(format!("{name} already granted"), cx);
                    return;
                }
                self.doc.edit(|c| c.add_feat(feat));
                self.report(format!("added feat {name}"), cx);
            }
            PickKind::Race => {
                let Some(db) = corpus.races.iter().find(|r| r.index == index).cloned() else {
                    self.report("corpus entry vanished", cx);
                    return;
                };
                let race = blackletter_types::sheet::race::Race::try_from_database(db);
                let name = race.name.clone();
                self.doc.edit(|c| c.race = Some(race));
                self.report(format!("race set to {name}"), cx);
            }
            PickKind::Trait => {
                let Some(db) = corpus.traits.iter().find(|t| t.index == index).cloned() else {
                    self.report("corpus entry vanished", cx);
                    return;
                };
                let tr = blackletter_types::sheet::traits::Trait {
                    name: db.name.clone(),
                    desc: db.desc.join("\n\n"),
                    feature: None,
                    source: Some(srd()),
                };
                let name = tr.name.clone();
                if self.doc.character.has_grant(&name) {
                    self.report(format!("{name} already granted"), cx);
                    return;
                }
                self.doc.edit(|c| c.add_trait(tr));
                self.report(format!("added trait {name}"), cx);
            }
            PickKind::Class | PickKind::Background | PickKind::Subrace => {
                // Wizard-only kinds; the sheet never opens these pickers.
                self.report("that picker belongs to the creation wizard", cx);
            }
            PickKind::Feature => {
                let Some(db) = corpus.feature(index).cloned() else {
                    self.report("corpus entry vanished", cx);
                    return;
                };
                let feat = blackletter_types::feature::Feature {
                    name: db.name.clone(),
                    source: Some(srd()),
                    effects: vec![],
                };
                let name = feat.name.clone();
                if self.doc.character.has_grant(&name) {
                    self.report(format!("{name} already granted"), cx);
                    return;
                }
                self.doc.edit(|c| c.add_feature(feat));
                self.report(format!("added feature {name}"), cx);
            }
        }
    }

    /// A library pick needs no conversion — entries are already sheet
    /// types — only the same duplicate checks and grant bookkeeping.
    fn apply_custom_pick(&mut self, kind: PickKind, index: &str, cx: &mut Context<Self>) {
        let Some(name) = Library::custom_name(index).map(str::to_owned) else {
            return;
        };
        let lib = self.library.read(cx).clone();
        match kind {
            PickKind::Spell => {
                let Some(sp) = lib.spells.get(&name).cloned() else {
                    self.report("library entry vanished", cx);
                    return;
                };
                let outcome = self.doc.try_edit(|c| {
                    let name = sp.name.clone();
                    if c.spells.spells.insert(name.clone(), sp).is_some() {
                        anyhow::bail!("{name} is already in the spellbook");
                    }
                    Ok(name)
                });
                match outcome {
                    Ok(name) => self.report(format!("added {name} to the spellbook"), cx),
                    Err(e) => self.report(format!("could not add spell: {e}"), cx),
                }
            }
            PickKind::Item => {
                let Some(item) = lib.items.get(&name).cloned() else {
                    self.report("library entry vanished", cx);
                    return;
                };
                let outcome = self.doc.try_edit(|c| {
                    let name = item.name.clone();
                    if c.inventory.0.insert(name.clone(), item).is_some() {
                        anyhow::bail!("{name} is already in the inventory");
                    }
                    Ok(name)
                });
                match outcome {
                    Ok(name) => self.report(format!("added {name} to the inventory"), cx),
                    Err(e) => self.report(format!("could not add item: {e}"), cx),
                }
            }
            PickKind::Feat => {
                let Some(feat) = lib.feats.get(&name).cloned() else {
                    self.report("library entry vanished", cx);
                    return;
                };
                let name = feat.name.clone();
                if self.doc.character.has_grant(&name) {
                    self.report(format!("{name} already granted"), cx);
                    return;
                }
                self.doc.edit(|c| c.add_feat(feat));
                self.report(format!("added feat {name}"), cx);
            }
            PickKind::Race => {
                let Some(race) = lib.races.get(&name).cloned() else {
                    self.report("library entry vanished", cx);
                    return;
                };
                let name = race.name.clone();
                self.doc.edit(|c| c.race = Some(race));
                self.report(format!("race set to {name}"), cx);
            }
            PickKind::Trait => {
                let Some(tr) = lib.traits.get(&name).cloned() else {
                    self.report("library entry vanished", cx);
                    return;
                };
                let name = tr.name.clone();
                if self.doc.character.has_grant(&name) {
                    self.report(format!("{name} already granted"), cx);
                    return;
                }
                self.doc.edit(|c| c.add_trait(tr));
                self.report(format!("added trait {name}"), cx);
            }
            PickKind::Class | PickKind::Background | PickKind::Subrace => (),
            PickKind::Feature => {
                let Some(feat) = lib.features.get(&name).cloned() else {
                    self.report("library entry vanished", cx);
                    return;
                };
                let name = feat.name.clone();
                if self.doc.character.has_grant(&name) {
                    self.report(format!("{name} already granted"), cx);
                    return;
                }
                self.doc.edit(|c| c.add_feature(feat));
                self.report(format!("added feature {name}"), cx);
            }
        }
    }

    fn on_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.undo() {
            self.report("undo", cx);
        }
        cx.notify();
    }

    fn on_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.redo() {
            self.report("redo", cx);
        }
        cx.notify();
    }

    fn on_save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        self.save(cx);
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        match self.characters_dir.clone() {
            Some(dir) => match self.doc.save_to_dir(&dir) {
                Ok(Some(p)) => self.report(
                    format!(
                        "saved {}",
                        p.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    cx,
                ),
                Ok(None) => self.report("nothing to save", cx),
                Err(e) => self.report(format!("save failed: {e}"), cx),
            },
            None => self.report("no campaign open (ctrl-O)", cx),
        }
    }

    /// Autosave hook: write if dirty and a campaign dir is known.
    pub fn save_if_dirty(&mut self) -> bool {
        if !self.doc.dirty {
            return false;
        }
        let Some(dir) = self.characters_dir.clone() else {
            return false;
        };
        self.doc.save_to_dir(&dir).is_ok_and(|p| p.is_some())
    }

    // ---- rendering ----

    fn nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.section;
        div()
            .w(gpui::px(150.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("SECTIONS"),
            )
            .children(Section::ALL.iter().map(|s| {
                div()
                    .id(s.label())
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .when(*s == active, |d| d.bg(gpui::rgb(0x45_47_66)))
                    .text_color(gpui::rgb(if *s == active {
                        0xcd_d6_f4
                    } else {
                        0x9c_af_c9
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.section = *s;
                        this.sel = 0;
                        cx.notify();
                    }))
                    .child(s.label())
            }))
            .child(
                div()
                    .mt_2()
                    .text_xs()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("[]: sections\nj/k: class  +/-: level\nenter: name  /: SRD picker\nctrl-s: save"),
            )
    }

    fn summary(&self) -> AnyElement {
        let c = &self.doc.character;
        let senses = c.passive_senses();
        let col = |label: &'static str, value: String| {
            div()
                .flex()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .text_color(gpui::rgb(0x9c_af_c9))
                        .child(label),
                )
                .child(div().text_sm().child(value))
        };
        div()
            .w(gpui::px(220.))
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_md()
            .bg(gpui::rgb(0x2a_2a_3a))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("SUMMARY"),
            )
            .child(col("AC", c.armor_class_total().to_string()))
            .child(col(
                "HP",
                format!(
                    "{} / {}{}",
                    c.health.current,
                    c.max_health_total(),
                    if c.health.bonus > 0 {
                        format!(" +{}", c.health.bonus)
                    } else {
                        String::new()
                    }
                ),
            ))
            .child(col("Speed", format!("{} ft", c.speed_total())))
            .child(col("PB", format!("+{}", c.skills.proficiency_bonus)))
            .child(col("Passive Perception", senses.perception.to_string()))
            .child(col("Exhaustion", c.exhaustion.to_string()))
            .child(match &c.concentrating {
                Some(k) => div()
                    .text_sm()
                    .text_color(gpui::rgb(0xe0_af_68))
                    .child(format!("Concentrating: {}", k.spell_name)),
                None => div().text_sm().text_color(gpui::rgb(0x6c_70_86)).child("—"),
            })
            .when(self.doc.character.needs_spell_prep, |d| {
                d.child(
                    div()
                        .text_sm()
                        .text_color(gpui::rgb(0xf3_8b_a8))
                        .child("Prepare spells (long rest)"),
                )
            })
            .into_any_element()
    }

    fn center(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            Section::Basic => self.basic_section(cx),
            Section::Abilities => self.abilities_section(cx),
            Section::Skills => self.skills_section(cx),
            Section::Combat => self.combat_section(cx),
            Section::Spells => self.spells_section(cx),
            Section::Conditions => self.conditions_section(cx),
            Section::Inventory => self.inventory_section(cx),
            Section::Features => self.features_section(cx),
            Section::Notes => self.notes_section(cx),
        }
    }

    /// Turn a section's root div into the center column's scroll viewport:
    /// it takes the space between the field bar and the tray, may shrink
    /// below its content height, and scrolls (wheel + scroll_to_item).
    /// The scroll modifiers live on `Stateful<Div>` only, hence the id.
    /// The handle tracks THIS div's direct children, so `scroll_child`
    /// must index them, not the logical row numbers.
    fn scrollable(&self, d: gpui::Div) -> gpui::Stateful<gpui::Div> {
        d.flex_1()
            .min_h_0()
            .id("section-scroll")
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
    }

    /// Child index (within the section root's direct children) of the
    /// selected row, for ScrollHandle::scroll_to_item.
    fn scroll_child(&self) -> usize {
        match self.section {
            // name, Race, Background, Level, XP, CLASSES header, then class rows.
            Section::Basic => 6 + self.class_sel,
            // rows are the first children.
            Section::Abilities | Section::Features => self.sel,
            // 18 skills, saves header, 6 saves, profs header, 6 profs.
            Section::Skills => match self.sel {
                s if s < 18 => s,
                s if s < 24 => s + 1,
                s => s + 2,
            },
            // every other section leads with a header line.
            Section::Combat | Section::Spells | Section::Conditions | Section::Inventory => {
                self.sel + 1
            }
            Section::Notes => 0,
        }
    }

    /// Keep the selected row inside the scroll viewport after the
    /// selection moved (j/k, section switches).
    fn scroll_sel_into_view(&self) {
        self.scroll.scroll_to_item(self.scroll_child());
    }

    fn basic_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.doc.character;
        let row = |label: &'static str, value: String| {
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .w(gpui::px(90.))
                        .text_sm()
                        .text_color(gpui::rgb(0x9c_af_c9))
                        .child(label),
                )
                .child(div().text_sm().child(value))
        };
        let classes: Vec<_> = c
            .classes
            .classes
            .iter()
            .enumerate()
            .map(|(i, cl)| {
                let selected = i == self.class_sel;
                div()
                    .id(("class", i))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .when(selected, |d| d.bg(gpui::rgb(0x35_35_48)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.class_sel = i;
                        cx.notify();
                    }))
                    .child(format!(
                        "{} — level {} ({:?} hit die)",
                        cl.class, cl.level.0, cl.hit_dice
                    ))
            })
            .collect();
        self.scrollable(div())
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(div().text_lg().child(c.name.clone()))
            // No FieldElement here: the root center column renders the one
            // shared field for every section (a second copy double-painted
            // it in Basic).
            .child(row(
                "Race",
                c.race.as_ref().map_or("—".into(), |r| r.name.clone()),
            ))
            .child(row(
                "Background",
                c.background.as_ref().map_or("—".into(), |b| b.name.clone()),
            ))
            .child(row("Level", c.classes.total_level().to_string()))
            .child(row("XP", {
                let mut xp = c.xp.to_string();
                if c.xp_ready_to_level() {
                    xp.push_str("  (enough for another level — your call)");
                } else if let Some(t) = c.xp_next_threshold() {
                    xp.push_str(&format!("  (next at {t})"));
                }
                xp
            }))
            .child(
                div()
                    .mt_1()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("CLASSES (j/k select, +/- level)"),
            )
            .children(classes)
            .into_any_element()
    }

    fn row(&self, i: usize, text: String, cx: &mut Context<Self>) -> AnyElement {
        let selected = i == self.sel;
        div()
            .id(("row", i))
            .px_2()
            .py_1()
            .rounded_md()
            .when(selected, |d| d.bg(gpui::rgb(0x35_35_48)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.sel = i;
                cx.notify();
            }))
            .child(text)
            .into_any_element()
    }

    fn abilities_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.doc.character;
        self.scrollable(div())
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .children(
                SCORE_ROWS
                    .iter()
                    .enumerate()
                    .map(|(i, score)| {
                        let base = c.ability_scores.get(score).base;
                        self.row(
                            i,
                            format!(
                                "{:<14} {:>3}  ({:+})",
                                format!("{score}"),
                                base,
                                c.ability_modifier_for(*score)
                            ),
                            cx,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("j/k select  +/- adjust score (3-30)"),
            )
            .into_any_element()
    }

    fn skills_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.doc.character;
        let skills: Vec<_> = Skill::ALL
            .iter()
            .enumerate()
            .map(|(i, skill)| {
                let prof = c.skills.proficiencies.contains(*skill);
                let total = c.skills.check(skill, &c.ability_scores);
                self.row(
                    i,
                    format!(
                        "{} {:<16} {:>+4}  ({})",
                        if prof { "*" } else { " " },
                        skill.to_string(),
                        total,
                        c.skills.score_for(skill),
                    ),
                    cx,
                )
            })
            .collect();
        let saves: Vec<_> = SCORE_ROWS
            .iter()
            .enumerate()
            .map(|(i, score)| {
                let on = match score {
                    Score::Str => c.saving_throws.str,
                    Score::Dex => c.saving_throws.dex,
                    Score::Con => c.saving_throws.con,
                    Score::Int => c.saving_throws.int,
                    Score::Wis => c.saving_throws.wis,
                    Score::Cha => c.saving_throws.cha,
                };
                self.row(
                    18 + i,
                    format!(
                        "{} {:<16} save",
                        if on { "*" } else { " " },
                        format!("{score}")
                    ),
                    cx,
                )
            })
            .collect();
        let profs: Vec<_> = PROF_LABELS
            .iter()
            .enumerate()
            .map(|(i, (label, kind))| {
                let on = match kind {
                    ProfKind::Armor(a) => c.proficiencies.armor.contains(*a),
                    ProfKind::Weapon(w) => c.proficiencies.weapons.contains(*w),
                };
                self.row(
                    24 + i,
                    format!("{} {:<16}", if on { "*" } else { " " }, label),
                    cx,
                )
            })
            .collect();
        self.scrollable(div())
            .flex()
            .flex_col()
            .gap_0()
            .p_2()
            .children(skills)
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("SAVING THROWS"),
            )
            .children(saves)
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("PROFICIENCIES"),
            )
            .children(profs)
            .child(
                div()
                    .mt_1()
                    .text_sm()
                    .text_color(gpui::rgb(0x9c_af_c9))
                    .child(if c.proficiencies.specific.is_empty() {
                        "specific: —".to_string()
                    } else {
                        format!(
                            "specific: {}",
                            c.proficiencies
                                .specific
                                .iter()
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }),
            )
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("j/k select  space toggles proficiency (* = proficient)"),
            )
            .into_any_element()
    }

    /// Slot meters (classic + pact), sorted by level then pact-last.
    fn slot_rows(&self) -> Vec<(String, i64, bool)> {
        let mut out: Vec<(String, i64, bool)> = self
            .doc
            .character
            .meters
            .meters
            .keys()
            .filter_map(|name| {
                let (pact, rest) = match name.strip_prefix("pact_slot_") {
                    Some(l) => (true, l),
                    None => (false, name.strip_prefix("spell_slot_")?),
                };
                Some((name.clone(), rest.parse().unwrap_or(0), pact))
            })
            .collect();
        out.sort_by_key(|(_, l, pact)| (*l, *pact));
        out
    }

    /// Spell names sorted by (level, name) — the row order after the slots.
    fn spell_rows(&self) -> Vec<(String, i64)> {
        let mut out: Vec<(String, i64)> = self
            .doc
            .character
            .spells
            .spells
            .iter()
            .map(|(name, s)| (name.clone(), s.level))
            .collect();
        out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        out
    }

    fn toggle_spell_row(&mut self, cx: &mut Context<Self>) {
        let slots = self.slot_rows();
        if self.sel < slots.len() {
            let (name, level, pact) = slots[self.sel].clone();
            self.doc.edit(|c| {
                if let Some(m) = c.meters.meters.get_mut(&name) {
                    if m.spent < m.slot_number {
                        m.spend();
                    } else {
                        m.spent = 0;
                    }
                }
            });
            let m = &self.doc.character.meters.meters[&name];
            self.report(
                format!(
                    "{} L{} slot {} ({}/{})",
                    if pact { "pact" } else { "spell" },
                    level,
                    if m.spent > 0 { "spent" } else { "restored" },
                    m.slot_number - m.spent,
                    m.slot_number,
                ),
                cx,
            );
            return;
        }
        let spells = self.spell_rows();
        let idx = self.sel - slots.len();
        if idx >= spells.len() {
            return;
        }
        let (name, _) = spells[idx].clone();
        let prepared_caster = matches!(
            self.doc.character.casting_mechanic(),
            Some(CastingMechanic::Prepared) | Some(CastingMechanic::PreparedWithSpellbook)
        );
        if !prepared_caster {
            self.report("spontaneous caster: no prepared toggles", cx);
            return;
        }
        self.doc.edit(|c| {
            if let Some(s) = c.spells.spells.get_mut(&name) {
                s.prepared = !s.prepared;
            }
        });
        let prep = self.doc.character.spells.spells[&name].prepared;
        self.report(
            format!("{name} {}", if prep { "prepared" } else { "unprepared" }),
            cx,
        );
    }

    /// `t` in Spells: spend a slot, start concentration if the spell needs it.
    fn cast_spell(&mut self, cx: &mut Context<Self>) {
        let slots_n = self.slot_rows().len();
        let spells = self.spell_rows();
        let Some(idx) = self.sel.checked_sub(slots_n) else {
            return;
        };
        if idx >= spells.len() {
            return;
        }
        let (name, level) = spells[idx].clone();
        let (conc, dc) = {
            let s = &self.doc.character.spells.spells[&name];
            (s.concentration, self.doc.character.save_dc())
        };
        let cast = self.doc.try_edit(|c| {
            if level > 0 && !c.spend_spell_slot(level) {
                return Err(());
            }
            if conc {
                c.start_concentration(&name, dc);
            }
            Ok(())
        });
        if cast.is_ok() {
            self.report(
                format!(
                    "cast {name}{}",
                    if conc {
                        format!(" — concentrating (DC {dc})")
                    } else {
                        String::new()
                    }
                ),
                cx,
            );
        } else {
            self.report(format!("no L{level} slots left for {name}"), cx);
        }
    }

    fn spells_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.doc.character;
        let prepared_caster = matches!(
            c.casting_mechanic(),
            Some(CastingMechanic::Prepared) | Some(CastingMechanic::PreparedWithSpellbook)
        );
        let slots = self.slot_rows();
        let mut rows: Vec<_> = slots
            .iter()
            .enumerate()
            .map(|(i, (name, level, pact))| {
                let m = &c.meters.meters[name];
                let pips: String = "\u{25cf}"
                    .repeat((m.slot_number - m.spent) as usize)
                    .chars()
                    .chain("\u{25cb}".repeat(m.spent as usize).chars())
                    .collect();
                self.row(
                    i,
                    format!(
                        "L{level} {} slots  {pips}  ({} left)",
                        if *pact { "pact" } else { "classic" },
                        m.slot_number - m.spent
                    ),
                    cx,
                )
            })
            .collect();
        let nslots = slots.len();
        rows.extend(
            self.spell_rows()
                .iter()
                .enumerate()
                .map(|(j, (name, level))| {
                    let s = &c.spells.spells[name];
                    let mark = if *level == 0 {
                        "c"
                    } else if prepared_caster {
                        if s.prepared { "P" } else { "-" }
                    } else {
                        "k"
                    };
                    self.row(
                        nslots + j,
                        format!(
                            "[{mark}] {name}  L{level}{}{}",
                            if s.concentration { "  conc" } else { "" },
                            if s.ritual { "  ritual" } else { "" },
                        ),
                        cx,
                    )
                }),
        );
        self.scrollable(div())
            .flex()
            .flex_col()
            .gap_0()
            .p_2()
            .child(div().text_sm().child(format!(
                "casting: {:?}   save DC {}   [P] prepared [k] known [c] cantrip",
                c.casting_mechanic(),
                c.save_dc()
            )))
            .children(rows)
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child(
                        "j/k select  space spend-slot/toggle-prepared  t cast\n/: add spell (SRD)",
                    ),
            )
            .into_any_element()
    }

    /// The fifteen SRD conditions (PHB appendix A), in a fixed picker order.
    const KIND_ROWS: [ConditionKind; 15] = [
        ConditionKind::Blinded,
        ConditionKind::Charmed,
        ConditionKind::Deafened,
        ConditionKind::Exhaustion,
        ConditionKind::Frightened,
        ConditionKind::Grappled,
        ConditionKind::Incapacitated,
        ConditionKind::Invisible,
        ConditionKind::Paralyzed,
        ConditionKind::Petrified,
        ConditionKind::Poisoned,
        ConditionKind::Prone,
        ConditionKind::Restrained,
        ConditionKind::Stunned,
        ConditionKind::Unconscious,
    ];

    fn custom_count(&self) -> usize {
        self.doc
            .character
            .conditions
            .conditions
            .iter()
            .filter(|c| matches!(c.kind, ConditionKind::Custom(_)))
            .count()
    }

    fn condition_rows(&self) -> usize {
        self.custom_count() + Self::KIND_ROWS.len()
    }

    fn toggle_condition_row(&mut self, cx: &mut Context<Self>) {
        let customs = self.custom_count();
        if self.sel < customs {
            let victim = self
                .doc
                .character
                .conditions
                .conditions
                .iter()
                .filter(|c| matches!(c.kind, ConditionKind::Custom(_)))
                .nth(self.sel)
                .map(|c| c.kind.to_string())
                .unwrap_or_default();
            let mut i = 0;
            self.doc.edit(|c| {
                c.conditions.conditions.retain(|cond| {
                    if matches!(cond.kind, ConditionKind::Custom(_)) {
                        i += 1;
                        i - 1 != self.sel
                    } else {
                        true
                    }
                });
            });
            self.report(format!("removed {victim}"), cx);
            self.sel = self.sel.min(self.condition_rows().saturating_sub(1));
            return;
        }
        let kind = Self::KIND_ROWS[self.sel - customs].clone();
        let active = self
            .doc
            .character
            .conditions
            .conditions
            .iter()
            .any(|c| c.kind == kind);
        if active {
            self.doc.edit(|c| {
                c.conditions.conditions.retain(|cond| cond.kind != kind);
            });
            self.report(format!("{kind} ended"), cx);
        } else {
            let added = self.doc.edit_return(|c| {
                c.add_condition(Condition {
                    kind: kind.clone(),
                    desc: String::new(),
                    duration: -1,
                })
            });
            self.report(
                if added {
                    format!("{kind} applied (until removed)")
                } else {
                    format!("{kind}: immune")
                },
                cx,
            );
        }
    }

    fn adjust_condition_duration(&mut self, delta: i64, cx: &mut Context<Self>) {
        let customs = self.custom_count();
        let is_custom = self.sel < customs;
        let idx_target = if is_custom {
            self.doc
                .character
                .conditions
                .conditions
                .iter()
                .enumerate()
                .filter(|(_, c)| matches!(c.kind, ConditionKind::Custom(_)))
                .nth(self.sel)
                .map(|(i, _)| i)
        } else {
            let kind = Self::KIND_ROWS[self.sel - customs].clone();
            self.doc
                .character
                .conditions
                .conditions
                .iter()
                .position(|c| c.kind == kind)
        };
        let Some(i) = idx_target else { return };
        self.doc.edit(move |c| {
            let cond = &mut c.conditions.conditions[i];
            if delta > 0 {
                // +/- on an indefinite (-1) condition starts a round timer.
                cond.duration = if cond.duration < 0 {
                    delta as i32
                } else {
                    cond.duration + delta as i32
                };
            } else {
                cond.duration = (cond.duration + delta as i32).max(0);
            }
        });
        let dur = self.doc.character.conditions.conditions[i].duration;
        self.report(
            format!(
                "{} duration {dur}",
                self.doc.character.conditions.conditions[i].kind
            ),
            cx,
        );
    }

    fn conditions_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.doc.character;
        let mut rows: Vec<_> = c
            .conditions
            .conditions
            .iter()
            .filter(|cond| matches!(cond.kind, ConditionKind::Custom(_)))
            .enumerate()
            .map(|(i, cond)| {
                self.row(
                    i,
                    format!("* {:<16}  duration {}", cond.kind, cond.duration),
                    cx,
                )
            })
            .collect();
        let customs = rows.len();
        rows.extend(Self::KIND_ROWS.iter().enumerate().map(|(j, kind)| {
            let active = c
                .conditions
                .conditions
                .iter()
                .any(|cond| &cond.kind == kind);
            let detail = if active {
                let dur = c
                    .conditions
                    .conditions
                    .iter()
                    .find(|cond| &cond.kind == kind)
                    .map_or(0, |cond| cond.duration);
                format!("  (active, duration {dur})")
            } else {
                String::new()
            };
            self.row(
                customs + j,
                format!("{} {kind:<16}{detail}", if active { "*" } else { " " },),
                cx,
            )
        }));
        self.scrollable(div())
            .flex()
            .flex_col()
            .gap_0()
            .p_2()
            .child(
                div()
                    .text_sm()
                    .child("space add/remove   +/- duration   t tick all durations"),
            )
            .children(rows)
            .into_any_element()
    }

    /// Inventory rows: the character's items, sorted by name.
    fn item_rows(&self) -> Vec<(String, i32, bool)> {
        self.doc
            .character
            .inventory
            .0
            .iter()
            .map(|(name, it)| (name.clone(), it.quantity, it.equipped))
            .collect()
    }

    fn inventory_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<_> = self
            .item_rows()
            .into_iter()
            .enumerate()
            .map(|(i, (name, qty, equipped))| {
                let mark = if equipped { "[e]" } else { "[ ]" };
                let qty = if qty == 1 {
                    String::new()
                } else {
                    format!(" x{qty}")
                };
                self.row(i, format!("{mark} {name}{qty}"), cx)
            })
            .collect();
        self.scrollable(div())
            .flex()
            .flex_col()
            .gap_0()
            .p_2()
            .child(div().text_sm().child(format!(
                "gold {}   items {}",
                self.doc.character.gold,
                self.item_rows().len()
            )))
            .children(rows)
            .child(self.row(self.item_rows().len(), "+ add item (space)".into(), cx))
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child(
                        "j/k select  space equip/unequip  +/- quantity\n(- to 0 removes the item)   /: SRD item picker",
                    ),
            )
            .into_any_element()
    }

    fn toggle_item_row(&mut self, cx: &mut Context<Self>) {
        if self.sel == self.doc.character.inventory.0.len() {
            self.field = Some((FieldTarget::NewItem, Field::new("")));
            self.report("new item name — enter adds it", cx);
            return;
        }
        let Some((name, _, equipped)) = self.item_rows().get(self.sel).cloned() else {
            return;
        };
        let ok = self.doc.try_edit(|c| {
            if equipped {
                Ok::<_, ()>(c.unequip(&name))
            } else {
                Ok(c.equip(&name))
            }
        });
        match ok {
            Ok(true) => self.report(
                format!(
                    "{name} {}",
                    if equipped { "unequipped" } else { "equipped" }
                ),
                cx,
            ),
            _ => self.report("nothing changed", cx),
        }
    }

    /// Feature/feat/trait rows: name + effect summary.
    fn grant_rows(&self) -> Vec<(String, String)> {
        use std::fmt::Write as _;
        let c = &self.doc.character;
        let mut out = Vec::new();
        for f in c
            .features
            .inner
            .iter()
            .map(|f| (&f.name, &f.effects[..]))
            .chain(c.feats.iter().map(|f| {
                (
                    &f.name,
                    f.feature.as_ref().map_or(&[][..], |x| &x.effects[..]),
                )
            }))
            .chain(c.traits.iter().map(|t| {
                (
                    &t.name,
                    t.feature.as_ref().map_or(&[][..], |x| &x.effects[..]),
                )
            }))
        {
            let mut sum = String::new();
            for e in f.1.iter().take(4) {
                if !sum.is_empty() {
                    sum.push_str("; ");
                }
                let _ = write!(sum, "{e}");
            }
            if f.1.len() > 4 {
                let _ = write!(sum, " (+{})", f.1.len() - 4);
            }
            out.push((f.0.clone(), sum));
        }
        out
    }

    fn features_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let grants = self.grant_rows().len();
        let rows: Vec<_> = self
            .grant_rows()
            .into_iter()
            .enumerate()
            .map(|(i, (name, effects))| {
                let text = if effects.is_empty() {
                    name
                } else {
                    format!("{name} — {effects}")
                };
                self.row(i, text, cx)
            })
            .collect();
        self.scrollable(div())
            .flex()
            .flex_col()
            .gap_0()
            .p_2()
            .children(rows)
            .child(self.row(grants, "+ add feat (SRD picker)".into(), cx))
            .child(self.row(grants + 1, "+ add feature (SRD picker)".into(), cx))
            .child(self.row(grants + 2, "+ add trait (SRD picker)".into(), cx))
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child(
                        "j/k select  space removes the grant (ctrl-z undoes)\nadd rows: space opens the SRD picker",
                    ),
            )
            .into_any_element()
    }

    fn grant_activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let grants = self.grant_rows().len();
        match self.sel {
            s if s == grants => self.open_picker(PickKind::Feat, window, cx),
            s if s == grants + 1 => self.open_picker(PickKind::Feature, window, cx),
            s if s == grants + 2 => self.open_picker(PickKind::Trait, window, cx),
            _ => self.remove_grant_row(cx),
        }
    }

    fn remove_grant_row(&mut self, cx: &mut Context<Self>) {
        let Some((name, _)) = self.grant_rows().get(self.sel).cloned() else {
            return;
        };
        let removed = self.doc.try_edit(|c| Ok::<_, ()>(c.remove_grant(&name)));
        self.report(
            match removed {
                Ok(true) => format!("{name} removed (ctrl-z to undo)"),
                _ => "nothing to remove".to_string(),
            },
            cx,
        );
    }

    fn notes_section(&self, _cx: &mut Context<Self>) -> AnyElement {
        let lore = &self.doc.character.lore;
        let blocks = [
            ("Backstory", &lore.backstory),
            ("Personality", &lore.personality_traits),
            ("Physical", &lore.physical_traits),
            ("Allies", &lore.allies),
            ("Enemies", &lore.enemies),
        ];
        let mut out = Vec::new();
        for (label, text) in blocks {
            if text.trim().is_empty() {
                continue;
            }
            out.push(
                div()
                    .mb_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(0x8a_90_b0))
                            .child(label.to_uppercase()),
                    )
                    .children(text.lines().map(|l| {
                        div()
                            .text_sm()
                            .text_ellipsis()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(l.to_string())
                    })),
            );
        }
        if out.is_empty() {
            out.push(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("no lore recorded"),
            );
        }
        self.scrollable(div())
            .flex()
            .flex_col()
            .p_2()
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("lore (read-only here — edit via the vault or a future editor)"),
            )
            .children(out)
            .into_any_element()
    }

    /// Bottom dice-tray strip: recent roll results + key hints.
    fn tray(&self, cx: &mut Context<Self>) -> AnyElement {
        let _ = cx;
        let lines: Vec<_> = self
            .roll_log
            .iter()
            .map(|l| {
                div()
                    .text_xs()
                    .text_color(gpui::rgb(0xba_b2_c8))
                    .child(l.clone())
            })
            .collect();
        div()
            .border_t_1()
            .border_color(gpui::rgb(0x31_32_44))
            .px_2()
            .py_1()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("a attack   v save   c check   x damage   (d = apply damage to HP)"),
            )
            .children(lines)
            .into_any_element()
    }

    fn combat_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.doc.character;
        let rows: Vec<_> = COMBAT_LABELS
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let detail = match i {
                    0 => format!("  [type: {:?}]", self.damage_type),
                    2 => format!("  (temp {})", c.health.bonus),
                    3 => match c.armor_class.flat {
                        Some(v) => format!("  (pinned at {v})"),
                        None => "  (derived)".to_string(),
                    },
                    4 => format!("  (gold {})", c.gold),
                    5 => format!("  (level {})", c.exhaustion),
                    8 => {
                        let rem: i64 = c
                            .classes
                            .classes
                            .iter()
                            .map(|k| k.healing_die_remaining)
                            .sum();
                        let tot: i64 = c.classes.classes.iter().map(|k| k.max_healing_die).sum();
                        let die = c
                            .classes
                            .classes
                            .first()
                            .map_or(String::new(), |k| format!(" d{}", k.hit_dice.max()));
                        format!("  ({rem}/{tot}{die})")
                    }
                    9 => format!(
                        "  ({}S/{}F{})",
                        c.death_saves.successes,
                        c.death_saves.failures,
                        if c.death_saves.stable { ", stable" } else { "" }
                    ),
                    _ => String::new(),
                };
                self.row(i, format!("{label}{detail}"), cx)
            })
            .collect();
        let ds = &c.death_saves;
        self.scrollable(div())
            .flex()
            .flex_col()
            .gap_0()
            .p_2()
            .child(
                div()
                    .text_sm()
                    .child(format!(
                        "HP {} / {}   death saves: {}S / {}F{}",
                        c.health.current,
                        c.max_health_total(),
                        ds.successes,
                        ds.failures,
                        if ds.stable { "  STABLE" } else { "" }
                    )),
            )
            .children(rows)
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("j/k select  space activates  d damage  t cycle type\nr death save  s/S rest"),
            )
            .into_any_element()
    }
}

impl FieldOwner for SheetView {
    fn field(&self) -> &Field {
        self.field.as_ref().map_or(&self.blank, |(_, f)| f)
    }

    fn field_mut(&mut self) -> &mut Field {
        match &mut self.field {
            Some((_, f)) => f,
            None => &mut self.blank,
        }
    }

    fn family(&self) -> SharedString {
        "monospace".into()
    }

    fn size(&self) -> f32 {
        12.0
    }

    fn focus(&self) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl gpui::EntityInputHandler for SheetView {
    fn text_for_range(
        &mut self,
        range_utf16: std::ops::Range<usize>,
        actual_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let (_, f) = self.field.as_ref()?;
        let (text, used) = f.text_in_utf16(range_utf16);
        actual_range.replace(used);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        Some(self.field.as_ref()?.1.caret_selection())
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<std::ops::Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, f)) = self.field.as_mut() {
            f.replace_utf16(range_utf16, new_text);
            cx.notify();
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range(range_utf16, new_text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        None
    }
}

impl Focusable for SheetView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SheetView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let field_active = self.field.is_some();
        let picker_open = self.picker.is_some();
        let form_open = self.form.is_some();
        div()
            .track_focus(&self.focus_handle)
            // While the picker is open its context replaces SHEET_CTX at the
            // root too: printable keys bound in SHEET_CTX (`a`, `d`, `r`)
            // would otherwise steal keystrokes from the picker's field.
            .key_context(if picker_open {
                crate::picker::PICKER_CTX
            } else if form_open {
                crate::form::FORM_CTX
            } else if field_active {
                crate::dungeon_view::FIELD_CTX
            } else {
                SHEET_CTX
            })
            .size_full()
            .flex()
            .gap_2()
            .p_2()
            .on_action(cx.listener(Self::on_next_section))
            .on_action(cx.listener(Self::on_prev_section))
            .on_action(cx.listener(Self::on_select_class_up))
            .on_action(cx.listener(Self::on_select_class_down))
            .on_action(cx.listener(Self::on_level_up))
            .on_action(cx.listener(Self::on_level_down))
            .on_action(cx.listener(Self::on_add_class))
            .on_action(cx.listener(Self::on_edit_name))
            .on_action(cx.listener(Self::on_field_submit))
            .on_action(cx.listener(Self::on_field_cancel))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_toggle_row))
            .on_action(cx.listener(Self::on_quick_damage))
            .on_action(cx.listener(Self::on_cycle_damage_type))
            .on_action(cx.listener(Self::on_roll_death_save))
            .on_action(cx.listener(Self::on_short_rest))
            .on_action(cx.listener(Self::on_long_rest))
            .on_action(cx.listener(Self::on_tray_attack))
            .on_action(cx.listener(Self::on_tray_save))
            .on_action(cx.listener(Self::on_tray_check))
            .on_action(cx.listener(Self::on_tray_damage))
            .on_action(cx.listener(Self::on_open_picker))
            .on_action(cx.listener(Self::on_new_custom))
            .child(self.nav(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    // Stretch to the root's height: gpui flex defaults to
                    // start-alignment on the cross axis, so without this the
                    // column grows to its content and the section's
                    // overflow_y_scroll viewport never engages.
                    .h_full()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .when_some(self.field.as_ref(), |d, _| {
                        d.child(
                            div()
                                .mb_1()
                                .w(gpui::px(320.))
                                .child(FieldElement::new(cx.entity())),
                        )
                    })
                    .child(self.center(cx))
                    .child(self.tray(cx)),
            )
            .child(self.summary())
            .when(field_active, |this| {
                let this = crate::field_key_actions!(this, cx, after_sheet_field_change);
                this.on_action(
                    cx.listener(|this, _: &crate::field::FieldBackspace, _, cx| {
                        if let Some((_, f)) = this.field.as_mut() {
                            f.backspace();
                            cx.notify();
                        }
                    }),
                )
                .on_action(cx.listener(|this, _: &crate::field::FieldDelete, _, cx| {
                    if let Some((_, f)) = this.field.as_mut() {
                        f.delete_forward();
                        cx.notify();
                    }
                }))
                .on_action(
                    cx.listener(|this, _: &crate::field::FieldDeleteWordBack, _, cx| {
                        if let Some((_, f)) = this.field.as_mut() {
                            f.delete_word_back();
                            cx.notify();
                        }
                    }),
                )
                .on_action(cx.listener(|this, _: &crate::field::FieldLeft, _, cx| {
                    if let Some((_, f)) = this.field.as_mut() {
                        f.step_caret(-1);
                        cx.notify();
                    }
                }))
                .on_action(cx.listener(
                    |this, _: &crate::field::FieldRight, _, cx| {
                        if let Some((_, f)) = this.field.as_mut() {
                            f.step_caret(1);
                            cx.notify();
                        }
                    },
                ))
            })
            // The SRD picker paints last over a dim scrim, centered: the
            // scrim says "modal", the centering says what it belongs to.
            .when_some(self.picker.clone(), |d, p| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::hsla(0., 0., 0., 0.45))
                        .flex()
                        .flex_col()
                        .items_center()
                        .pt(gpui::px(56.))
                        .child(p),
                )
            })
            // The level-up overlay rides the same scrim treatment.
            .when_some(self.levelup.clone(), |d, l| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::hsla(0., 0., 0., 0.45))
                        .flex()
                        .flex_col()
                        .items_center()
                        .pt(gpui::px(56.))
                        .child(l),
                )
            })
            // The custom-entry form rides the same scrim treatment.
            .when_some(self.form.clone(), |d, f| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::hsla(0., 0., 0., 0.45))
                        .flex()
                        .flex_col()
                        .items_center()
                        .pt(gpui::px(56.))
                        .child(f),
                )
            })
    }
}

/// `$after` hook for `field_key_actions!`: cut/paste change the text, but
/// sheet fields only commit on submit, so there is nothing to recompute.
pub fn after_sheet_field_change(_v: &mut SheetView, _cx: &mut Context<SheetView>) {}

/// The rows each custom-entry kind asks for, with defaults. Kept flat and
/// text-first: parsing is forgiving (bad numbers fall back, unknown schools
/// default), because the form is a quick capture tool, not a rules validator.
fn form_specs(kind: PickKind) -> Vec<(&'static str, String)> {
    let spec = |pairs: &[(&'static str, &str)]| {
        pairs.iter().map(|(l, v)| (*l, (*v).to_string())).collect()
    };
    match kind {
        PickKind::Spell => spec(&[
            ("name", ""),
            ("level", "1"),
            ("school", "Evocation"),
            ("cast time", "1 action"),
            ("range", "30 ft."),
            ("duration", "Instantaneous"),
            ("description", ""),
        ]),
        PickKind::Item => spec(&[("name", ""), ("quantity", "1"), ("description", "")]),
        PickKind::Feat => spec(&[("name", ""), ("description", "")]),
        PickKind::Feature => spec(&[("name", ""), ("description", "")]),
        PickKind::Trait => spec(&[("name", ""), ("description", "")]),
        PickKind::Race | PickKind::Class | PickKind::Background | PickKind::Subrace => Vec::new(),
    }
}

/// Constructor used by the root view.
pub fn build(
    doc: SheetDoc,
    characters_dir: Option<PathBuf>,
    corpus: Option<Arc<Corpus>>,
    library: Entity<Library>,
    root: WeakEntity<RootView>,
    cx: &mut Context<RootView>,
) -> Entity<SheetView> {
    cx.new(|cx| SheetView::new(doc, characters_dir, corpus, library, root, cx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autosave::Autosave;
    use crate::session::Session;
    use gpui::{Entity, TestAppContext, WindowHandle};

    fn open_sheet(cx: &mut TestAppContext) -> (WindowHandle<RootView>, Entity<RootView>) {
        open_sheet_with(cx, None)
    }

    fn test_corpus() -> Arc<Corpus> {
        use blackletter_types::database::reference::Edition;
        // The corpus is vendored in-repo under `srd/`.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../srd/2014/en");
        Arc::new(Corpus::load(&dir, Edition::Y2014).expect("vendored corpus loads"))
    }

    fn open_sheet_with(
        cx: &mut TestAppContext,
        corpus: Option<Arc<Corpus>>,
    ) -> (WindowHandle<RootView>, Entity<RootView>) {
        open_sheet_sized(cx, corpus, None)
    }

    /// `window_size: Some(..)` opens a fixed-size window (layout-sensitive
    /// tests: the default maximized test window never overflows).
    fn open_sheet_sized(
        cx: &mut TestAppContext,
        corpus: Option<Arc<Corpus>>,
        window_size: Option<gpui::Size<gpui::Pixels>>,
    ) -> (WindowHandle<RootView>, Entity<RootView>) {
        cx.update(|cx| {
            cx.bind_keys(crate::root::keybindings());
            cx.bind_keys(keybindings());
            cx.bind_keys(crate::field::field_keys(crate::dungeon_view::FIELD_CTX));
            cx.bind_keys(crate::picker::picker_keys());
            cx.bind_keys(crate::form::form_keys());
            cx.bind_keys(crate::levelup_view::levelup_keys());
            Autosave::init(cx);
        });
        let window = match window_size {
            Some(size) => cx.open_window(size, |_, cx| {
                RootView::new(
                    cx,
                    Session::default(),
                    None,
                    corpus.clone(),
                    Library::default(),
                    None,
                )
            }),
            None => cx.add_window(|_, cx| {
                RootView::new(
                    cx,
                    Session::default(),
                    None,
                    corpus,
                    Library::default(),
                    None,
                )
            }),
        };
        window
            .update(cx, |view, window, cx| window.focus(view.focus_handle(), cx))
            .unwrap();
        // Sheet mode is the default mode; build the view explicitly.
        let root = window.root(cx).unwrap();
        let _ = window.update(cx, |view, window, cx| view.ensure_sheet(window, cx));
        (window, root)
    }

    fn sheet(cx: &TestAppContext, root: &Entity<RootView>) -> Entity<SheetView> {
        cx.read(|cx| root.read(cx).sheet_view().cloned())
            .expect("sheet view built")
    }

    #[gpui::test]
    fn level_keys_change_levels_and_derived_numbers(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        let pb0 = cx.read(|cx| view.read(cx).doc().character.skills.proficiency_bonus);
        cx.simulate_keystrokes(window.into(), "equal equal equal equal");
        cx.run_until_parked();
        let (level, pb) = cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            (c.classes.total_level(), c.skills.proficiency_bonus)
        });
        assert_eq!(level, 5);
        assert_eq!((pb0, pb), (2, 3), "PB derives from total level");
        // Undo walks the snapshots back one keystroke at a time.
        cx.simulate_keystrokes(window.into(), "ctrl-z ctrl-z ctrl-z ctrl-z");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).doc().character.classes.total_level()),
            1
        );
    }

    #[gpui::test]
    fn level_up_overlay_walks_a_fighter_from_1_to_2(cx: &mut TestAppContext) {
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        let hp0 = cx.read(|cx| view.read(cx).doc().character.health.max);
        cx.simulate_keystrokes(window.into(), "equal");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).levelup.is_some()),
            "the overlay opens on + in Basic"
        );
        // HP step: enter accepts the fixed gain; confirm: enter applies.
        cx.simulate_keystrokes(window.into(), "enter enter");
        cx.run_until_parked();
        cx.read(|cx| {
            let v = view.read(cx);
            assert!(v.levelup.is_none(), "overlay closed after confirm");
            let c = &v.doc.character;
            assert_eq!(c.classes[0].get_level(), 2);
            // CON 10 (mod 0): fixed gain is d10/2 + 1 + 0 = 6.
            assert_eq!(c.health.max, hp0 + 6);
            assert!(
                c.features
                    .inner
                    .iter()
                    .any(|f| f.name.contains("Action Surge")),
                "level-2 grant landed"
            );
        });
        // One ctrl-z walks the whole level-up back (single undo snapshot).
        cx.simulate_keystrokes(window.into(), "ctrl-z");
        cx.run_until_parked();
        cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            assert_eq!(c.classes[0].get_level(), 1, "undo reverts the level");
            assert_eq!(c.health.max, hp0, "undo reverts the HP");
        });
    }

    #[gpui::test]
    fn level_up_overlay_esc_cancels_without_touching_the_sheet(cx: &mut TestAppContext) {
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        let hp0 = cx.read(|cx| view.read(cx).doc().character.health.max);
        cx.simulate_keystrokes(window.into(), "equal r escape");
        cx.run_until_parked();
        cx.read(|cx| {
            let v = view.read(cx);
            assert!(v.levelup.is_none(), "esc closes the overlay");
            let c = &v.doc.character;
            assert_eq!(c.classes[0].get_level(), 1, "level untouched");
            assert_eq!(c.health.max, hp0, "HP untouched");
        });
        // Keyboard focus came back to the sheet: `]` moves sections.
        cx.simulate_keystrokes(window.into(), "]");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).section),
            Section::Abilities,
            "focus is back on the sheet after cancel"
        );
    }

    #[gpui::test]
    fn level_up_overlay_prompts_subclass_at_3_and_asi_at_4(cx: &mut TestAppContext) {
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        // L1 -> L2 (HP, confirm).
        cx.simulate_keystrokes(window.into(), "equal enter enter");
        cx.run_until_parked();
        // L2 -> L3: HP enter, subclass step accepts the first option
        // (champion, per the corpus order the engine tests rely on),
        // confirm enter.
        cx.simulate_keystrokes(window.into(), "equal enter enter enter");
        cx.run_until_parked();
        cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            assert_eq!(c.classes[0].get_level(), 3);
            assert_eq!(c.classes[0].subclass, "champion", "subclass applied");
            assert!(
                c.features
                    .inner
                    .iter()
                    .any(|f| f.name.contains("Improved Critical")),
                "champion level-3 feature granted"
            );
        });
        // L3 -> L4: HP enter; ASI step: row 0 (STR) is selected, two
        // bumps, confirm.
        cx.simulate_keystrokes(window.into(), "equal enter equal equal enter enter");
        cx.run_until_parked();
        cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            assert_eq!(c.classes[0].get_level(), 4);
            assert_eq!(c.ability_scores.str.base, 12, "STR +2 from the ASI");
        });
    }

    #[gpui::test]
    fn level_up_asi_can_swap_for_a_feat_and_chains_the_picker(cx: &mut TestAppContext) {
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        // Two quick levels first (no corpus-free path here: the overlay
        // drives everything once a corpus is loaded).
        cx.simulate_keystrokes(window.into(), "equal enter enter");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "equal enter enter enter");
        cx.run_until_parked();
        // L4: HP enter, ASI step: `f` swaps the ASI for a feat, enter,
        // confirm.
        cx.simulate_keystrokes(window.into(), "equal enter f enter enter");
        cx.run_until_parked();
        cx.read(|cx| {
            let v = view.read(cx);
            let c = &v.doc.character;
            assert_eq!(c.classes[0].get_level(), 4);
            assert_eq!(
                c.ability_scores.str.base, 10,
                "no ASI bumps with feat chosen"
            );
            assert!(v.picker.is_some(), "the feat picker chained open");
        });
    }

    #[gpui::test]
    fn add_class_is_gated_by_multiclass_prerequisites(cx: &mut TestAppContext) {
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        // All-10 default sheet: wizard needs INT 13 — the pick is refused.
        cx.simulate_keystrokes(window.into(), "m");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).picker.is_some()),
            "m opens the class picker"
        );
        cx.simulate_input(window.into(), "wizard");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        cx.read(|cx| {
            let v = view.read(cx);
            assert_eq!(v.doc.character.classes.classes.len(), 1);
            assert!(v.picker.is_none(), "picker closed after the refusal");
        });
    }

    #[gpui::test]
    fn add_class_grants_the_reduced_multiclass_package(cx: &mut TestAppContext) {
        use blackletter_types::sheet::class::ClassType;
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        // Meet the wizard's INT 13 multiclass prerequisite.
        view.update(cx, |v, _| v.doc.character.ability_scores.int.base = 13);
        let profs0 = cx.read(|cx| view.read(cx).doc().character.proficiencies.specific.len());
        let hp0 = cx.read(|cx| view.read(cx).doc().character.health.max);
        cx.simulate_keystrokes(window.into(), "m");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "wizard");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        cx.read(|cx| {
            let v = view.read(cx);
            let c = &v.doc.character;
            assert_eq!(c.classes.classes.len(), 2);
            assert_eq!(c.classes.classes[1].class, ClassType::Wizard);
            assert_eq!(c.classes.classes[1].get_level(), 1);
            assert_eq!(c.classes.total_level(), 2);
            // Level-1 HP rule: full d6 + CON mod 0.
            assert_eq!(c.health.max, hp0 + 6);
            // PHB: a new class grants no skill proficiencies; the
            // wizard's multiclass proficiency set is empty anyway.
            assert_eq!(c.proficiencies.specific.len(), profs0);
            // SRD wizard level 1 feature lands.
            assert!(
                c.features
                    .inner
                    .iter()
                    .any(|f| f.name == "Spellcasting: Wizard")
                    && c.features.inner.iter().any(|f| f.name == "Arcane Recovery"),
                "SRD wizard level-1 features land"
            );
            // Multiclass caster level 1 -> two 1st-level slots.
            let slot = &c.meters.meters["spell_slot_1"];
            assert_eq!(slot.slot_number, 2);
        });
        // One ctrl-z reverts the whole multiclass.
        cx.simulate_keystrokes(window.into(), "ctrl-z");
        cx.run_until_parked();
        cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            assert_eq!(c.classes.classes.len(), 1);
            assert_eq!(c.health.max, hp0);
        });
    }

    #[gpui::test]
    fn enter_rename_type_enter_commits(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).field.is_some()),
            "field opens on enter"
        );
        cx.simulate_input(window.into(), "Minsc");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).doc().character.name.clone()),
            "Minsc"
        );
    }

    #[gpui::test]
    fn space_toggles_skill_proficiency_and_moves_the_check(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ]"); // Skills
        cx.run_until_parked();
        let before = cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            c.skills.check(&Skill::Athletics, &c.ability_scores)
        });
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        let (prof, after) = cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            (
                c.skills.proficiencies.contains(Skill::Acrobatics),
                c.skills.check(&Skill::Acrobatics, &c.ability_scores),
            )
        });
        assert!(prof, "row 0 is Acrobatics (Skill::ALL is alphabetical)");
        assert_eq!(after, before + 2, "PB is +2 at level 1");
        cx.simulate_keystrokes(window.into(), "space"); // toggle back
        cx.run_until_parked();
        assert!(!cx.read(|cx| {
            view.read(cx)
                .doc()
                .character
                .skills
                .proficiencies
                .contains(Skill::Acrobatics)
        }));
    }

    #[gpui::test]
    fn damage_respects_concentration_and_rest_restores(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        view.update(cx, |v, _| {
            v.doc.edit(|c| {
                c.start_concentration("Haste", 13);
            });
        });
        cx.simulate_keystrokes(window.into(), "] ] ]"); // Combat
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "d");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).field.is_some()),
            "d opens the damage field"
        );
        cx.simulate_keystrokes(window.into(), "4");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).field.as_ref().unwrap().1.text().to_string()),
            "4"
        );
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let (hp, conc) = cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            (
                c.health.current,
                c.concentrating.as_ref().map(|k| k.spell_name.clone()),
            )
        });
        assert_eq!(hp, 6, "4 bludgeoning against nothing resisted");
        // The concentration save is a real d20 roll: Haste survives on a save,
        // is broken otherwise. Either is consistent; anything else is not.
        assert!(
            conc.as_deref().is_none_or(|s| s == "Haste"),
            "concentration is kept or broken, never renamed"
        );

        // Long rest brings HP back to full.
        cx.simulate_keystrokes(window.into(), "shift-s");
        cx.run_until_parked();
        let full = cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            (c.health.current, c.max_health_total())
        });
        assert_eq!(full.0, full.1);
    }

    #[gpui::test]
    fn ability_plus_minus_moves_score_and_modifier(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "]"); // Abilities
        cx.run_until_parked();
        cx.simulate_keystrokes(
            window.into(),
            "equal equal equal equal equal equal equal equal equal",
        );
        cx.run_until_parked();
        let (base, modifier) = cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            (
                c.ability_scores.str.base,
                c.ability_modifier_for(Score::Str),
            )
        });
        assert_eq!(base, 19, "default 10 + 9 presses");
        assert_eq!(modifier, 4);
    }

    #[gpui::test]
    fn spells_section_casts_and_toggles_prepared(cx: &mut TestAppContext) {
        use blackletter_types::{
            class::{Class, Level},
            spells::Spell,
        };
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        view.update(cx, |v, _| {
            v.doc.edit(|c| {
                c.classes.classes.push(Class::new(Level(1), "wizard"));
                c.ability_scores.wis.base = 16;
                c.ability_modifier = Score::Wis;
                c.sync_proficiency_bonus();
                c.sync_spell_slots();
                let sp = Spell {
                    name: "shield".into(),
                    level: 1,
                    concentration: true,
                    ..Spell::default()
                };
                c.spells.spells.insert("shield".into(), sp);
            });
        });
        cx.simulate_keystrokes(window.into(), "] ] ] ]"); // Spells
        cx.run_until_parked();
        // Row 0 is the L1 slot meter (PHB full casters start with two):
        // spend both, so the cast below must fail.
        cx.simulate_keystrokes(window.into(), "space space");
        cx.run_until_parked();
        let spent = cx.read(|cx| view.read(cx).doc().character.meters.meters["spell_slot_1"].spent);
        assert_eq!(spent, 2, "space spends slot pips");
        // Move to the spell row and try to cast: no slots left.
        cx.simulate_keystrokes(window.into(), "j t");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).doc().character.concentrating.is_none()),
            "a failed cast must not start concentration"
        );
        // Re-press on a full-spent meter restores all pips, then cast:
        // shield concentrates at DC 13.
        cx.simulate_keystrokes(window.into(), "k space");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "j t");
        cx.run_until_parked();
        let snap = cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            (
                c.meters.meters["spell_slot_1"].spent,
                c.concentrating.as_ref().map(|k| k.spell_name.clone()),
                c.concentrating.as_ref().map(|k| k.save_dc),
            )
        });
        assert_eq!(snap.0, 1, "casting spends a slot");
        assert_eq!(snap.1.as_deref(), Some("shield"));
        assert_eq!(snap.2, Some(13), "8 + PB 2 + WIS 3");
        // Prepared toggle on the spell row (wizard = PreparedWithSpellbook).
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).doc().character.spells.spells["shield"].prepared),
            "space toggles prepared for prepared casters"
        );
    }

    #[gpui::test]
    fn dice_tray_rolls_all_kinds(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        // Attack with advantage: "a" opens the prompt, "a" marks adv.
        cx.simulate_keystrokes(window.into(), "a a enter");
        cx.run_until_parked();
        let log = cx.read(|cx| view.read(cx).roll_log.clone());
        assert_eq!(log.len(), 1, "{log:?}");
        // NB: a natural 20 appends " CRIT!" after the (adv) marker, so
        // match the marker with contains, not ends_with (flaky otherwise).
        assert!(
            log[0].starts_with("attack ") && log[0].contains("(adv)"),
            "{log:?}"
        );
        // Save.
        cx.simulate_keystrokes(window.into(), "v d e x enter");
        cx.run_until_parked();
        let log = cx.read(|cx| view.read(cx).roll_log.clone());
        assert!(log[0].starts_with("Dex save"), "{log:?}");
        // Check by skill prefix.
        cx.simulate_keystrokes(window.into(), "c p e r enter");
        cx.run_until_parked();
        let log = cx.read(|cx| view.read(cx).roll_log.clone());
        assert!(log[0].starts_with("Perception check"), "{log:?}");
        // Damage expression with a type: resisted/vulnerable/no effect.
        cx.simulate_keystrokes(window.into(), "x 2 d 6 enter");
        cx.run_until_parked();
        let log = cx.read(|cx| view.read(cx).roll_log.clone());
        assert!(log[0].starts_with("2d6:"), "{log:?}");
        assert_eq!(log.len(), 4);
    }

    #[gpui::test]
    fn inventory_add_equip_and_quantity(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ] ] ]"); // Inventory
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Inventory);
        // Row 0 is the add-item row on an empty inventory.
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "d a g g e r enter");
        cx.run_until_parked();
        let items = cx.read(|cx| {
            view.read(cx)
                .doc()
                .character
                .inventory
                .0
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        });
        assert_eq!(items, vec!["dagger".to_string()]);
        // +/- adjusts quantity; - back to zero removes.
        cx.simulate_keystrokes(window.into(), "plus");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).doc().character.inventory.0["dagger"].quantity),
            2
        );
        // space on the item equips it.
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        assert!(cx.read(|cx| view.read(cx).doc().character.inventory.0["dagger"].equipped));
        cx.simulate_keystrokes(window.into(), "minus minus");
        cx.run_until_parked();
        assert!(cx.read(|cx| view.read(cx).doc().character.inventory.0.is_empty()));
    }

    #[gpui::test]
    fn massive_overflow_kills_instantly(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ]"); // Combat
        cx.run_until_parked();
        // Default sheet: 10 max HP. 30 damage -> overflow 20 >= 10: dead.
        cx.simulate_keystrokes(window.into(), "d 3 0 enter");
        cx.run_until_parked();
        assert!(cx.read(|cx| view.read(cx).doc().character.is_dead()));
        assert_eq!(
            cx.read(|cx| view.read(cx).doc().character.health.current),
            0
        );
        // Undo revives the whole event.
        cx.simulate_keystrokes(window.into(), "ctrl-z");
        cx.run_until_parked();
        assert!(!cx.read(|cx| view.read(cx).doc().character.is_dead()));
        assert_eq!(
            cx.read(|cx| view.read(cx).doc().character.health.current),
            10
        );
    }

    #[gpui::test]
    fn combat_hit_dice_and_death_saves(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ]"); // Combat
        cx.run_until_parked();
        // Damage down to 4/10, then spend the single d10 hit die.
        cx.simulate_keystrokes(window.into(), "d 4 enter");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).doc().character.health.current),
            6
        );
        // Row 8 = spend hit die (j x8 from row 0).
        cx.simulate_keystrokes(window.into(), "j j j j j j j j");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).sel), 8);
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        let (hp, dice) = cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            (c.health.current, c.classes.classes[0].healing_die_remaining)
        });
        assert!(hp > 6, "hit die healed ({hp})");
        assert_eq!(dice, 0, "the L1 die is spent");
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).doc().character.classes.classes[0].healing_die_remaining),
            0,
            "no dice left to spend"
        );
        // Row 9 = death save roll (only meaningful while down; the engine
        // still tracks pips, and long rest clears them).
        cx.simulate_keystrokes(window.into(), "j");
        let hp_before = cx.read(|cx| view.read(cx).doc().character.health.current);
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        let (ds, hp_after) = cx.read(|cx| {
            let c = &view.read(cx).doc().character;
            (c.death_saves, c.health.current)
        });
        // RNG branches: a pip is recorded; a nat 20 revives (regain 1 HP,
        // pips reset) or stabilizes — both leave no pips behind, so ANY HP
        // change and `stable` are accepted alternates (flaky-proof). This
        // sheet is standing (HP > 0), where the engine's nat-20 branch
        // sets current to 1 — a *drop* — so compare inequality, not >.
        assert!(
            ds.successes + ds.failures > 0 || ds.stable || hp_after != hp_before,
            "a save was recorded, a nat 20 moved HP, or stabilized"
        );
        cx.simulate_keystrokes(window.into(), "S"); // shift-S long rest
        cx.run_until_parked();
        let ds = cx.read(|cx| view.read(cx).doc().character.death_saves);
        assert_eq!(
            ds.successes + ds.failures,
            0,
            "long rest clears death saves"
        );
        assert_eq!(
            cx.read(|cx| view.read(cx).doc().character.classes.classes[0].healing_die_remaining),
            1,
            "long rest regains the spent die"
        );
    }

    #[gpui::test]
    fn conditions_add_tick_and_remove(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ] ] ] ] ]"); // Conditions
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Conditions);
        // Row 0 is Blinded in the picker: apply it, then set a 2-round timer.
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).sel), 0);
        assert!(cx.read(|cx| {
            view.read(cx)
                .doc()
                .character
                .conditions
                .conditions
                .iter()
                .any(|c| c.kind.to_string() == "Blinded")
        }));
        cx.simulate_keystrokes(window.into(), "plus plus");
        cx.run_until_parked();
        // Two ticks expire it; the third tick would leave nothing to remove.
        cx.simulate_keystrokes(window.into(), "t t");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view
                .read(cx)
                .doc()
                .character
                .conditions
                .conditions
                .is_empty()),
            "expired conditions are removed by tick_conditions"
        );
        // Space again removes the condition.
        cx.simulate_keystrokes(window.into(), "space t");
        cx.run_until_parked();
        assert!(cx.read(|cx| {
            view.read(cx)
                .doc()
                .character
                .conditions
                .conditions
                .iter()
                .any(|c| c.kind.to_string() == "Blinded")
        }));
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        assert!(cx.read(|cx| {
            view.read(cx)
                .doc()
                .character
                .conditions
                .conditions
                .is_empty()
        }));
    }

    #[gpui::test]
    fn proficiency_toggles_and_xp_row(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ]"); // Skills & Saves
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Skills);
        // Walk to the first PROFICIENCIES row (24).
        cx.simulate_keystrokes(
            window.into(),
            "j j j j j j j j j j j j j j j j j j j j j j j j",
        );
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).sel), 24);
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| {
                view.read(cx)
                    .doc()
                    .character
                    .proficiencies
                    .armor
                    .contains(ArmorProf::Light)
            }),
            "space toggles light armor on"
        );
        // XP row lives at the bottom of Combat.
        cx.simulate_keystrokes(window.into(), "]"); // Combat
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "j j j j j j j j j j");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "plus");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).doc().character.xp), 100);
    }

    #[gpui::test]
    fn selection_scrolls_into_view(cx: &mut TestAppContext) {
        // Short window: the Skills list (~34 rows) cannot fit, so the
        // viewport must follow the selection (user ruling 2026-11-24).
        let (window, root) =
            open_sheet_sized(cx, None, Some(gpui::size(gpui::px(900.), gpui::px(420.))));
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ]"); // Skills & Saves
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).scroll_offset_y()) > -40.,
            "a fresh section starts at the top (within padding slack)"
        );
        // Walk to the first PROFICIENCIES row (24).
        cx.simulate_keystrokes(
            window.into(),
            "j j j j j j j j j j j j j j j j j j j j j j j j",
        );
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).sel), 24);
        assert!(
            cx.read(|cx| view.read(cx).scroll_offset_y()) < 0.,
            "selection past the viewport bottom must scroll the list"
        );
        // Section switches reset the viewport to the top.
        cx.simulate_keystrokes(window.into(), "]"); // Combat
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).scroll_offset_y()) > -40.,
            "section switch resets the viewport to the top"
        );
    }

    #[gpui::test]
    fn focus_survives_section_navigation(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let _view = sheet(cx, &root);
        let before = window
            .update(cx, |_, window, cx| window.focused(cx))
            .unwrap();
        cx.simulate_keystrokes(window.into(), "]");
        cx.run_until_parked();
        let after = window
            .update(cx, |_, window, cx| window.focused(cx))
            .unwrap();
        assert_eq!(before, after, "focus must not move on section change");
    }

    #[gpui::test]
    fn tab_walks_sections(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Basic);
        cx.simulate_keystrokes(window.into(), "] ]");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Skills);
        cx.simulate_keystrokes(window.into(), "[");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Abilities);
    }

    // ---- SRD pickers (plan Task 4.2) ----

    #[gpui::test]
    fn sheet_field_backspace_edits_the_prompt(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ]"); // Combat
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "d"); // damage prompt
        cx.run_until_parked();
        cx.simulate_input(window.into(), "45");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "backspace"); // "45" -> "4"
        cx.run_until_parked();
        let hp0 = cx.read(|cx| view.read(cx).doc().character.health.current);
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let hp1 = cx.read(|cx| view.read(cx).doc().character.health.current);
        assert_eq!(hp0 - hp1, 4, "backspace must edit the field before commit");
    }

    #[gpui::test]
    fn spell_picker_adds_a_spell_via_the_conversion(cx: &mut TestAppContext) {
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ] ]"); // Spells
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "/");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).picker.is_some()),
            "/ opens the spell picker"
        );
        cx.simulate_input(window.into(), "water walk");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let spells = cx.read(|cx| {
            view.read(cx)
                .doc()
                .character
                .spells
                .spells
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        });
        assert!(
            spells.iter().any(|s| s == "Water Walk"),
            "Water Walk must be in the spellbook: {spells:?}"
        );
        assert!(
            cx.read(|cx| view.read(cx).picker.is_none()),
            "picker dismisses after picking"
        );
    }

    #[gpui::test]
    fn the_custom_form_adds_a_spell_to_the_library(cx: &mut TestAppContext) {
        // Task 4.3 slice C: `n` opens the add-menu, ctrl-s lands the entry
        // in the library entity (and, with a path configured, on disk).
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ] ]"); // Spells
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "n");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).form.is_some()),
            "n opens the custom-entry form in the Spells section"
        );
        // enter edits the name row; type; enter commits; ctrl-s saves.
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "Moonfire Bolt");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter ctrl-s");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| view.read(cx).form.is_none()),
            "form dismisses after saving"
        );
        let spells = cx.read(|cx| {
            root.read(cx)
                .library()
                .read(cx)
                .spells
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        });
        assert_eq!(spells, vec!["Moonfire Bolt"], "library holds the new spell");
        // And it shows up in the picker straight away.
        cx.simulate_keystrokes(window.into(), "/");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "moonfire");
        cx.run_until_parked();
        let rows = cx.read(|cx| {
            view.read(cx)
                .picker
                .as_ref()
                .map(|p| p.read(cx).row_indexes())
                .unwrap_or_default()
        });
        assert_eq!(rows, vec!["custom:Moonfire Bolt"]);
    }

    #[gpui::test]
    fn the_library_file_caches_added_content(cx: &mut TestAppContext) {
        // The user's rule: anything added is cached and shows up in future
        // sessions. Drive the form with a real library path in a temp dir.
        let dir = std::env::temp_dir().join(format!("bl_form_lib_{}", std::process::id()));
        let path = dir.join("nested/library.json");
        cx.update(|cx| {
            cx.bind_keys(crate::root::keybindings());
            cx.bind_keys(keybindings());
            cx.bind_keys(crate::field::field_keys(crate::dungeon_view::FIELD_CTX));
            cx.bind_keys(crate::picker::picker_keys());
            cx.bind_keys(crate::form::form_keys());
            crate::autosave::Autosave::init(cx);
        });
        let window = cx.add_window(|_, cx| {
            RootView::new(
                cx,
                Session::default(),
                None,
                Some(test_corpus()),
                Library::default(),
                Some(path.clone()),
            )
        });
        window
            .update(cx, |view, window, cx| window.focus(view.focus_handle(), cx))
            .unwrap();
        let root = window.root(cx).unwrap();
        let _ = window.update(cx, |view, window, cx| view.ensure_sheet(window, cx));
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ] ]"); // Spells
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "n");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "Ember Lance");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter ctrl-s");
        cx.run_until_parked();
        let back = Library::load(&path).expect("library file was written");
        assert!(
            back.spells.contains_key("Ember Lance"),
            "the add is cached on disk"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = view;
    }

    #[gpui::test]
    fn a_library_spell_flows_through_the_picker(cx: &mut TestAppContext) {
        // Task 4.3: custom content rides the same picker as SRD content.
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        root.update(cx, |root, cx| {
            root.library().update(cx, |lib, _| {
                lib.spells.insert(
                    "Moonfire Bolt".into(),
                    blackletter_types::sheet::spells::Spell {
                        name: "Moonfire Bolt".into(),
                        level: 4,
                        ..Default::default()
                    },
                );
            });
        });
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ] ]"); // Spells
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "/");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "moonfire");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let spells = cx.read(|cx| {
            view.read(cx)
                .doc()
                .character
                .spells
                .spells
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        });
        assert!(
            spells.iter().any(|s| s == "Moonfire Bolt"),
            "library spell must land in the spellbook: {spells:?}"
        );
    }

    #[gpui::test]
    fn race_picker_sets_typed_race_fields_from_the_srd(cx: &mut TestAppContext) {
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "/"); // Basic -> race
        cx.run_until_parked();
        cx.simulate_input(window.into(), "dwarf");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let race = cx.read(|cx| view.read(cx).doc().character.race.clone());
        let race = race.expect("race set from the picker");
        assert_eq!(race.name, "Dwarf");
        assert_eq!(race.speed, Some(25));
        assert!(
            race.ability_bonuses
                .iter()
                .any(|b| b.score == Score::Con && b.bonus == 2),
            "Dwarf carries its typed CON bonus"
        );
    }

    #[gpui::test]
    fn picker_escape_cancels_without_touching_the_sheet(cx: &mut TestAppContext) {
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "/");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "dwarf");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        assert!(cx.read(|cx| view.read(cx).picker.is_none()));
        // The fresh sheet's default Elf race is untouched (no Dwarf import).
        assert_eq!(
            cx.read(|cx| {
                view.read(cx)
                    .doc()
                    .character
                    .race
                    .as_ref()
                    .map(|r| r.name.clone())
            }),
            Some("Elf".to_string())
        );
        // Focus returns to the sheet: a sheet chord still works.
        cx.simulate_keystrokes(window.into(), "]");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Abilities);
    }

    #[gpui::test]
    fn inventory_picker_adds_an_srd_item(cx: &mut TestAppContext) {
        let (window, root) = open_sheet_with(cx, Some(test_corpus()));
        let view = sheet(cx, &root);
        cx.simulate_keystrokes(window.into(), "] ] ] ] ]"); // Inventory
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "/");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "flail");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let items = cx.read(|cx| {
            view.read(cx)
                .doc()
                .character
                .inventory
                .0
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        });
        assert!(items.iter().any(|i| i == "Flail"), "items: {items:?}");
    }
}
