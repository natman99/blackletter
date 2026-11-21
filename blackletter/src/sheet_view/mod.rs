//! Sheet mode: the editable character document + three-column layout.
//!
//! Layout (plan Task 3.1): left = section nav, center = the section form,
//! right = an always-visible summary card. The view never recomputes
//! modifiers — every number on screen comes from the engine's derived
//! getters (`armor_class_total()`, `skill_check()`, `passive_senses()`, …).

mod doc;

use std::path::PathBuf;

use blackletter_types::{
    Item, Score,
    class::CastingMechanic,
    condition::{Condition, ConditionKind},
    damage::DamageType,
    skills::Skill,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable, KeyBinding,
    SharedString, WeakEntity, Window, actions, div, prelude::*,
};

pub use doc::SheetDoc;

use crate::field::{Field, FieldElement, FieldOwner};
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
        ToggleRow,
        QuickDamage,
        CycleDamageType,
        RollDeathSave,
        ShortRest,
        LongRest,
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
        "enter" => EditName,
        "space" => ToggleRow,
        "d" => QuickDamage,
        "t" => CycleDamageType,
        "r" => RollDeathSave,
        "s" => ShortRest,
        "shift-s" => LongRest,
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
const COMBAT_ROWS: usize = 10;
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
];

pub struct SheetView {
    doc: SheetDoc,
    /// The campaign's `characters/` dir for first-saves of new sheets.
    characters_dir: Option<PathBuf>,
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
    blank: Field,
}

impl SheetView {
    pub fn new(
        doc: SheetDoc,
        characters_dir: Option<PathBuf>,
        root: WeakEntity<RootView>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            doc,
            characters_dir,
            root,
            section: Section::Basic,
            focus_handle: cx.focus_handle(),
            class_sel: 0,
            sel: 0,
            damage_type: DamageType::Bludgeoning,
            field: None,
            blank: Field::new(""),
        }
    }

    pub fn doc(&self) -> &SheetDoc {
        &self.doc
    }

    #[allow(dead_code)] // read by tests
    pub fn section(&self) -> Section {
        self.section
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
        cx.notify();
    }

    fn on_prev_section(&mut self, _: &PrevSection, _: &mut Window, cx: &mut Context<Self>) {
        self.section = self.section.prev();
        self.sel = 0;
        cx.notify();
    }

    /// Number of selectable rows in the active section.
    fn rows(&self) -> usize {
        match self.section {
            Section::Basic => self.doc.character.classes.classes.len(),
            Section::Abilities => 6,
            Section::Skills => 24,
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
        cx.notify();
    }

    fn on_level_up(&mut self, _: &LevelUp, _: &mut Window, cx: &mut Context<Self>) {
        self.on_adjust(1, cx);
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
    fn on_toggle_row(&mut self, _: &ToggleRow, _: &mut Window, cx: &mut Context<Self>) {
        match self.section {
            Section::Skills => self.toggle_skill_or_save(cx),
            Section::Combat => self.combat_activate(cx),
            Section::Spells => self.toggle_spell_row(cx),
            Section::Conditions => self.toggle_condition_row(cx),
            Section::Inventory => self.toggle_item_row(cx),
            Section::Features => self.remove_grant_row(cx),
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
                    .child("[]: sections\nj/k: class  +/-: level\nenter: name  ctrl-s: save"),
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
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(div().text_lg().child(c.name.clone()))
            .when_some(self.field.as_ref(), |d, _| {
                d.child(
                    div()
                        .w(gpui::px(320.))
                        .child(FieldElement::new(cx.entity())),
                )
            })
            .child(row(
                "Race",
                c.race.as_ref().map_or("—".into(), |r| r.name.clone()),
            ))
            .child(row(
                "Background",
                c.background.as_ref().map_or("—".into(), |b| b.name.clone()),
            ))
            .child(row("Level", c.classes.total_level().to_string()))
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
        div()
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
        div()
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
        div()
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
                    .child("j/k select  space spend-slot/toggle-prepared  t cast"),
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
        div()
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
        div()
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
                        "j/k select  space equip/unequip  +/- quantity\n(- to 0 removes the item)",
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
        div()
            .flex()
            .flex_col()
            .gap_0()
            .p_2()
            .children(rows)
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("j/k select  space removes the grant (ctrl-z undoes)"),
            )
            .into_any_element()
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
        div()
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
        div()
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
        div()
            .track_focus(&self.focus_handle)
            .key_context(if field_active {
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
            .child(self.nav(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
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
                    .child(self.center(cx)),
            )
            .child(self.summary())
    }
}

/// Constructor used by the root view.
pub fn build(
    doc: SheetDoc,
    characters_dir: Option<PathBuf>,
    root: WeakEntity<RootView>,
    cx: &mut Context<RootView>,
) -> Entity<SheetView> {
    cx.new(|cx| SheetView::new(doc, characters_dir, root, cx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autosave::Autosave;
    use crate::session::Session;
    use gpui::{Entity, TestAppContext, WindowHandle};

    fn open_sheet(cx: &mut TestAppContext) -> (WindowHandle<RootView>, Entity<RootView>) {
        cx.update(|cx| {
            cx.bind_keys(crate::root::keybindings());
            cx.bind_keys(keybindings());
            cx.bind_keys(crate::field::field_keys(crate::dungeon_view::FIELD_CTX));
            Autosave::init(cx);
        });
        let window = cx.add_window(|_, cx| RootView::new(cx, Session::default(), None));
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
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        let ds = cx.read(|cx| view.read(cx).doc().character.death_saves);
        assert!(ds.successes + ds.failures > 0, "a save was recorded");
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
}
