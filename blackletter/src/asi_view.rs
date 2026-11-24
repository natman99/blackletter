//! Standalone Ability Score Improvement menu (Task 5.3 live pass):
//! distribute +2 across the six scores, PHB rules — at most +2 to any
//! one score, total +2, scores capped at 20. Every score is shown as a
//! row (no "type a stat name" field); bumped scores render as
//! `15 → 17` in a highlight color, and the header counts the points
//! left to spend — the same budget readout the creation wizard's
//! point-buy should reuse later.

use gpui::{App, Context, FocusHandle, Focusable, Render, Window, actions, div, prelude::*};

use blackletter_types::sheet::{AbilityScores, Score};

pub const ASI_CTX: &str = "Asi";

actions!(
    asi,
    [
        AsiNext, AsiPrev, AsiInc, AsiDec, AsiReset, ConfirmAsi, CloseAsi
    ]
);

/// Keys the ASI menu answers (standard sheet hotkeys).
#[must_use]
pub fn asi_keys() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let ctx = Some(ASI_CTX);
    vec![
        KeyBinding::new("j", AsiNext, ctx),
        KeyBinding::new("down", AsiNext, ctx),
        KeyBinding::new("k", AsiPrev, ctx),
        KeyBinding::new("up", AsiPrev, ctx),
        KeyBinding::new("+", AsiInc, ctx),
        KeyBinding::new("=", AsiInc, ctx),
        KeyBinding::new("-", AsiDec, ctx),
        KeyBinding::new("_", AsiDec, ctx),
        KeyBinding::new("r", AsiReset, ctx),
        KeyBinding::new("enter", ConfirmAsi, ctx),
        KeyBinding::new("escape", CloseAsi, ctx),
    ]
}

/// The menu's answer to the sheet that opened it.
pub enum AsiOutcome {
    Apply(Vec<(Score, i64)>),
    Cancelled,
}

type ChoiceFn = Box<dyn FnMut(AsiOutcome, &mut Window, &mut App) + Send>;

pub struct AsiView {
    /// Focused while open so its actions dispatch inside the entity
    /// (the picker's pattern: a sheet-level listener would nest a
    /// SheetView update inside the callback and panic).
    pub focus_handle: FocusHandle,
    /// Current totals (base + bonus), in `Score::ALL` order.
    totals: [i64; 6],
    /// Points allocated per score, same order.
    delta: [i64; 6],
    sel: usize,
    budget: i64,
    on_choice: Option<ChoiceFn>,
}

impl AsiView {
    pub fn new(
        scores: &AbilityScores,
        budget: i64,
        on_choice: impl FnMut(AsiOutcome, &mut Window, &mut App) + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let totals = std::array::from_fn(|i| scores.iter()[i].get());
        Self {
            focus_handle: cx.focus_handle(),
            totals,
            delta: [0; 6],
            sel: 0,
            budget,
            on_choice: Some(Box::new(on_choice)),
        }
    }

    #[must_use]
    pub fn left(&self) -> i64 {
        self.budget - self.delta.iter().sum::<i64>()
    }

    #[must_use]
    pub fn deltas(&self) -> Vec<(Score, i64)> {
        Score::ALL
            .iter()
            .copied()
            .enumerate()
            .filter(|(i, _)| self.delta[*i] > 0)
            .map(|(i, sc)| (sc, self.delta[i]))
            .collect()
    }

    pub fn move_sel(&mut self, delta: i64) {
        self.sel = (self.sel as i64 + delta).rem_euclid(6) as usize;
    }

    /// PHB ASI: at most +2 to one score, +2 total, cap 20.
    pub fn inc(&mut self) {
        if self.left() <= 0 || self.delta[self.sel] >= 2 {
            return;
        }
        if self.totals[self.sel] + self.delta[self.sel] >= 20 {
            return;
        }
        self.delta[self.sel] += 1;
    }

    pub fn dec(&mut self) {
        if self.delta[self.sel] > 0 {
            self.delta[self.sel] -= 1;
        }
    }

    pub fn reset(&mut self) {
        self.delta = [0; 6];
        self.sel = 0;
    }

    pub fn finish(&mut self, apply: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut cb) = self.on_choice.take() else {
            return;
        };
        let outcome = if apply && !self.deltas().is_empty() {
            AsiOutcome::Apply(self.deltas())
        } else {
            AsiOutcome::Cancelled
        };
        cb(outcome, window, cx);
    }
}

impl Focusable for AsiView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AsiView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bumped = gpui::rgb(0x8f_c8_6a);
        let rows: Vec<_> = Score::ALL
            .iter()
            .enumerate()
            .map(|(i, sc)| {
                let total = self.totals[i];
                let d = self.delta[i];
                let label = format!("{sc:?}");
                let value = if d > 0 {
                    // Changed scores render whole in the highlight color.
                    div()
                        .text_color(bumped)
                        .child(format!("{total} \u{2192} {}", total + d))
                } else {
                    div().child(format!("{total}"))
                };
                div()
                    .id(("asi-score", i))
                    .px_2()
                    .rounded_sm()
                    .flex()
                    .justify_between()
                    .when(i == self.sel, |r| {
                        r.bg(gpui::rgb(0x35_35_48))
                            .text_color(gpui::rgb(0xe8_e8_f0))
                    })
                    .when(i != self.sel, |r| r.text_color(gpui::rgb(0x9c_af_c9)))
                    .child(label)
                    .child(value)
            })
            .collect();
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &AsiNext, _, cx| {
                this.move_sel(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &AsiPrev, _, cx| {
                this.move_sel(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &AsiInc, _, cx| {
                this.inc();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &AsiDec, _, cx| {
                this.dec();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &AsiReset, _, cx| {
                this.reset();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ConfirmAsi, window, cx| {
                this.finish(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &CloseAsi, window, cx| {
                this.finish(false, window, cx);
            }))
            .w(gpui::px(420.))
            .bg(gpui::rgb(0x1c_1d_28))
            .border_1()
            .border_color(gpui::rgb(0x4a_4c_66))
            .rounded_md()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .child(format!("ability score improvement — {} left", self.left())),
            )
            .child(div().flex().flex_col().gap_0().children(rows))
            .child(div().text_xs().text_color(gpui::rgb(0x6c_70_86)).child(
                "j/k score   +/- bump (max +2, cap 20)   r reset   enter apply   esc cancel",
            ))
            .into_any_element()
    }
}
