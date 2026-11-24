//! Level-up overlay (plan Task 5.2): one level, step-wizard shaped.
//!
//! The engine (`blackletter_types::sheet::levelup`) computes the plan;
//! this view walks the player through the choices the plan cannot make
//! for them: HP gain (fixed or rolled), the subclass at the class's
//! archetype level, ASI distribution (or the 2014 feat variant), then a
//! confirm. Picker-shaped: its own key context (`LevelUp`) so sheet
//! chords never leak in, and the owner paints it over a scrim. The
//! owner applies the outcome through one undo snapshot.

use gpui::{
    App, Context, FocusHandle, Focusable, KeyBinding, Render, Window, actions, div, prelude::*,
};

use blackletter_types::database::corpus::Corpus;
use blackletter_types::sheet::levelup::{LevelUpPlan, plan_level_up, roll_hp_gain};
use blackletter_types::sheet::{Character, Score};

pub const LEVELUP_CTX: &str = "LevelUp";

actions!(
    levelup,
    [
        LevelUpNext,
        LevelUpUp,
        LevelUpDown,
        LevelUpRoll,
        LevelUpFeatToggle,
        LevelUpBump,
        LevelUpUnbump,
        LevelUpCancel
    ]
);

/// Keys the overlay answers. `+`/`-` here are ASI bumps — the sheet's
/// section-aware adjusters live in a different key context.
#[must_use]
pub fn levelup_keys() -> Vec<KeyBinding> {
    let ctx = Some(LEVELUP_CTX);
    vec![
        KeyBinding::new("enter", LevelUpNext, ctx),
        KeyBinding::new("k", LevelUpUp, ctx),
        KeyBinding::new("up", LevelUpUp, ctx),
        KeyBinding::new("j", LevelUpDown, ctx),
        KeyBinding::new("down", LevelUpDown, ctx),
        KeyBinding::new("r", LevelUpRoll, ctx),
        KeyBinding::new("f", LevelUpFeatToggle, ctx),
        KeyBinding::new("+", LevelUpBump, ctx),
        KeyBinding::new("=", LevelUpBump, ctx),
        KeyBinding::new("-", LevelUpUnbump, ctx),
        KeyBinding::new("_", LevelUpUnbump, ctx),
        KeyBinding::new("escape", LevelUpCancel, ctx),
    ]
}

/// The overlay's answer to the sheet that opened it.
pub enum LevelUpOutcome {
    Apply {
        hp_gain: i64,
        asi: Vec<(Score, i64)>,
        subclass: Option<(String, String)>,
        feat_instead_of_asi: bool,
        /// True when this level granted an ASI that the user neither
        /// spent nor swapped for a feat — the sheet re-offers it via the
        /// feat picker's ASI row so it can't be silently lost.
        asi_offered: bool,
    },
    Cancelled,
}

type OutcomeFn = Box<dyn FnMut(LevelUpOutcome, &mut Window, &mut App) + Send>;

/// The steps this plan needs; HP and Confirm are always present.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Hp,
    Subclass,
    Asi,
    Confirm,
}

const SCORE_LABELS: [&str; 6] = ["STR", "DEX", "CON", "INT", "WIS", "CHA"];
const SCORE_KEYS: [Score; 6] = [
    Score::Str,
    Score::Dex,
    Score::Con,
    Score::Int,
    Score::Wis,
    Score::Cha,
];

pub struct LevelUpView {
    plan: LevelUpPlan,
    /// e.g. "fighter 1" — the class row this level-up belongs to.
    label: String,
    steps: Vec<Step>,
    step: usize,
    /// HP step: rolled value (die, total); `use_roll` picks it over fixed.
    rolled: Option<(i64, i64)>,
    use_roll: bool,
    /// Subclass step: selection into `plan.subclass_options`.
    sub_sel: usize,
    /// ASI step: +1 per point onto the six bases; `feat_instead` swaps
    /// the whole ASI for a feat (2014 variant rule).
    bases: [i64; 6],
    alloc: [i64; 6],
    asi_sel: usize,
    feat_instead: bool,
    focus_handle: FocusHandle,
    on_outcome: Option<OutcomeFn>,
}

impl LevelUpView {
    /// The plan for `class_pos`'s next level, or `None` at level 20 or
    /// when the class has no corpus level rows.
    #[must_use]
    pub fn plan_for(
        corpus: &Corpus,
        c: &Character,
        class_pos: usize,
    ) -> Option<(LevelUpPlan, i64)> {
        let cl = c.classes.classes.get(class_pos)?;
        let con_mod = c.ability_scores.con.modifier();
        let plan = plan_level_up(
            corpus,
            &cl.class,
            cl.get_level(),
            Some(&cl.subclass),
            con_mod,
        )?;
        Some((plan, con_mod))
    }

    pub fn new(
        plan: LevelUpPlan,
        label: String,
        bases: [i64; 6],
        on_outcome: impl FnMut(LevelUpOutcome, &mut Window, &mut App) + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut steps = vec![Step::Hp];
        if !plan.subclass_options.is_empty() {
            steps.push(Step::Subclass);
        }
        if plan.asi_points > 0 {
            steps.push(Step::Asi);
        }
        steps.push(Step::Confirm);
        Self {
            plan,
            label,
            steps,
            step: 0,
            rolled: None,
            use_roll: false,
            sub_sel: 0,
            bases,
            alloc: [0; 6],
            asi_sel: 0,
            feat_instead: false,
            focus_handle: cx.focus_handle(),
            on_outcome: Some(Box::new(on_outcome)),
        }
    }

    fn hp_gain(&self) -> i64 {
        match (self.use_roll, self.rolled) {
            (true, Some((_, total))) => total,
            _ => self.plan.hp_fixed,
        }
    }

    fn asi_spent(&self) -> i64 {
        self.alloc.iter().sum()
    }

    fn roll(&mut self) {
        self.rolled = Some(roll_hp_gain(&self.plan));
        self.use_roll = true;
    }

    fn bump(&mut self) {
        if self.feat_instead || self.asi_spent() >= i64::from(self.plan.asi_points) {
            return;
        }
        let i = self.asi_sel;
        if self.bases[i] + self.alloc[i] < 30 {
            self.alloc[i] += 1;
        }
    }

    fn unbump(&mut self) {
        let i = self.asi_sel;
        self.alloc[i] = self.alloc[i].saturating_sub(1);
    }

    fn toggle_feat(&mut self) {
        self.feat_instead = !self.feat_instead;
        if self.feat_instead {
            self.alloc = [0; 6];
        }
    }

    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.step + 1 < self.steps.len() {
            self.step += 1;
            cx.notify();
            return;
        }
        let Some(mut cb) = self.on_outcome.take() else {
            return;
        };
        let subclass = if self.steps.contains(&Step::Subclass) {
            self.plan.subclass_options.get(self.sub_sel).cloned()
        } else {
            None
        };
        let asi: Vec<(Score, i64)> = if self.feat_instead {
            Vec::new()
        } else {
            self.alloc
                .iter()
                .enumerate()
                .filter(|(_, n)| **n > 0)
                .map(|(i, n)| (SCORE_KEYS[i], *n))
                .collect()
        };
        cb(
            LevelUpOutcome::Apply {
                hp_gain: self.hp_gain(),
                asi: asi.clone(),
                subclass,
                feat_instead_of_asi: self.feat_instead,
                asi_offered: self.plan.asi_points > 0 && !self.feat_instead && asi.is_empty(),
            },
            window,
            cx,
        );
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut cb) = self.on_outcome.take() {
            cb(LevelUpOutcome::Cancelled, window, cx);
        }
    }

    fn move_sel(&mut self, delta: isize) {
        match self.steps[self.step] {
            Step::Subclass => {
                let n = self.plan.subclass_options.len().max(1) as isize;
                self.sub_sel = (self.sub_sel as isize + delta).rem_euclid(n) as usize;
            }
            Step::Asi => {
                self.asi_sel = (self.asi_sel as isize + delta).rem_euclid(6) as usize;
            }
            _ => {}
        }
    }

    fn step_name(step: Step) -> &'static str {
        match step {
            Step::Hp => "hit points",
            Step::Subclass => "subclass",
            Step::Asi => "ability boost",
            Step::Confirm => "confirm",
        }
    }
}

impl Focusable for LevelUpView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for LevelUpView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let here = self.steps[self.step];
        let rail: Vec<_> = self
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                div()
                    .text_color(if i == self.step {
                        gpui::rgb(0xcd_d6_f4)
                    } else {
                        gpui::rgb(0x58_5b_70)
                    })
                    .child(Self::step_name(*s).to_string())
            })
            .collect();

        let body = match here {
            Step::Hp => {
                let roll_line = match self.rolled {
                    Some((die, total)) => {
                        format!(
                            "rolled d{}: {die} -> +{total} (f reverts to fixed)",
                            self.plan.hit_die_max
                        )
                    }
                    None => format!("r rolls the d{} + CON", self.plan.hit_die_max),
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!(
                        "HP gain: fixed +{} {}",
                        self.plan.hp_fixed,
                        if self.use_roll { "" } else { "(current)" }
                    ))
                    .child(format!(
                        "{} {}",
                        roll_line,
                        if self.use_roll { "(current)" } else { "" }
                    ))
                    .child("enter continues")
            }
            Step::Subclass => {
                let rows: Vec<_> = self
                    .plan
                    .subclass_options
                    .iter()
                    .enumerate()
                    .map(|(i, (_, name))| {
                        let text = if i == self.sub_sel {
                            format!("> {name}")
                        } else {
                            format!("  {name}")
                        };
                        div()
                            .id(("sub-row", i))
                            .px_2()
                            .rounded_sm()
                            .when(i == self.sub_sel, |d| d.bg(gpui::rgb(0x35_35_48)))
                            .when(i != self.sub_sel, |d| d.text_color(gpui::rgb(0x8a_90_b0)))
                            .child(text)
                    })
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child("choose the archetype (j/k move):")
                    .child(div().flex().flex_col().children(rows))
                    .child("enter continues")
            }
            Step::Asi => {
                let rows: Vec<_> = SCORE_LABELS
                    .iter()
                    .enumerate()
                    .map(|(i, label)| {
                        let bumped = self.alloc[i] > 0;
                        let text = format!("{label} {}", self.bases[i]);
                        div()
                            .id(("asi-row", i))
                            .px_2()
                            .rounded_sm()
                            .flex()
                            .gap_1()
                            .when(i == self.asi_sel, |d| d.bg(gpui::rgb(0x35_35_48)))
                            .when(i != self.asi_sel, |d| d.text_color(gpui::rgb(0x8a_90_b0)))
                            .child(text)
                            // Changed scores read in the highlight color,
                            // matching the standalone ASI menu.
                            .when(bumped, |d| {
                                d.child(
                                    div()
                                        .text_color(gpui::rgb(0x8f_c8_6a))
                                        .child(format!("(+{})", self.alloc[i])),
                                )
                            })
                    })
                    .collect();
                let left = i64::from(self.plan.asi_points) - self.asi_spent();
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(if self.feat_instead {
                        "FEAT instead of ASI (f switches back)".to_string()
                    } else {
                        format!("+/- adjust · {left} point(s) left · f takes a feat instead")
                    })
                    .child(div().flex().flex_col().children(rows))
                    .child("enter continues")
            }
            Step::Confirm => {
                let mut lines = vec![
                    format!("{} -> level {}", self.label, self.plan.new_level),
                    format!("HP +{}", self.hp_gain()),
                ];
                if self.steps.contains(&Step::Subclass)
                    && let Some((_, name)) = self.plan.subclass_options.get(self.sub_sel)
                {
                    lines.push(format!("subclass: {name}"));
                }
                if self.feat_instead {
                    lines.push("ASI -> feat (picked after)".into());
                } else {
                    for (i, n) in self.alloc.iter().enumerate() {
                        if *n > 0 {
                            lines.push(format!("{} +{}", SCORE_LABELS[i], n));
                        }
                    }
                }
                if !self.plan.feature_names.is_empty() {
                    lines.push(format!("grants: {}", self.plan.feature_names.join(", ")));
                }
                let known = self.plan.spells_known_gain + self.plan.cantrips_known_gain;
                if known > 0 {
                    lines.push(format!(
                        "spells/cantrips known +{known} — pick them in the Spells section"
                    ));
                }
                lines.push("enter applies · esc cancels".into());
                div().flex().flex_col().gap_1().children(lines)
            }
        };

        div()
            .track_focus(&self.focus_handle)
            .key_context(LEVELUP_CTX)
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
                        "LEVEL UP {} — {}",
                        self.label,
                        Self::step_name(here)
                    )),
            )
            .child(div().flex().gap_2().text_xs().children(rail))
            .child(body)
            .on_action(cx.listener(|this, _: &LevelUpNext, window, cx| {
                this.advance(window, cx);
            }))
            .on_action(cx.listener(|this, _: &LevelUpUp, _window, cx| {
                this.move_sel(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &LevelUpDown, _window, cx| {
                this.move_sel(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &LevelUpRoll, _window, cx| {
                this.roll();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &LevelUpFeatToggle, _window, cx| {
                // `f` is step-aware: on the HP step it means "fixed"
                // (undo a misclicked roll); only the ASI step treats it
                // as the feat-swap toggle. Without this, pressing f at
                // the HP step silently armed a feat and opened the feat
                // picker after confirm — even at levels with no ASI.
                match this.steps[this.step] {
                    Step::Hp => this.use_roll = false,
                    Step::Asi => this.toggle_feat(),
                    _ => {}
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &LevelUpBump, _window, cx| {
                this.bump();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &LevelUpUnbump, _window, cx| {
                this.unbump();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &LevelUpCancel, window, cx| {
                this.cancel(window, cx);
            }))
    }
}
