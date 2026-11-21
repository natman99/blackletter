//! Sheet mode: the editable character document + three-column layout.
//!
//! Layout (plan Task 3.1): left = section nav, center = the section form,
//! right = an always-visible summary card. The view never recomputes
//! modifiers — every number on screen comes from the engine's derived
//! getters (`armor_class_total()`, `skill_check()`, `passive_senses()`, …).

mod doc;

use std::path::PathBuf;

use blackletter_types::Score;
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
        "tab" => NextSection,
        "shift-tab" => PrevSection,
        "j" => SelectClassDown,
        "k" => SelectClassUp,
        "plus" => LevelUp,
        "equal" => LevelUp,
        "minus" => LevelDown,
        "enter" => EditName,
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
}

pub struct SheetView {
    doc: SheetDoc,
    /// The campaign's `characters/` dir for first-saves of new sheets.
    characters_dir: Option<PathBuf>,
    root: WeakEntity<RootView>,
    section: Section,
    focus_handle: FocusHandle,
    /// Index of the class the +/- keys act on (multiclass editing).
    class_sel: usize,
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
        cx.notify();
    }

    fn on_prev_section(&mut self, _: &PrevSection, _: &mut Window, cx: &mut Context<Self>) {
        self.section = self.section.prev();
        cx.notify();
    }

    fn on_select_class_up(&mut self, _: &SelectClassUp, _: &mut Window, cx: &mut Context<Self>) {
        if !self.doc.character.classes.classes.is_empty() {
            self.class_sel = self.class_sel.saturating_sub(1);
            cx.notify();
        }
    }

    fn on_select_class_down(
        &mut self,
        _: &SelectClassDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let n = self.doc.character.classes.classes.len();
        if n > 0 {
            self.class_sel = (self.class_sel + 1).min(n - 1);
            cx.notify();
        }
    }

    /// Level the selected class. SRD bounds 1..=20; every level change
    /// re-derives proficiency bonus and spell slots (engine sync).
    fn on_level_up(&mut self, _: &LevelUp, _: &mut Window, cx: &mut Context<Self>) {
        self.change_level(1, cx);
    }

    fn on_level_down(&mut self, _: &LevelDown, _: &mut Window, cx: &mut Context<Self>) {
        self.change_level(-1, cx);
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
                        cx.notify();
                    }))
                    .child(s.label())
            }))
            .child(
                div()
                    .mt_2()
                    .text_xs()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("tab: sections\nj/k: class  +/-: level\nenter: name  ctrl-s: save"),
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
            Section::Abilities => self.abilities_section(),
            other => div()
                .p_2()
                .text_sm()
                .text_color(gpui::rgb(0x6c_70_86))
                .child(format!(
                    "{} — interactive editing lands in Task 3.2",
                    other.label()
                ))
                .into_any_element(),
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

    fn abilities_section(&self) -> AnyElement {
        let c = &self.doc.character;
        let scores = [
            ("Strength", Score::Str, c.ability_scores.str.base),
            ("Dexterity", Score::Dex, c.ability_scores.dex.base),
            ("Constitution", Score::Con, c.ability_scores.con.base),
            ("Intelligence", Score::Int, c.ability_scores.int.base),
            ("Wisdom", Score::Wis, c.ability_scores.wis.base),
            ("Charisma", Score::Cha, c.ability_scores.cha.base),
        ];
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .children(scores.into_iter().map(|(label, score, base)| {
                div()
                    .flex()
                    .gap_2()
                    .child(div().w(gpui::px(120.)).child(label))
                    .child(format!("{base} ({:+})", c.ability_modifier_for(score)))
            }))
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("score editing lands in Task 3.2"),
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
            .child(self.nav(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
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
    fn tab_walks_sections(cx: &mut TestAppContext) {
        let (window, root) = open_sheet(cx);
        let view = sheet(cx, &root);
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Basic);
        cx.simulate_keystrokes(window.into(), "tab tab");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Skills);
        cx.simulate_keystrokes(window.into(), "shift-tab");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).section()), Section::Abilities);
    }
}
