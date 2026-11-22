//! Custom-entry creation form (plan Task 4.3): the menu for adding custom
//! spells, items, feats, features, and traits to the library.
//!
//! Picker-shaped: its own key context (`Form`) so sheet chords never leak
//! in, a shared `Field` for text entry, and the owner paints it over a
//! scrim. Rows are label/value pairs; enter edits the selected row, enter
//! again commits it, ctrl-s saves the whole form, esc backs out one level
//! at a time. The owner turns the saved pairs into a typed library entry —
//! the form itself stays content-agnostic.

use gpui::{
    App, Context, FocusHandle, Focusable, KeyBinding, Render, SharedString, Window, actions, div,
    prelude::*,
};

use crate::field::{Field, FieldCancel, FieldElement, FieldOwner, FieldSubmit, field_keys};

pub const FORM_CTX: &str = "Form";

actions!(form, [FormUp, FormDown, FormSave]);

/// Keys the form answers: the shared field table (enter = edit/commit,
/// escape = back out) plus row movement and save.
#[must_use]
pub fn form_keys() -> Vec<KeyBinding> {
    let mut kb = field_keys(FORM_CTX);
    let ctx = Some(FORM_CTX);
    kb.push(KeyBinding::new("up", FormUp, ctx));
    kb.push(KeyBinding::new("down", FormDown, ctx));
    kb.push(KeyBinding::new("ctrl-s", FormSave, ctx));
    kb
}

/// The form's answer to the entity that opened it.
pub enum FormOutcome {
    /// Every row as a (label, value) pair, edits committed.
    Saved(Vec<(String, String)>),
    Cancelled,
}

type OutcomeFn = Box<dyn FnMut(FormOutcome, &mut Window, &mut App) + Send>;

struct FormRow {
    label: &'static str,
    value: String,
}

pub struct Form {
    title: String,
    rows: Vec<FormRow>,
    sel: usize,
    /// The row whose value the field is currently editing.
    editing: Option<usize>,
    field: Field,
    focus_handle: FocusHandle,
    on_outcome: Option<OutcomeFn>,
}

impl Form {
    pub fn new(
        title: impl Into<SharedString>,
        specs: &[(&'static str, String)],
        on_outcome: impl FnMut(FormOutcome, &mut Window, &mut App) + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            title: title.into().to_string(),
            rows: specs
                .iter()
                .map(|(label, value)| FormRow {
                    label,
                    value: value.clone(),
                })
                .collect(),
            sel: 0,
            editing: None,
            field: Field::new(""),
            focus_handle: cx.focus_handle(),
            on_outcome: Some(Box::new(on_outcome)),
        }
    }

    /// Write the field back into the row being edited, if any.
    fn commit_edit(&mut self) {
        if let Some(i) = self.editing.take()
            && let Some(row) = self.rows.get_mut(i)
        {
            row.value = self.field.text().to_string();
        }
    }

    fn start_edit(&mut self) {
        if let Some(row) = self.rows.get(self.sel) {
            self.editing = Some(self.sel);
            self.field = Field::new(&row.value);
            self.field.caret_to_end();
        }
    }

    fn move_sel(&mut self, delta: isize) {
        self.commit_edit();
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as isize;
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_edit();
        let Some(mut cb) = self.on_outcome.take() else {
            return;
        };
        let pairs = self
            .rows
            .iter()
            .map(|r| (r.label.to_string(), r.value.clone()))
            .collect();
        cb(FormOutcome::Saved(pairs), window, cx);
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut cb) = self.on_outcome.take() {
            cb(FormOutcome::Cancelled, window, cx);
        }
    }
}

impl FieldOwner for Form {
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

impl gpui::EntityInputHandler for Form {
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

impl Focusable for Form {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Form {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.editing.is_some();
        let rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let selected = i == self.sel;
                let base = div()
                    .id(("form-row", i))
                    .px_2()
                    .flex()
                    .gap_2()
                    .rounded_sm()
                    .when(selected, |d| d.bg(gpui::rgb(0x35_35_48)))
                    .on_click(cx.listener(move |this, _, _window, _cx| {
                        this.commit_edit();
                        this.sel = i;
                    }))
                    .child(
                        div()
                            .w(gpui::px(110.))
                            .text_color(gpui::rgb(0x8a_90_b0))
                            .child(r.label.to_string()),
                    );
                if self.editing == Some(i) {
                    base.child(
                        div()
                            .w(gpui::px(360.))
                            .child(FieldElement::new(cx.entity())),
                    )
                } else {
                    base.child(
                        div()
                            .w(gpui::px(360.))
                            .text_ellipsis()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_color(gpui::rgb(0xba_c2_d9))
                            .child(if r.value.is_empty() {
                                "—".to_string()
                            } else {
                                r.value.clone()
                            }),
                    )
                }
            })
            .collect();
        div()
            .track_focus(&self.focus_handle)
            .key_context(FORM_CTX)
            .w(gpui::px(560.))
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_lg()
            .bg(gpui::rgb(0x1e_1e_2e))
            .border_1()
            .border_color(gpui::rgb(0x58_5b_70))
            .text_color(gpui::rgb(0xcd_d6_f4))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(0x8a_90_b0))
                    .child(format!(
                        "{} — ↑/↓ rows · enter edits, enter commits · ctrl-s saves · esc {}",
                        self.title,
                        if editing { "leaves the row" } else { "cancels" }
                    )),
            )
            .child(div().flex().flex_col().children(rows))
            .on_action(cx.listener(|this, _: &FormUp, _window, cx| {
                this.move_sel(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FormDown, _window, cx| {
                this.move_sel(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FormSave, window, cx| {
                this.save(window, cx);
            }))
            .on_action(cx.listener(|this, _: &FieldSubmit, _window, cx| {
                if this.editing.is_some() {
                    this.commit_edit();
                } else {
                    this.start_edit();
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FieldCancel, window, cx| {
                if this.editing.take().is_some() {
                    cx.notify();
                } else {
                    this.cancel(window, cx);
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Entity;
    use gpui::TestAppContext;

    fn form(cx: &mut TestAppContext) -> (gpui::WindowHandle<Form>, Entity<Form>) {
        cx.update(|cx| cx.bind_keys(form_keys()));
        let window = cx.add_window(|_, cx| {
            Form::new(
                "New custom spell",
                &[
                    ("name", String::new()),
                    ("level", "1".into()),
                    ("desc", String::new()),
                ],
                |_, _, _| {},
                cx,
            )
        });
        let form = window.root(cx).unwrap();
        window
            .update(cx, |f, window, cx| window.focus(&f.focus_handle(cx), cx))
            .unwrap();
        (window, form)
    }

    #[gpui::test]
    fn editing_a_row_commits_into_the_value(cx: &mut TestAppContext) {
        let (window, form) = form(cx);
        // enter starts editing row 0; type; enter commits.
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "Moonfire Bolt");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let rows = cx.read(|cx| {
            form.read(cx)
                .rows
                .iter()
                .map(|r| (r.label, r.value.clone()))
                .collect::<Vec<_>>()
        });
        assert_eq!(rows[0], ("name", "Moonfire Bolt".to_string()));
        assert_eq!(rows[1], ("level", "1".to_string()));
    }

    #[gpui::test]
    fn arrows_move_and_pending_edits_survive(cx: &mut TestAppContext) {
        let (window, form) = form(cx);
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "7");
        cx.run_until_parked();
        // Moving down commits the edit into row 0 and selects row 1.
        cx.simulate_keystrokes(window.into(), "down");
        cx.run_until_parked();
        cx.read(|cx| {
            let f = form.read(cx);
            assert_eq!(f.rows[0].value, "7");
            assert_eq!(f.sel, 1);
            assert!(f.editing.is_none());
        });
    }
}
