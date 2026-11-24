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
    sheet::{
        background::Background, race::Race, skills::Skill, source::Source, spells::Spell,
        traits::Trait,
    },
};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyBinding, Render, ScrollHandle, SharedString,
    Window, actions, div, prelude::*,
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
        WizardClearScore,
        WizardCancel
    ]
);

#[must_use]
pub fn wizard_keys() -> Vec<KeyBinding> {
    let ctx = Some(WIZARD_CTX);
    let kb = vec![
        KeyBinding::new("enter", WizardNext, ctx),
        KeyBinding::new("shift-enter", WizardBack, ctx),
        // [ / ] walk steps, matching the sheet's section nav.
        KeyBinding::new("[", WizardBack, ctx),
        KeyBinding::new("]", WizardNext, ctx),
        KeyBinding::new("up", WizardUp, ctx),
        KeyBinding::new("down", WizardDown, ctx),
        // hjkl mirrors the arrows everywhere in the wizard (the rest of
        // the app is hjkl-first; the picker deliberately keeps plain
        // letters for its filter).
        KeyBinding::new("k", WizardUp, ctx),
        KeyBinding::new("j", WizardDown, ctx),
        KeyBinding::new("h", WizardCycleDown, ctx),
        KeyBinding::new("l", WizardCycleUp, ctx),
        KeyBinding::new("space", WizardToggle, ctx),
        KeyBinding::new("left", WizardCycleDown, ctx),
        KeyBinding::new("right", WizardCycleUp, ctx),
        KeyBinding::new("backspace", WizardClearScore, ctx),
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
    /// Starting-spells prompts driven by the class's level-1 row
    /// (cantrips_known / spells_known; the wizard's spellbook six is
    /// prose-only and special-cased in `spell_prompts`).
    SpellChoice(SpellPrompt),
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpellPrompt {
    Cantrips,
    Known,
}

impl SpellPrompt {
    fn label(self) -> &'static str {
        match self {
            SpellPrompt::Cantrips => "cantrip",
            SpellPrompt::Known => "spell",
        }
    }

    /// Spell level the prompt draws from (0 = cantrips).
    fn spell_level(self) -> i64 {
        match self {
            SpellPrompt::Cantrips => 0,
            SpellPrompt::Known => 1,
        }
    }
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
            Step::SpellChoice(SpellPrompt::Cantrips) => "cantrips",
            Step::SpellChoice(SpellPrompt::Known) => "spells",
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
    /// Spell indexes chosen for the starting-spell prompts.
    cantrips: Vec<String>,
    known: Vec<String>,
}

/// The starting-spell prompts a class owes at level 1: (prompt, count).
/// Known-caster counts come from the Levels row; the wizard's spellbook
/// six is SRD prose ("a spellbook containing six 1st-level wizard
/// spells"), not a column, so it is special-cased. Full-list prepared
/// casters (cleric/druid) get no prompt — `build_character` grants their
/// whole 1st-level list per the 2014 prepared rule.
fn spell_prompts(corpus: &Corpus, class_index: &str) -> Vec<(SpellPrompt, usize)> {
    let Some(row) = corpus.level_row(class_index, 1) else {
        return Vec::new();
    };
    let Some(sc) = row.spellcasting.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(n) = sc.cantrips_known.filter(|n| *n > 0) {
        out.push((SpellPrompt::Cantrips, n as usize));
    }
    let known = match sc.spells_known.filter(|n| *n > 0) {
        Some(n) => Some(n as usize),
        None if class_index == "wizard" => Some(6),
        None => None,
    };
    if let Some(n) = known {
        out.push((SpellPrompt::Known, n));
    }
    out
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
    /// Viewport for the step's scrollable row list (short windows).
    scroll: ScrollHandle,
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
            scroll: ScrollHandle::new(),
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
        if let Some(ci) = self.draft.class.as_deref() {
            for (p, _) in spell_prompts(self.corpus.as_ref(), ci) {
                steps.push(Step::SpellChoice(p));
            }
        }
        steps.push(Step::Confirm);
        self.steps = steps;
    }

    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_self(window, cx);
        self.picker = None;
        if self.idx + 1 < self.steps.len() {
            self.idx += 1;
            self.sel = 0;
            self.scroll.scroll_to_top_of_item(0);
            self.enter_step(window, cx);
        }
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_self(window, cx);
        self.picker = None;
        if self.idx > 0 {
            self.idx -= 1;
            self.sel = 0;
            self.scroll.scroll_to_top_of_item(0);
            self.enter_step(window, cx);
        }
    }

    /// Focus the wizard *before* the closing picker entity is released:
    /// gpui clears window focus when the focused entity is dropped, and
    /// that drop lands after the update frame — reclaiming afterwards
    /// loses to it (live-check finding: every keyboard step went dead
    /// after the first picker closed).
    fn focus_self(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
    }

    /// The step's row list becomes a scroll viewport at short window
    /// heights (same pattern as the sheet sections, Task ruling
    /// 2026-11-24). One tracked div per frame: only one step renders.
    fn scrollable(&self, d: gpui::Div) -> gpui::Stateful<gpui::Div> {
        d.flex_1()
            .min_h_0()
            .id("wizard-scroll")
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
    }

    #[cfg(test)] // the scroll regression test's window into the viewport
    pub fn scroll_offset_y(&self) -> f32 {
        self.scroll.offset().y.as_f32()
    }

    /// Side effects on entering a step: picker steps open their picker.
    fn enter_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.step() {
            Step::Race => self.open_picker(PickKind::Race, window, cx),
            Step::Subrace => self.open_picker(PickKind::Subrace, window, cx),
            Step::Class => self.open_picker(PickKind::Class, window, cx),
            Step::Background => self.open_picker(PickKind::Background, window, cx),
            Step::Scores => self.say("↑/↓ pick · ←/→ cycle the array · backspace clears", cx),
            Step::Choice(i) => {
                let opts = self.pending_options();
                match opts.get(i) {
                    Some(o) => self.say(
                        format!("{} — j/k move · space toggles · enter confirms", o.desc),
                        cx,
                    ),
                    None => self.say("choose", cx),
                }
            }
            Step::SpellChoice(p) => self.open_spell_picker(p, window, cx),
            Step::Confirm => self.say("enter creates the character · [ walks back to edit", cx),
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

    /// Spell pickers are searchable one-pick-at-a-time: the class spell
    /// list is too long for the inline toggle list, and the picker's
    /// filter is the fastest way through 200+ rows. Already-chosen
    /// spells are excluded, and the picker reopens until the count is
    /// met.
    fn open_spell_picker(
        &mut self,
        prompt: SpellPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ci) = self.draft.class.clone() else {
            return;
        };
        let chosen: &[String] = match prompt {
            SpellPrompt::Cantrips => &self.draft.cantrips,
            SpellPrompt::Known => &self.draft.known,
        };
        let need = spell_prompts(self.corpus.as_ref(), &ci)
            .into_iter()
            .find(|(p, _)| *p == prompt)
            .map(|(_, n)| n)
            .unwrap_or(0);
        let allowed: Vec<String> = self
            .corpus
            .class_spell_list(&ci, prompt.spell_level())
            .iter()
            .map(|sp| sp.index.clone())
            .filter(|idx| !chosen.contains(idx))
            .collect();
        self.say(
            format!(
                "choose {} {}s — {} of {need} picked · type to search, enter picks",
                need,
                prompt.label(),
                chosen.len()
            ),
            cx,
        );
        self.open_picker(PickKind::Spell, window, cx);
        if let Some(picker) = self.picker.clone() {
            picker.update(cx, |p, _| p.restrict(&allowed));
        }
    }

    fn on_pick(
        &mut self,
        kind: PickKind,
        outcome: PickerOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_self(window, cx);
        self.picker = None;
        match outcome {
            PickerOutcome::Cancelled => {
                self.say("picker closed — enter reopens it", cx);
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
                    self.draft.cantrips.clear();
                    self.draft.known.clear();
                    self.rebuild_steps();
                    self.say("class chosen", cx);
                    self.advance(window, cx);
                }
                PickKind::Background => {
                    self.draft.background = Some(index);
                    self.say("background chosen", cx);
                    self.advance(window, cx);
                }
                PickKind::Spell => {
                    let Step::SpellChoice(p) = self.step() else {
                        self.say("that pick does not belong here", cx);
                        return;
                    };
                    match p {
                        SpellPrompt::Cantrips => self.draft.cantrips.push(index),
                        SpellPrompt::Known => self.draft.known.push(index),
                    }
                    let Some(ci) = self.draft.class.clone() else {
                        return;
                    };
                    let need = spell_prompts(self.corpus.as_ref(), &ci)
                        .into_iter()
                        .find(|(q, _)| *q == p)
                        .map(|(_, n)| n)
                        .unwrap_or(0);
                    let have = match p {
                        SpellPrompt::Cantrips => self.draft.cantrips.len(),
                        SpellPrompt::Known => self.draft.known.len(),
                    };
                    if have >= need {
                        self.say(format!("{}s chosen", p.label()), cx);
                        self.advance(window, cx);
                    } else {
                        // Reopen for the next pick, minus what's taken.
                        self.open_spell_picker(p, window, cx);
                    }
                }
                _ => self.say("that pick does not belong here", cx),
            },
        }
    }

    // ---- scores step ----

    /// The unassigned values of the standard array. The selected row's
    /// own value counts as assigned: `cycle_score` re-adds it exactly
    /// once, so leaving it in the pool too would duplicate it and
    /// right-arrow would cycle the value onto itself forever (live-check
    /// finding: right arrow "does nothing" after any assignment).
    fn score_pool(&self) -> Vec<i64> {
        let mut pool: Vec<i64> = STANDARD_ARRAY.to_vec();
        for v in self.draft.scores.iter() {
            if let Some(v) = *v
                && let Some(p) = pool.iter().position(|x| *x == v)
            {
                pool.remove(p);
            }
        }
        pool.sort_unstable();
        pool
    }

    /// Up/down (and k/j): walk the selected row of the current step.
    fn move_sel(&mut self, dir: isize, cx: &mut Context<Self>) {
        let last = match self.step() {
            Step::Scores => 5,
            Step::Choice(i) => {
                let opts = self.pending_options();
                match opts.get(i) {
                    Some(o) => o.candidates().len().saturating_sub(1),
                    None => return,
                }
            }
            _ => return,
        };
        let next = (self.sel as isize + dir).clamp(0, last as isize) as usize;
        if next != self.sel {
            self.sel = next;
            self.scroll.scroll_to_item(next);
            cx.notify();
        }
    }

    fn cycle_score(&mut self, dir: isize, cx: &mut Context<Self>) {
        if self.step() != Step::Scores {
            return;
        }
        let row = self.sel;
        // The cycle list is the free pool plus the row's own current value.
        let mut values = self.score_pool();
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
                self.score_pool()
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
        let Some(cand) = cands.get(self.sel) else {
            return;
        };
        let granted = granted_indexes(self.corpus.as_ref(), &self.draft);
        if granted.contains(&cand.index) {
            self.say(
                format!("{} is already gained — pick another", cand.name),
                cx,
            );
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

    /// Everything the draft still owes before confirm (the confirm step
    /// lists these and finish refuses until they are all filled).
    fn missing(&self) -> Vec<String> {
        let mut m = Vec::new();
        if self.draft.name.trim().is_empty() {
            m.push("name".into());
        }
        if self.draft.race.is_none() {
            m.push("race".into());
        }
        if self.steps.contains(&Step::Subrace) && self.draft.subrace.is_none() {
            m.push("subrace".into());
        }
        if self.draft.class.is_none() {
            m.push("class".into());
        }
        if self.draft.background.is_none() {
            m.push("background".into());
        }
        if self.draft.scores.iter().any(Option::is_none) {
            m.push("all six ability scores".into());
        }
        for (i, opt) in self.pending_options().iter().enumerate() {
            let have = self.draft.choices.get(i).map(Vec::len).unwrap_or(0);
            let need = usize::try_from(opt.choose).unwrap_or(0);
            if have != need {
                m.push(format!("{} ({have}/{need})", opt.desc));
            }
        }
        if let Some(ci) = self.draft.class.as_deref() {
            for (p, n) in spell_prompts(self.corpus.as_ref(), ci) {
                let have = match p {
                    SpellPrompt::Cantrips => self.draft.cantrips.len(),
                    SpellPrompt::Known => self.draft.known.len(),
                };
                if have != n {
                    m.push(format!("{}s ({have}/{n})", p.label()));
                }
            }
        }
        m
    }

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let missing = self.missing();
        if !missing.is_empty() {
            self.say(
                format!("cannot create yet — still needed: {}", missing.join(", ")),
                cx,
            );
            return;
        }
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

/// Candidate indexes the draft has ALREADY been granted: background and
/// class skill proficiencies, race languages, the class's fixed
/// proficiency references, and items in the class's fixed equipment
/// package. Nothing on the sheet applies twice, so the choice steps
/// cross these off and refuse to toggle them.
fn granted_indexes(corpus: &Corpus, draft: &Draft) -> std::collections::HashSet<String> {
    let mut g = std::collections::HashSet::new();
    if let Some(bi) = draft.background.as_deref()
        && let Some(b) = corpus.background(bi)
    {
        g.extend(b.starting_proficiencies.iter().map(|p| p.index.clone()));
    }
    if let Some(ri) = draft.race.as_deref()
        && let Some(r) = corpus.race(ri)
    {
        g.extend(r.languages.iter().map(|l| l.index.clone()));
    }
    if let Some(ci) = draft.class.as_deref()
        && let Some(c) = corpus.class(ci)
    {
        g.extend(c.proficiencies.iter().map(|p| p.index.clone()));
        g.extend(
            c.starting_equipment
                .iter()
                .map(|e| e.equipment.index.clone()),
        );
    }
    g
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

    // Saving-throw proficiencies: the initial class's only. SRD
    // multiclassing grants NO saving throws when you take a new class
    // (Wiki/03:87 table), so this belongs to creation, not Add Class.
    for st in &class_db.saving_throws {
        // The saving_throws refs point straight at ability scores
        // ("int", "wis"); the class proficiencies list carries the
        // parallel "saving-throw-int" proficiency rows.
        if let Ok(score) = st.index.parse::<Score>() {
            match score {
                Score::Str => c.saving_throws.str = true,
                Score::Dex => c.saving_throws.dex = true,
                Score::Con => c.saving_throws.con = true,
                Score::Int => c.saving_throws.int = true,
                Score::Wis => c.saving_throws.wis = true,
                Score::Cha => c.saving_throws.cha = true,
            }
        }
    }

    // Typed weapon/armor/tool proficiencies from the class row
    // (skill-* and saving-throw-* entries are skipped by add_index).
    for prof in &class_db.proficiencies {
        c.proficiencies.add_index(&prof.index);
    }

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

    // Every class option prompt must be fully answered — the confirm
    // step gates on the same check; belt and braces for direct callers.
    for (i, opt) in class_db.starting_equipment_options.iter().enumerate() {
        let idx = choice_index_for(class_db, i, false);
        let have = draft.choices.get(idx).map(Vec::len).unwrap_or(0);
        let need = usize::try_from(opt.choose).unwrap_or(0);
        anyhow::ensure!(
            have == need,
            "unanswered choice: {} ({have}/{need})",
            opt.desc
        );
    }
    for (i, opt) in class_db.proficiency_choices.iter().enumerate() {
        let idx = choice_index_for(class_db, i, true);
        let have = draft.choices.get(idx).map(Vec::len).unwrap_or(0);
        let need = usize::try_from(opt.choose).unwrap_or(0);
        anyhow::ensure!(
            have == need,
            "unanswered choice: {} ({have}/{need})",
            opt.desc
        );
    }

    // Starting spells (plan Task 5.1). Cantrips are always prepared;
    // known-caster picks land known-and-prepared; the wizard's picks go
    // into the spellbook unprepared (they prepare from it after a long
    // rest, SRD "Preparing and Casting Spells").
    for (p, n) in spell_prompts(corpus, ci) {
        let picked: &[String] = match p {
            SpellPrompt::Cantrips => &draft.cantrips,
            SpellPrompt::Known => &draft.known,
        };
        anyhow::ensure!(
            picked.len() == n,
            "incomplete {} selection ({}/{n})",
            p.label(),
            picked.len()
        );
        let prepared = match p {
            SpellPrompt::Cantrips => true,
            SpellPrompt::Known => ci != "wizard",
        };
        for si in picked {
            let db = corpus.spell(si).ctx_missing("spell", si)?;
            let mut sp = Spell::try_from_database(db.clone(), &c)?;
            sp.prepared = prepared;
            c.spells.spells.insert(sp.name.clone(), sp);
        }
    }
    // Full-list prepared casters (2014 cleric/druid): the whole
    // 1st-level class list lands prepared, no choice step.
    if matches!(ci, "cleric" | "druid") {
        for db in corpus.class_spell_list(ci, 1) {
            let mut sp = Spell::try_from_database(db.clone(), &c)?;
            sp.prepared = true;
            c.spells.spells.insert(sp.name.clone(), sp);
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

/// The name field has no derived state to refresh on text change.
fn after_wiz_name_change(_this: &mut WizardView, _cx: &mut gpui::Context<WizardView>) {}

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
            Step::SpellChoice(p) => {
                if picker_open {
                    div().child("")
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(format!("Pick {}s.", p.label()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(gpui::rgb(0x8a_90_b0))
                                .child("enter reopens the picker"),
                        )
                }
            }
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
                let pool = self.score_pool();
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
                    .flex_1()
                    .min_h_0()
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
                    .child(self.scrollable(div()).children(rows))
            }
            Step::Choice(i) => {
                let opts = self.pending_options();
                let opt = opts.get(i).cloned().unwrap_or_default();
                let cands = opt.candidates();
                let selected = self.draft.choices.get(i).cloned().unwrap_or_default();
                let granted = granted_indexes(self.corpus.as_ref(), &self.draft);
                let rows: Vec<_> = cands
                    .iter()
                    .enumerate()
                    .map(|(j, cand)| {
                        let is_sel = selected.contains(&j);
                        let cursor = j == self.sel;
                        let have = granted.contains(&cand.index);
                        div()
                            .id(("cand", j))
                            .px_2()
                            .rounded_sm()
                            .when(cursor, |d| d.bg(gpui::rgb(0x35_35_48)))
                            .when(have, |d| d.text_color(gpui::rgb(0x5a_5e_78)))
                            .on_click(cx.listener(move |this, _, _window, _cx| {
                                this.sel = j;
                            }))
                            .child(format!(
                                "{} {}{}",
                                if have {
                                    "[✕]"
                                } else if is_sel {
                                    "[x]"
                                } else {
                                    "[ ]"
                                },
                                if cand.quantity > 1 {
                                    format!("{} ×{}", cand.name, cand.quantity)
                                } else {
                                    cand.name.clone()
                                },
                                if have { " — already gained" } else { "" }
                            ))
                    })
                    .collect();
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_lg().child(opt.desc.clone()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(0x8a_90_b0))
                            .child(format!(
                                "choose {} — j/k move · space toggles · enter confirms",
                                opt.choose
                            )),
                    )
                    .child(self.scrollable(div()).children(rows))
            }
            Step::Confirm => {
                let lines = summary(&self.corpus, &self.draft);
                let missing = self.missing();
                let gate = if missing.is_empty() {
                    div()
                } else {
                    div()
                        .text_sm()
                        .text_color(gpui::rgb(0xf3_8b_a8))
                        .child(format!("still needed: {}", missing.join(" · ")))
                };
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_lg().child(if missing.is_empty() {
                        "Ready"
                    } else {
                        "Not ready yet"
                    }))
                    .child(gate)
                    .child(
                        self.scrollable(div())
                            .children(lines.into_iter().map(|l| div().text_sm().child(l))),
                    )
            }
        };

        let base = div()
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
                        } else if got > need {
                            this.say(
                                format!(
                                    "too many: {got} selected, only {need} needed — space unselects"
                                ),
                                cx,
                            );
                        } else {
                            this.say(
                                format!(
                                    "not enough yet: {got} of {need} selected — j/k move, space toggles a row, then enter"
                                ),
                                cx,
                            );
                        }
                    }
                    Step::Confirm => this.finish(window, cx),
                    Step::SpellChoice(p) => {
                        if this.picker.is_none() {
                            this.open_spell_picker(p, window, cx);
                        }
                    }
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
                this.move_sel(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &WizardDown, _window, cx| {
                this.move_sel(1, cx);
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
            .on_action(cx.listener(|this, _: &WizardClearScore, _window, cx| {
                if this.step() == Step::Scores {
                    this.draft.scores[this.sel] = None;
                    this.say(
                        format!(
                            "{} cleared · pool: {}",
                            Score::ALL_STR[this.sel].to_uppercase(),
                            this.score_pool()
                                .iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(" ")
                        ),
                        cx,
                    );
                }
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
            .on_action(cx.listener(|this, _: &crate::field::FieldBackspace, _, cx| {
                this.field_mut().backspace();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldDelete, _, cx| {
                this.field_mut().delete_forward();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldDeleteWordBack, _, cx| {
                this.field_mut().delete_word_back();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldLeft, _, cx| {
                this.field_mut().step_caret(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldRight, _, cx| {
                this.field_mut().step_caret(1);
                cx.notify();
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
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(body),
            )
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
            });

        // Selection/clipboard chords for the name field (FIELD_CTX);
        // Backspace/Delete/Left/Right are wired explicitly above.
        crate::field_key_actions!(base, cx, after_wiz_name_change)
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
        d.cantrips = vec!["fire-bolt".into(), "light".into(), "mage-hand".into()];
        d.known = vec![
            "magic-missile".into(),
            "shield".into(),
            "detect-magic".into(),
            "burning-hands".into(),
            "charm-person".into(),
            "feather-fall".into(),
        ];
        d
    }

    #[test]
    fn granted_indexes_cross_off_background_and_race_gains() {
        let g = granted_indexes(&test_corpus(), &wizard_draft());
        // Acolyte skills — the wizard's skill prompt must not offer them.
        assert!(g.contains("skill-insight") && g.contains("skill-religion"));
        // Elf languages and the class's fixed grants.
        assert!(g.contains("common") && g.contains("elvish"));
        assert!(g.contains("spellbook"), "fixed equipment package");
        assert!(g.contains("saving-throw-int"), "class proficiencies");
        // The pickable skill rows (Arcana/History) are NOT in the set.
        assert!(!g.contains("skill-arcana") && !g.contains("skill-history"));
    }

    #[test]
    fn confirm_refuses_a_draft_with_holes() {
        // Engine-side gate mirrors the confirm step's: every prompt
        // answered or build_character refuses.
        let mut d = wizard_draft();
        d.known.pop();
        let e = build_character(&test_corpus(), &d).expect_err("six spellbook spells required");
        assert!(e.to_string().contains("spell selection"), "{e}");
        let mut d = wizard_draft();
        d.choices[3] = vec![0];
        let e = build_character(&test_corpus(), &d).expect_err("two skill picks required");
        assert!(e.to_string().contains("unanswered choice"), "{e}");
    }

    #[test]
    fn spell_prompts_follow_the_levels_rows() {
        let c = test_corpus();
        assert_eq!(
            spell_prompts(&c, "wizard"),
            vec![(SpellPrompt::Cantrips, 3), (SpellPrompt::Known, 6)]
        );
        assert_eq!(
            spell_prompts(&c, "sorcerer"),
            vec![(SpellPrompt::Cantrips, 4), (SpellPrompt::Known, 2)]
        );
        assert_eq!(
            spell_prompts(&c, "cleric"),
            vec![(SpellPrompt::Cantrips, 3)]
        );
        assert!(spell_prompts(&c, "fighter").is_empty());
        assert!(
            spell_prompts(&c, "ranger").is_empty(),
            "rangers cast at 2nd"
        );
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
        // The class row's spellcasting ability landed on the sheet class.
        assert_eq!(c.classes.classes[0].spellcasting_ability, Some(Score::Int));
        // Wizard proficiencies: no armor, no weapons, hand crossbow
        // (from the fixed equipment package? no — from class row: none).
        // SRD wizard class proficiencies are only the two saves + skills.
        assert!(
            c.proficiencies.armor.is_empty() && c.proficiencies.weapons.is_empty(),
            "wizard gets no armor/weapon proficiencies"
        );
        // Class saving-throw proficiencies landed (wizard: INT + WIS).
        assert!(c.saving_throws.int && c.saving_throws.wis);
        assert!(!c.saving_throws.str);
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
        // Starting spells: 3 cantrips prepared + 6 spellbook entries
        // unprepared (the wizard prepares from the spellbook).
        assert_eq!(
            c.spells.spells.len(),
            9,
            "spellbook: {:?}",
            c.spells.spells.keys()
        );
        assert!(
            c.spells.spells["Fire Bolt"].prepared,
            "cantrips stay prepared"
        );
        assert_eq!(c.spells.spells["Fire Bolt"].level, 0);
        assert!(
            !c.spells.spells["Magic Missile"].prepared,
            "spellbook entries start unprepared"
        );
    }

    /// Drives the wizard exactly like the live check: open via
    /// ctrl-shift-n, pick through the pickers with `enter`, then press
    /// `right` on the scores step. This is the regression test for the
    /// picker-close focus drop and the shadowed right-arrow binding.
    #[gpui::test]
    fn the_full_picker_flow_leaves_the_scores_step_responsive(cx: &mut gpui::TestAppContext) {
        use crate::autosave::Autosave;
        use crate::session::Session;
        cx.update(|cx| {
            cx.bind_keys(crate::root::keybindings());
            cx.bind_keys(crate::sheet_view::keybindings());
            cx.bind_keys(crate::field::field_keys(crate::dungeon_view::FIELD_CTX));
            cx.bind_keys(crate::picker::picker_keys());
            cx.bind_keys(crate::form::form_keys());
            cx.bind_keys(wizard_keys());
            Autosave::init(cx);
        });
        let window = cx.add_window(|_, cx| {
            crate::root::RootView::new(
                cx,
                Session::default(),
                None,
                Some(test_corpus()),
                Library::default(),
                None,
            )
        });
        window
            .update(cx, |view, window, cx| window.focus(view.focus_handle(), cx))
            .unwrap();

        cx.simulate_keystrokes(window.into(), "ctrl-shift-n");
        cx.run_until_parked();
        // Typo + backspace: FIELD_CTX backspace must reach the field
        // (live bug: the wizard never handled FieldBackspace).
        cx.simulate_keystrokes(window.into(), "T e s t y z backspace enter");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "e l f enter");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter"); // first elf subrace
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "w i z enter");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "a c o enter");
        cx.run_until_parked();

        let root = window.root(cx).unwrap();
        let wiz = root
            .read_with(cx, |r, _| r.wizard().cloned())
            .expect("wizard open");
        assert_eq!(
            wiz.read_with(cx, |w, _| w.draft.name.clone()),
            "Testy",
            "backspace deleted the typo'd z"
        );
        let step = wiz.read_with(cx, |w, _| w.step());
        assert_eq!(step, Step::Scores, "flow reached the scores step");

        // The live bug: after the last picker closed, arrow keys died.
        cx.simulate_keystrokes(window.into(), "right");
        cx.run_until_parked();
        let s0 = wiz.read_with(cx, |w, _| w.draft.scores[0]);
        assert!(s0.is_some(), "right arrow assigned STR ({s0:?})");
        cx.simulate_keystrokes(window.into(), "left");
        cx.run_until_parked();
        let s0b = wiz.read_with(cx, |w, _| w.draft.scores[0]);
        assert_ne!(s0, s0b, "left arrow cycled {s0:?} -> {s0b:?}");
        // Regression (live check): right after an assignment must change
        // the value, not cycle a duplicated pool entry onto itself.
        cx.simulate_keystrokes(window.into(), "right right");
        cx.run_until_parked();
        let s0c = wiz.read_with(cx, |w, _| w.draft.scores[0]);
        assert_ne!(s0b, s0c, "right cycled again {s0b:?} -> {s0c:?}");
        // Backspace clears an assigned score (live check: no way to undo).
        cx.simulate_keystrokes(window.into(), "backspace");
        cx.run_until_parked();
        let s0d = wiz.read_with(cx, |w, _| w.draft.scores[0]);
        assert_eq!(s0d, None, "backspace cleared STR ({s0d:?})");
        cx.simulate_keystrokes(window.into(), "down right");
        cx.run_until_parked();
        let s1 = wiz.read_with(cx, |w, _| w.draft.scores[1]);
        assert!(s1.is_some(), "down+right assigned DEX ({s1:?})");

        // Assign the remaining scores (hjkl works the same as arrows)
        // and advance into the first starting-choices step.
        cx.simulate_keystrokes(
            window.into(),
            // STR was cleared by the backspace above: re-assign DEX's
            // neighbors, walk back up, and fill STR last.
            "l h down l down l down l down l up up up up up l enter",
        );
        cx.run_until_parked();
        let step = wiz.read_with(cx, |w, _| w.step());
        assert_eq!(step, Step::Choice(0), "entered the first choice step");

        // Enter with nothing selected must NOT advance (the confusing
        // live case: no feedback about why enter did nothing).
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert_eq!(
            wiz.read_with(cx, |w, _| w.step()),
            Step::Choice(0),
            "insufficient selection holds the step"
        );
        // j moves the candidate cursor, space toggles, enter advances.
        cx.simulate_keystrokes(window.into(), "j space");
        cx.run_until_parked();
        assert_eq!(wiz.read_with(cx, |w, _| w.sel), 1, "j moved the selection");
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert_eq!(wiz.read_with(cx, |w, _| w.step()), Step::Choice(1));
        assert_eq!(
            wiz.read_with(cx, |w, _| w.draft.choices[0].clone()),
            vec![1],
            "space toggled candidate 1"
        );

        // Remaining equipment prompts (1 each), then the skill choice
        // (2 of 5).
        cx.simulate_keystrokes(window.into(), "space enter space enter");
        cx.run_until_parked();
        // Skill choice: Acolyte already grants Insight and Religion,
        // so those rows are crossed off and refuse to toggle.
        cx.simulate_keystrokes(window.into(), "j j space");
        cx.run_until_parked();
        assert!(
            wiz.read_with(cx, |w, _| w.draft.choices[3].is_empty()),
            "background-granted Insight must not be pickable"
        );
        cx.simulate_keystrokes(window.into(), "k k space j space enter");
        cx.run_until_parked();
        assert_eq!(
            wiz.read_with(cx, |w, _| w.draft.choices[3].clone()),
            vec![0, 1],
            "Arcana + History picked instead"
        );
        assert_eq!(
            wiz.read_with(cx, |w, _| w.step()),
            Step::SpellChoice(SpellPrompt::Cantrips)
        );

        // Cantrip picker (3 picks, one searchable picker per pick).
        cx.simulate_keystrokes(window.into(), "f i r e enter l i g h t enter m a g e enter");
        cx.run_until_parked();
        assert_eq!(
            wiz.read_with(cx, |w, _| w.step()),
            Step::SpellChoice(SpellPrompt::Known)
        );

        // Spellbook picker (wizard's six 1st-level spells).
        cx.simulate_keystrokes(
            window.into(),
            "m a g i c enter s h i e l d enter d e t e c t enter              b u r n i n g enter c h a r m enter f e a t h e r enter",
        );
        cx.run_until_parked();
        assert_eq!(wiz.read_with(cx, |w, _| w.step()), Step::Confirm);

        // Confirm creates: the wizard hands the character to the root.
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let gone = root.read_with(cx, |r, _| r.wizard().is_none());
        assert!(gone, "wizard closed after confirm");
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

    /// Live bug (2026-11-24): after closing the wizard early the keyboard
    /// was dead until a mouse click. gpui clears window focus when the
    /// focused entity is dropped, so the sheet must reclaim it before the
    /// wizard is released.
    #[gpui::test]
    fn cancelling_the_wizard_hands_keyboard_focus_back_to_the_sheet(cx: &mut gpui::TestAppContext) {
        use crate::autosave::Autosave;
        use crate::session::Session;
        cx.update(|cx| {
            cx.bind_keys(crate::root::keybindings());
            cx.bind_keys(crate::sheet_view::keybindings());
            cx.bind_keys(crate::field::field_keys(crate::dungeon_view::FIELD_CTX));
            cx.bind_keys(crate::picker::picker_keys());
            cx.bind_keys(crate::form::form_keys());
            cx.bind_keys(wizard_keys());
            Autosave::init(cx);
        });
        let window = cx.add_window(|_, cx| {
            crate::root::RootView::new(
                cx,
                Session::default(),
                None,
                Some(test_corpus()),
                Library::default(),
                None,
            )
        });
        window
            .update(cx, |view, window, cx| window.focus(view.focus_handle(), cx))
            .unwrap();
        cx.simulate_keystrokes(window.into(), "ctrl-shift-n");
        cx.run_until_parked();
        // Name step: escape is the field's cancel, not the wizard's.
        // Walk to the race step (picker open), then escape closes the
        // picker, escape cancels the wizard.
        cx.simulate_keystrokes(window.into(), "T enter");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "escape escape");
        cx.run_until_parked();
        let root = window.root(cx).unwrap();
        assert!(
            root.read_with(cx, |r, _| r.wizard().is_none()),
            "wizard closed"
        );
        // The keyboard must work on the sheet with no mouse click:
        // `]` advances the sheet's section.
        cx.simulate_keystrokes(window.into(), "]");
        cx.run_until_parked();
        let sheet = root
            .read_with(cx, |r, _| r.sheet_view().cloned())
            .expect("sheet view built by the cancel path");
        assert_ne!(
            sheet.read_with(cx, |s, _| s.section()),
            crate::sheet_view::Section::Basic,
            "`]` reached the sheet after the wizard closed"
        );
    }

    /// Scroll ruling (2026-11-24): wizard row lists scroll in short
    /// windows and j/k keep the selection in view, like the sheet.
    #[gpui::test]
    fn the_wizard_row_list_scrolls_in_a_short_window(cx: &mut gpui::TestAppContext) {
        use crate::autosave::Autosave;
        use crate::session::Session;
        cx.update(|cx| {
            cx.bind_keys(crate::root::keybindings());
            cx.bind_keys(crate::sheet_view::keybindings());
            cx.bind_keys(crate::field::field_keys(crate::dungeon_view::FIELD_CTX));
            cx.bind_keys(crate::picker::picker_keys());
            cx.bind_keys(crate::form::form_keys());
            cx.bind_keys(wizard_keys());
            Autosave::init(cx);
        });
        let window = cx.open_window(gpui::size(gpui::px(900.), gpui::px(160.)), |_, cx| {
            crate::root::RootView::new(
                cx,
                Session::default(),
                None,
                Some(test_corpus()),
                Library::default(),
                None,
            )
        });
        window
            .update(cx, |view, window, cx| window.focus(view.focus_handle(), cx))
            .unwrap();
        cx.simulate_keystrokes(window.into(), "ctrl-shift-n");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "T enter");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "e l f enter");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter"); // first elf subrace
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "w i z enter");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "a c o enter");
        cx.run_until_parked();
        let root = window.root(cx).unwrap();
        let wiz = root
            .read_with(cx, |r, _| r.wizard().cloned())
            .expect("wizard open");
        assert_eq!(wiz.read_with(cx, |w, _| w.step()), Step::Scores);
        cx.simulate_keystrokes(window.into(), "j j j j j");
        cx.run_until_parked();
        assert_eq!(wiz.read_with(cx, |w, _| w.sel), 5);
        assert!(
            wiz.read_with(cx, |w, _| w.scroll_offset_y()) < 0.,
            "the score rows scrolled to keep the selection in view"
        );
    }
}
