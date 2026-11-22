//! New-character creation wizard (plan Task 5.1): a linear step flow from
//! blank sheet to a fully granted level-1 `Character`.
//!
//! Steps: name → race (→ subrace when the race has them) → class →
//! background → ability scores (standard array) → class starting-equipment
//! and skill choices → confirm. Pickers (Task 4.2) drive the corpus lookups;
//! the draft is pure data, and `build_character` is a free function so the
//! grant math is testable without a window.
//!
//! Keyboard contract (WIZARD_CTX): enter advances / confirms, shift-enter
//! steps back, up/down move row selections, space toggles a choice,
//! left/right cycle a score, esc cancels. Picker and name-field steps swap
//! the key context exactly like the sheet does (Task 4.2 lesson: an
//! ancestor context's printable bindings would otherwise steal keystrokes).

use std::sync::Arc;

use blackletter_types::{
    AbilityScores, Character, HitPoints, Item, Score,
    ability_score::AbilityScore,
    class::Class,
    database::corpus::Corpus,
    library::Library,
    sheet::{background::Background, race::Race, skills::Skill, source::Source, traits::Trait},
};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyBinding, Render, SharedString, Window,
    actions, div, prelude::*,
};

use crate::field::{Field, FieldCancel, FieldElement, FieldOwner, FieldSubmit};
use crate::picker::{PickKind, Picker, PickerOutcome};

pub const WIZARD_CTX: &str = "Wizard";

actions!(
    wizard,
    [
        WizardNext,
        WizardBack,
        WizardUp,
        WizardDown,
        WizardToggle,
        WizardCycleDown,
        WizardCycleUp,
        WizardCancel
    ]
);

#[must_use]
pub fn wizard_keys() -> Vec<KeyBinding> {
    let ctx = Some(WIZARD_CTX);
    let mut kb = vec![
        KeyBinding::new("enter", WizardNext, ctx),
        KeyBinding::new("shift-enter", WizardBack, ctx),
        KeyBinding::new("up", WizardUp, ctx),
        KeyBinding::new("down", WizardDown, ctx),
        KeyBinding::new("space", WizardToggle, ctx),
        KeyBinding::new("left", WizardCycleDown, ctx),
        KeyBinding::new("right", WizardCycleUp, ctx),
        KeyBinding::new("escape", WizardCancel, ctx),
    ];
    // NOTE: deliberately no field_keys(WIZARD_CTX) here — the name step
    // runs in FIELD_CTX (which carries the field table), and extending it
    // into WIZARD_CTX double-binds enter/esc/left/right to Field actions
    // the wizard does not handle, shadowing WizardNext/WizardCancel/
    // WizardCycleUp (live-check finding).
    kb
}

/// The PHB standard array. Point buy is a planned sibling (Task 5.1 step
/// 5 "point-buy 27 + standard array + manual"); the array ships first.
const STANDARD_ARRAY: [i64; 6] = [15, 14, 13, 12, 10, 8];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Name,
    Race,
    Subrace,
    Class,
    Background,
    Scores,
    /// One prompt per class option (equipment alternatives, then skill
    /// choices); the payload is the option's index in `pending_options`.
    Choice(usize),
    Confirm,
}

impl Step {
    fn label(self) -> &'static str {
        match self {
            Step::Name => "name",
            Step::Race => "race",
            Step::Subrace => "subrace",
            Step::Class => "class",
            Step::Background => "background",
            Step::Scores => "ability scores",
            Step::Choice(_) => "starting choices",
            Step::Confirm => "confirm",
        }
    }
}

/// Everything the wizard collects, as plain data (testable without gpui).
#[derive(Debug, Default)]
pub struct Draft {
    name: String,
    race: Option<String>,
    subrace: Option<String>,
    class: Option<String>,
    background: Option<String>,
    /// Standard-array values assigned per `Score::ALL` slot (str..cha).
    scores: [Option<i64>; 6],
    /// Per pending option: the chosen candidate indexes.
    choices: Vec<Vec<usize>>,
}

pub enum WizardOutcome {
    // Boxed: `Character` is far larger than the unit variant (clippy).
    Finished(Box<Character>),
    Cancelled,
}

type FinishFn = Box<dyn FnMut(WizardOutcome, &mut Window, &mut App) + Send>;

pub struct WizardView {
    corpus: Arc<Corpus>,
    library: Entity<Library>,
    draft: Draft,
    steps: Vec<Step>,
    idx: usize,
    /// Row selection for the scores / choice steps.
    sel: usize,
    picker: Option<Entity<Picker>>,
    field: Field,
    status: Option<SharedString>,
    focus_handle: FocusHandle,
    on_finish: Option<FinishFn>,
}

impl WizardView {
    pub fn new(
        corpus: Arc<Corpus>,
        library: Entity<Library>,
        on_finish: impl FnMut(WizardOutcome, &mut Window, &mut App) + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut field = Field::new("");
        field.caret_to_end();
        let mut me = Self {
            corpus,
            library,
            draft: Draft::default(),
            steps: vec![
                Step::Name,
                Step::Race,
                Step::Class,
                Step::Background,
                Step::Scores,
                Step::Confirm,
            ],
            idx: 0,
            sel: 0,
            picker: None,
            field,
            status: None,
            focus_handle: cx.focus_handle(),
            on_finish: Some(Box::new(on_finish)),
        };
        me.say("start with a name — enter continues", cx);
        me
    }

    fn say(&mut self, line: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = Some(line.into());
        cx.notify();
    }

    fn step(&self) -> Step {
        self.steps[self.idx]
    }

    /// The class's "choose X from Y" prompts: equipment alternatives
    /// first, then skill choices (PHB ordering).
    fn pending_options(&self) -> Vec<blackletter_types::database::options::ChoiceOption> {
        let Some(ci) = &self.draft.class else {
            return Vec::new();
        };
        let Some(db) = self.corpus.class(ci) else {
            return Vec::new();
        };
        let mut opts = db.starting_equipment_options.clone();
        opts.extend(db.proficiency_choices.clone());
        opts
    }

    /// Rebuild the step list after race/class picks: subrace step appears
    /// only for subrace-bearing races; one Choice step per class option.
    fn rebuild_steps(&mut self) {
        let mut steps = vec![Step::Name];
        steps.push(Step::Race);
        if let Some(ri) = &self.draft.race
            && let Some(r) = self.corpus.race(ri)
            && !r.subraces.is_empty()
        {
            steps.push(Step::Subrace);
        }
        steps.push(Step::Class);
        steps.push(Step::Background);
        steps.push(Step::Scores);
        for i in 0..self.pending_options().len() {
            steps.push(Step::Choice(i));
        }
        steps.push(Step::Confirm);
        self.steps = steps;
    }

    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        if self.idx + 1 < self.steps.len() {
            self.idx += 1;
            self.sel = 0;
            self.enter_step(window, cx);
        }
        self.reclaim_focus(window, cx);
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        if self.idx > 0 {
            self.idx -= 1;
            self.sel = 0;
            self.enter_step(window, cx);
        }
        self.reclaim_focus(window, cx);
    }

    /// gpui drops focus when the focused entity (a closed picker) is
    /// released; keyboard steps need the wizard's own handle back.
    fn reclaim_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_none() {
            window.focus(&self.focus_handle, cx);
        }
    }

    /// Side effects on entering a step: picker steps open their picker.
    fn enter_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.step() {
            Step::Race => self.open_picker(PickKind::Race, window, cx),
            Step::Subrace => self.open_picker(PickKind::Subrace, window, cx),
            Step::Class => self.open_picker(PickKind::Class, window, cx),
            Step::Background => self.open_picker(PickKind::Background, window, cx),
            Step::Scores => self.say("↑/↓ pick an ability · ←/→ cycle the standard array", cx),
            Step::Choice(i) => {
                let opts = self.pending_options();
                match opts.get(i) {
                    Some(o) => self.say(format!("{} — space selects, enter confirms", o.desc), cx),
                    None => self.say("choose", cx),
                }
            }
            Step::Confirm => self.say("enter creates the character · shift-enter edits", cx),
            Step::Name => self.say("start with a name — enter continues", cx),
        }
    }

    fn open_picker(&mut self, kind: PickKind, window: &mut Window, cx: &mut Context<Self>) {
        let lib = self.library.read(cx).clone();
        let corpus = self.corpus.clone();
        let wiz = cx.entity().downgrade();
        let picker = cx.new(|cx| {
            Picker::new(
                kind,
                corpus,
                Some(&lib),
                move |outcome, window, cx| {
                    wiz.update(cx, |v, cx| v.on_pick(kind, outcome, window, cx))
                        .ok();
                },
                cx,
            )
        });
        if kind == PickKind::Subrace
            && let Some(ri) = &self.draft.race
            && let Some(r) = self.corpus.race(ri)
        {
            let allowed: Vec<String> = r.subraces.iter().map(|sr| sr.index.clone()).collect();
            picker.update(cx, |p, _| p.restrict(&allowed));
        }
        self.picker = Some(picker.clone());
        window.focus(&picker.read(cx).focus_handle(cx), cx);
        self.say("type to filter · ↑/↓ move · enter picks · esc closes", cx);
        cx.notify();
    }

    fn on_pick(
        &mut self,
        kind: PickKind,
        outcome: PickerOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.picker = None;
        match outcome {
            PickerOutcome::Cancelled => {
                self.say("picker closed — enter reopens it", cx);
                self.reclaim_focus(window, cx);
            }
            PickerOutcome::Chosen(index) => match kind {
                PickKind::Race => {
                    self.draft.race = Some(index);
                    self.draft.subrace = None;
                    self.rebuild_steps();
                    self.say("race chosen", cx);
                    self.advance(window, cx);
                }
                PickKind::Subrace => {
                    self.draft.subrace = Some(index);
                    self.say("subrace chosen", cx);
                    self.advance(window, cx);
                }
                PickKind::Class => {
                    self.draft.class = Some(index);
                    self.draft.choices = vec![Vec::new(); self.pending_options().len()];
                    self.rebuild_steps();
                    self.say("class chosen", cx);
                    self.advance(window, cx);
                }
                PickKind::Background => {
                    self.draft.background = Some(index);
                    self.say("background chosen", cx);
                    self.advance(window, cx);
                }
                _ => self.say("that pick does not belong here", cx),
            },
        }
    }

    // ---- scores step ----

    fn score_pool(&self, except_row: usize) -> Vec<i64> {
        let mut pool: Vec<i64> = STANDARD_ARRAY.to_vec();
        for (i, v) in self.draft.scores.iter().enumerate() {
            if i != except_row
                && let Some(v) = *v
                && let Some(p) = pool.iter().position(|x| *x == v)
            {
                pool.remove(p);
            }
        }
        pool
    }

    fn cycle_score(&mut self, dir: isize, cx: &mut Context<Self>) {
        if self.step() != Step::Scores {
            return;
        }
        let row = self.sel;
        // The cycle list is the free pool plus the row's own current value.
        let mut values = self.score_pool(row);
        if let Some(cur) = self.draft.scores[row] {
            values.push(cur);
            values.sort_unstable();
        }
        if values.is_empty() {
            return;
        }
        let pos = self.draft.scores[row]
            .and_then(|cur| values.iter().position(|v| *v == cur))
            .unwrap_or(values.len());
        let next = (pos as isize + dir).rem_euclid(values.len() as isize) as usize;
        self.draft.scores[row] = Some(values[next]);
        self.say(
            format!(
                "{} = {} · pool: {}",
                Score::ALL_STR[row].to_uppercase(),
                values[next],
                self.score_pool(row)
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            cx,
        );
    }

    // ---- choice steps ----

    fn toggle_choice(&mut self, cx: &mut Context<Self>) {
        let Step::Choice(i) = self.step() else {
            return;
        };
        let opts = self.pending_options();
        let Some(opt) = opts.get(i) else { return };
        let cands = opt.candidates();
        if cands.get(self.sel).is_none() {
            return;
        }
        let idx = self.sel;
        let sel = &mut self.draft.choices[i];
        match sel.iter().position(|c| *c == idx) {
            Some(p) => {
                sel.remove(p);
            }
            None => {
                if (sel.len() as i64) < opt.choose {
                    sel.push(idx);
                } else {
                    self.say(
                        format!("only {} to choose — unselect one first", opt.choose),
                        cx,
                    );
                    return;
                }
            }
        }
        self.say(
            format!("{} selected of {}", self.draft.choices[i].len(), opt.choose),
            cx,
        );
    }

    // ---- finish ----

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = match build_character(&self.corpus, &self.draft) {
            Ok(c) => WizardOutcome::Finished(Box::new(c)),
            Err(e) => {
                self.say(format!("cannot create: {e}"), cx);
                return;
            }
        };
        if let Some(mut cb) = self.on_finish.take() {
            cb(outcome, window, cx);
        }
    }
}

/// Turn a completed draft into a granted level-1 character (plan Task
/// 5.1 step 7): typed race/class/background, level-1 features, trait
/// prose, skill proficiencies, standard-array scores with race bonuses
/// folded into HP, starting equipment, gold, and spell slots.
pub fn build_character(corpus: &Corpus, draft: &Draft) -> anyhow::Result<Character> {
    anyhow::ensure!(!draft.name.trim().is_empty(), "no name");
    let ri = draft
        .race
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no race"))?;
    let ci = draft
        .class
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no class"))?;
    let bi = draft
        .background
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no background"))?;
    anyhow::ensure!(
        draft.scores.iter().all(Option::is_some),
        "all six scores must be assigned"
    );

    // Race (+ subrace).
    let race_db = corpus.race(ri).ctx_missing("race", ri)?;
    let mut race = Race::try_from_database(race_db.clone());
    if let Some(si) = &draft.subrace {
        let sub = corpus.subrace(si).ctx_missing("subrace", si)?;
        race.apply_subrace(sub);
    }

    // Class.
    let class_db = corpus.class(ci).ctx_missing("class", ci)?;
    let class = Class::try_from_database(class_db);

    // Scores: standard array as bases; race bonuses ride the grant layer
    // (modifiers()), except CON, which the level-1 HP math needs inline.
    let mut scores = AbilityScores::default();
    for (i, v) in draft.scores.iter().enumerate() {
        let base = v.unwrap_or(8);
        match Score::ALL[i] {
            Score::Str => scores.str = AbilityScore::new(base, 0),
            Score::Dex => scores.dex = AbilityScore::new(base, 0),
            Score::Con => scores.con = AbilityScore::new(base, 0),
            Score::Int => scores.int = AbilityScore::new(base, 0),
            Score::Wis => scores.wis = AbilityScore::new(base, 0),
            Score::Cha => scores.cha = AbilityScore::new(base, 0),
        }
    }
    let con_mod = scores.con.modifier()
        + race
            .ability_bonuses
            .iter()
            .filter(|b| b.score == Score::Con)
            .map(|b| b.bonus as i64)
            .sum::<i64>();
    let hp = (i64::from(class_db.hit_die) + con_mod).max(1);

    let mut c = Character::new(
        draft.name.trim().to_string(),
        1,
        HitPoints {
            current: hp,
            max: hp,
            level_bonus: 0,
            bonus: 0,
        },
        class.class.clone(),
        scores,
    );
    c.classes.classes[0] = class;
    c.race = Some(race.clone());

    // Background grant + its skill proficiencies (bitflags are the sheet's
    // proficiency truth; the grant carries the fiction).
    let bg_db = corpus.background(bi).ctx_missing("background", bi)?;
    let bg = Background::try_from(bg_db.clone()).map_err(anyhow::Error::msg)?;
    for skill in bg.skill_proficiencies.clone() {
        c.skills.proficiencies.insert(skill);
    }
    c.gold = bg.gold;
    c.background = Some(bg);

    // Race traits as prose grants (full corpus rows where available).
    let mut trait_indexes: Vec<String> = race_db.traits.iter().map(|t| t.index.clone()).collect();
    if let Some(si) = &draft.subrace
        && let Some(sub) = corpus.subrace(si)
    {
        trait_indexes.extend(sub.racial_traits.iter().map(|t| t.index.clone()));
    }
    for ti in trait_indexes {
        if let Some(row) = corpus.traits.iter().find(|t| t.index == ti) {
            let tr = Trait {
                name: row.name.clone(),
                desc: row.desc.join("\n\n"),
                feature: None,
                source: Some(Source::Single("SRD (2014)".into())),
            };
            if !c.has_grant(&tr.name) {
                c.add_trait(tr);
            }
        }
    }

    // Level-1 class features (prose; mechanics ride in with later phases).
    for f in corpus.level_features(ci, 1) {
        let feat = blackletter_types::feature::Feature {
            name: f.name.clone(),
            source: Some(Source::Single("SRD (2014)".into())),
            effects: Vec::new(),
        };
        if !c.has_grant(&feat.name) {
            c.add_feature(feat);
        }
    }

    // Class skill choices (e.g. wizard: 2 from Arcana..Religion).
    for (i, opt) in class_db.proficiency_choices.iter().enumerate() {
        let Some(selected) = draft.choices.get(choice_index_for(class_db, i, true)) else {
            continue;
        };
        let cands = opt.candidates();
        for ci_idx in selected {
            let Some(cand) = cands.get(*ci_idx) else {
                continue;
            };
            if let Some(name) = cand.index.strip_prefix("skill-")
                && let Ok(skill) = name.parse::<Skill>()
            {
                c.skills.proficiencies.insert(skill);
            }
        }
    }

    // Starting equipment: the fixed package, then the chosen alternatives.
    for entry in &class_db.starting_equipment {
        if let Some(eq) = corpus.item(&entry.equipment.index)
            && let Ok(item) = Item::try_from_database(eq.clone())
        {
            c.inventory.0.insert(item.name.clone(), item);
        }
    }
    for (i, opt) in class_db.starting_equipment_options.iter().enumerate() {
        let Some(selected) = draft.choices.get(choice_index_for(class_db, i, false)) else {
            continue;
        };
        let cands = opt.candidates();
        for ci_idx in selected {
            let Some(cand) = cands.get(*ci_idx) else {
                continue;
            };
            if let Some(eq) = corpus.item(&cand.index)
                && let Ok(mut item) = Item::try_from_database(eq.clone())
            {
                item.quantity = cand.quantity.max(1) as i32;
                c.inventory.0.insert(item.name.clone(), item);
            }
        }
    }

    // Feats: none at level 1 (ASI is a level-up concern, Task 5.2).
    c.sync_spell_slots();
    Ok(c)
}

/// `draft.choices` is indexed equipment-options-first, then
/// proficiency-choices — map each list's local index into that space.
fn choice_index_for(
    class_db: &blackletter_types::database::class::DatabaseClass,
    i: usize,
    prof: bool,
) -> usize {
    if prof {
        class_db.starting_equipment_options.len() + i
    } else {
        i
    }
}

// Small context helpers (keeps build_character readable without anyhow's
// Option-ext feature churn).
trait OptionCtx<T> {
    fn ctx_missing(self, what: &str, index: &str) -> anyhow::Result<T>;
}
impl<T> OptionCtx<T> for Option<T> {
    fn ctx_missing(self, what: &str, index: &str) -> anyhow::Result<T> {
        self.ok_or_else(|| anyhow::anyhow!("corpus has no {what} {index}"))
    }
}

// ---- FieldOwner / input / focus / render ----

impl FieldOwner for WizardView {
    fn field(&self) -> &Field {
        &self.field
    }

    fn field_mut(&mut self) -> &mut Field {
        &mut self.field
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

impl gpui::EntityInputHandler for WizardView {
    fn text_for_range(
        &mut self,
        range_utf16: std::ops::Range<usize>,
        actual_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let (text, used) = self.field.text_in_utf16(range_utf16);
        actual_range.replace(used);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        Some(self.field.caret_selection())
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
        self.field.replace_utf16(range_utf16, new_text);
        cx.notify();
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

impl Focusable for WizardView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for WizardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let step = self.step();
        let picker_open = self.picker.is_some();
        let name_active = step == Step::Name && !picker_open;

        // Left rail: the step list with done/current markers.
        let rail: Vec<_> = self
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mark = if i < self.idx {
                    "✓"
                } else if i == self.idx {
                    "▸"
                } else {
                    " "
                };
                div()
                    .text_sm()
                    .text_color(if i == self.idx {
                        gpui::rgb(0xcd_d6_f4)
                    } else {
                        gpui::rgb(0x6c_70_90)
                    })
                    .child(format!("{mark} {}", s.label()))
            })
            .collect();

        // Center: the current step's body.
        let body = match step {
            Step::Name => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_lg().child("What is the character's name?"))
                .child(
                    div()
                        .w(gpui::px(360.))
                        .child(FieldElement::new(cx.entity())),
                ),
            Step::Race | Step::Subrace | Step::Class | Step::Background => {
                if picker_open {
                    div().child("")
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(format!("Pick a {}.", step.label()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(gpui::rgb(0x8a_90_b0))
                                .child("enter reopens the picker"),
                        )
                }
            }
            Step::Scores => {
                let pool = self.score_pool(self.sel);
                let pool_s = pool
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join("  ");
                let rows: Vec<_> = Score::ALL_STR
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        let selected = i == self.sel;
                        let v = self.draft.scores[i]
                            .map(|v| v.to_string())
                            .unwrap_or("—".into());
                        div()
                            .id(("score", i))
                            .px_2()
                            .py_0()
                            .rounded_sm()
                            .flex()
                            .gap_3()
                            .when(selected, |d| d.bg(gpui::rgb(0x35_35_48)))
                            .on_click(cx.listener(move |this, _, _window, _cx| {
                                this.sel = i;
                            }))
                            .child(
                                div()
                                    .w(gpui::px(70.))
                                    .text_color(gpui::rgb(0x8a_90_b0))
                                    .child(name.to_uppercase()),
                            )
                            .child(div().w(gpui::px(50.)).child(v))
                    })
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_lg().child("Assign the standard array"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(0x8a_90_b0))
                            .child(format!("pool: {pool_s}")),
                    )
                    .child(div().flex().flex_col().children(rows))
            }
            Step::Choice(i) => {
                let opts = self.pending_options();
                let opt = opts.get(i).cloned().unwrap_or_default();
                let cands = opt.candidates();
                let selected = self.draft.choices.get(i).cloned().unwrap_or_default();
                let rows: Vec<_> = cands
                    .iter()
                    .enumerate()
                    .map(|(j, cand)| {
                        let is_sel = selected.contains(&j);
                        let cursor = j == self.sel;
                        div()
                            .id(("cand", j))
                            .px_2()
                            .rounded_sm()
                            .when(cursor, |d| d.bg(gpui::rgb(0x35_35_48)))
                            .on_click(cx.listener(move |this, _, _window, _cx| {
                                this.sel = j;
                            }))
                            .child(format!(
                                "{} {}",
                                if is_sel { "[x]" } else { "[ ]" },
                                if cand.quantity > 1 {
                                    format!("{} ×{}", cand.name, cand.quantity)
                                } else {
                                    cand.name.clone()
                                }
                            ))
                    })
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_lg().child(opt.desc.clone()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(0x8a_90_b0))
                            .child(format!(
                                "choose {} — space toggles, enter confirms",
                                opt.choose
                            )),
                    )
                    .child(div().flex().flex_col().children(rows))
            }
            Step::Confirm => {
                let lines = summary(&self.corpus, &self.draft);
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_lg().child("Ready"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .children(lines.into_iter().map(|l| div().text_sm().child(l))),
                    )
            }
        };

        div()
            .track_focus(&self.focus_handle)
            .key_context(if picker_open {
                crate::picker::PICKER_CTX
            } else if name_active {
                crate::dungeon_view::FIELD_CTX
            } else {
                WIZARD_CTX
            })
            .size_full()
            .flex()
            .gap_3()
            .p_2()
            .bg(gpui::rgb(0x1e_1e_2e))
            .text_color(gpui::rgb(0xcd_d6_f4))
            .on_action(
                cx.listener(|this, _: &WizardNext, window, cx| match this.step() {
                    Step::Name => {
                        let name = this.field.text().to_string();
                        if name.trim().is_empty() {
                            this.say("a name is required", cx);
                            return;
                        }
                        this.draft.name = name;
                        this.advance(window, cx);
                    }
                    Step::Scores => {
                        if this.draft.scores.iter().all(Option::is_some) {
                            this.advance(window, cx);
                        } else {
                            this.say("assign all six scores first", cx);
                        }
                    }
                    Step::Choice(i) => {
                        let opts = this.pending_options();
                        let need = opts.get(i).map(|o| o.choose).unwrap_or(0);
                        let got = this
                            .draft
                            .choices
                            .get(i)
                            .map(|s| s.len() as i64)
                            .unwrap_or(0);
                        if got == need {
                            this.advance(window, cx);
                        } else {
                            this.say(format!("select {need}, currently {got}"), cx);
                        }
                    }
                    Step::Confirm => this.finish(window, cx),
                    Step::Race | Step::Subrace | Step::Class | Step::Background => {
                        if this.picker.is_none() {
                            let kind = match this.step() {
                                Step::Race => PickKind::Race,
                                Step::Subrace => PickKind::Subrace,
                                Step::Class => PickKind::Class,
                                _ => PickKind::Background,
                            };
                            this.open_picker(kind, window, cx);
                        }
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &WizardBack, window, cx| this.back(window, cx)))
            .on_action(cx.listener(|this, _: &WizardUp, _window, cx| {
                if this.step() == Step::Scores {
                    this.sel = this.sel.saturating_sub(1);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &WizardDown, _window, cx| {
                if this.step() == Step::Scores {
                    this.sel = (this.sel + 1).min(5);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &WizardToggle, _window, cx| {
                if matches!(this.step(), Step::Choice(_)) {
                    this.toggle_choice(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &WizardCycleDown, _window, cx| {
                this.cycle_score(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &WizardCycleUp, _window, cx| {
                this.cycle_score(1, cx);
            }))
            .on_action(cx.listener(|this, _: &WizardCancel, window, cx| {
                if let Some(mut cb) = this.on_finish.take() {
                    cb(WizardOutcome::Cancelled, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &FieldSubmit, window, cx| {
                // The name field's enter is the same advance as WizardNext.
                this.on_action_submit(window, cx);
            }))
            .on_action(cx.listener(|this, _: &FieldCancel, _window, cx| {
                this.say("field edit cancelled", cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w(gpui::px(150.))
                    .children(rail),
            )
            .child(div().flex_1().flex().flex_col().gap_2().child(body))
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .px_2()
                    .py_1()
                    .bg(gpui::rgb(0x18_18_24))
                    .text_xs()
                    .text_color(gpui::rgb(0x8a_90_b0))
                    .child(
                        self.status
                            .clone()
                            .unwrap_or_else(|| "new character wizard".into()),
                    ),
            )
            // Pickers paint last, over a dim scrim (Task 4.2 ruling).
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
    }
}

impl WizardView {
    /// Shared enter path for the name step (FieldSubmit == WizardNext).
    fn on_action_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.step() == Step::Name {
            let name = self.field.text().to_string();
            if name.trim().is_empty() {
                self.say("a name is required", cx);
                return;
            }
            self.draft.name = name;
        }
        self.advance(window, cx);
    }
}

/// Human-readable confirm summary (also handy for tests).
fn summary(corpus: &Corpus, draft: &Draft) -> Vec<String> {
    let mut out = vec![format!("Name: {}", draft.name)];
    if let Some(r) = draft.race.as_deref().and_then(|i| corpus.race(i)) {
        let sub = draft
            .subrace
            .as_deref()
            .and_then(|i| corpus.subrace(i))
            .map(|s| format!(" ({})", s.name))
            .unwrap_or_default();
        out.push(format!("Race: {}{sub}", r.name));
    }
    if let Some(c) = draft.class.as_deref().and_then(|i| corpus.class(i)) {
        out.push(format!("Class: {} (d{}, level 1)", c.name, c.hit_die));
    }
    if let Some(b) = draft
        .background
        .as_deref()
        .and_then(|i| corpus.background(i))
    {
        out.push(format!("Background: {}", b.name));
    }
    let scores: Vec<String> = Score::ALL_STR
        .iter()
        .zip(draft.scores.iter())
        .map(|(n, v)| format!("{n} {}", v.unwrap_or(0)))
        .collect();
    out.push(format!("Scores: {}", scores.join(" ")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_corpus() -> Arc<Corpus> {
        use blackletter_types::database::reference::Edition;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../srd/2014/en");
        Arc::new(Corpus::load(&dir, Edition::Y2014).expect("vendored corpus loads"))
    }

    fn wizard_draft() -> Draft {
        let mut d = Draft {
            name: "Testy".into(),
            race: Some("elf".into()),
            subrace: Some("high-elf".into()),
            class: Some("wizard".into()),
            background: Some("acolyte".into()),
            ..Default::default()
        };
        d.scores = [Some(8), Some(15), Some(14), Some(13), Some(12), Some(10)];
        // Wizard has three equipment prompts (quarterstaff/dagger, pouch/
        // focus, scholar/explorer pack) then the skill choice: pick
        // Arcana + History there.
        d.choices = vec![vec![0], vec![0], vec![0], vec![0, 1]];
        d
    }

    #[test]
    fn a_full_draft_builds_a_granted_level_one_character() {
        let c = build_character(&test_corpus(), &wizard_draft()).expect("builds");
        assert_eq!(c.name, "Testy");
        assert_eq!(
            c.classes.classes[0].class,
            blackletter_types::class::ClassType::Wizard
        );
        assert_eq!(
            c.classes.classes[0].level,
            blackletter_types::class::Level(1)
        );
        let race = c.race.clone().expect("race set");
        assert_eq!(race.name, "High Elf");
        assert_eq!(race.ability_bonuses.len(), 2, "elf + high-elf bonuses");
        // HP: d6 max + CON mod (14 => +2) = 8.
        assert_eq!(c.health.max, 8, "hp {}", c.health.max);
        // Background skills + class skill choices are proficiencies.
        assert!(c.skills.proficiencies.contains(Skill::Insight));
        assert!(c.skills.proficiencies.contains(Skill::Religion));
        assert!(c.skills.proficiencies.contains(Skill::Arcana));
        assert!(c.skills.proficiencies.contains(Skill::History));
        // Level-1 wizard features.
        assert!(c.has_grant("Spellcasting: Wizard"));
        assert!(c.has_grant("Arcane Recovery"));
        // Race traits landed as prose grants.
        assert!(c.has_grant("Darkvision") || c.has_grant("Trance"));
        // Equipment: spellbook (fixed) + quarterstaff (chosen).
        assert!(c.inventory.0.contains_key("Spellbook"));
        assert!(c.inventory.0.contains_key("Quarterstaff"));
        assert!(c.inventory.0.contains_key("Component pouch"));
        // Background gold.
        assert_eq!(c.gold, 15);
        // Spell slots exist for a level-1 full caster.
        assert!(c.meters.meters.contains_key("spell_slot_1"));
    }

    #[test]
    fn incomplete_drafts_are_rejected_with_a_reason() {
        let corpus = test_corpus();
        let d = Draft::default();
        let e = build_character(&corpus, &d).unwrap_err().to_string();
        assert!(e.contains("no name"), "{e}");
        let mut d = wizard_draft();
        d.scores[3] = None;
        let e = build_character(&corpus, &d).unwrap_err().to_string();
        assert!(e.contains("six scores"), "{e}");
    }
}
