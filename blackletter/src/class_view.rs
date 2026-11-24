//! Class detail popup (Task 5.3 live pass): enter on the Basic tab shows
//! everything the selected class has granted so far — level, hit die,
//! subclass, and every class/subclass feature with its full SRD text.
//! Picker-shaped: its own key context (`ClassInfo`) so sheet keys don't
//! leak in; esc closes.

use std::sync::Arc;

use gpui::{Context, Render, ScrollHandle, Window, actions, div, prelude::*};

use blackletter_types::database::corpus::Corpus;
use blackletter_types::sheet::class::Class;

pub const CLASS_CTX: &str = "ClassInfo";

actions!(classinfo, [ClassNext, ClassPrev, CloseClassInfo]);

/// Keys the class-detail popup answers. Without these the popup would
/// be a mouse trap: its context replaces SHEET_CTX while open.
#[must_use]
pub fn class_keys() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let ctx = Some(CLASS_CTX);
    vec![
        KeyBinding::new("j", ClassNext, ctx),
        KeyBinding::new("down", ClassNext, ctx),
        KeyBinding::new("k", ClassPrev, ctx),
        KeyBinding::new("up", ClassPrev, ctx),
        KeyBinding::new("escape", CloseClassInfo, ctx),
    ]
}

pub struct ClassInfoView {
    class: Class,
    /// 0-based position in the class stack; the first class is marked.
    position: usize,
    stack_len: usize,
    corpus: Arc<Corpus>,
    rows: Vec<(String, String)>,
    sel: usize,
    scroll: ScrollHandle,
}

impl ClassInfoView {
    #[must_use]
    pub fn new(class: Class, position: usize, stack_len: usize, corpus: Arc<Corpus>) -> Self {
        let index = class.class.index().to_string();
        let level = class.get_level();
        let mut rows: Vec<(String, String)> = Vec::new();
        for lvl in 1..=level {
            for f in corpus.level_features(&index, i32::try_from(lvl).unwrap_or(i32::MAX)) {
                rows.push((f.name.clone(), f.desc.join("\n")));
            }
        }
        if !class.subclass.is_empty() {
            for row in corpus.levels_for(&index, Some(&class.subclass)) {
                if i64::from(row.level) > level {
                    continue;
                }
                for r in &row.features {
                    if let Some(f) = corpus.feature(&r.index) {
                        rows.push((f.name.clone(), f.desc.join("\n")));
                    }
                }
            }
        }
        Self {
            class,
            position,
            stack_len,
            corpus,
            rows,
            sel: 0,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn move_sel(&mut self, delta: i64) {
        let next = self.sel as i64 + delta;
        self.sel = next.clamp(0, self.rows.len().saturating_sub(1) as i64) as usize;
    }

    #[must_use]
    pub fn sel(&self) -> usize {
        self.sel
    }

    pub fn scroll_handle(&self) -> &gpui::ScrollHandle {
        &self.scroll
    }

    fn header(&self) -> String {
        let cl = &self.class;
        let mut head = format!(
            "{} — level {} ({:?} hit die)",
            cl.class, cl.level.0, cl.hit_dice
        );
        if !cl.subclass.is_empty() {
            let name = self
                .corpus
                .class(&cl.class.index().to_string())
                .and_then(|db| {
                    db.subclasses
                        .iter()
                        .find(|s| s.index == cl.subclass)
                        .map(|s| s.name.clone())
                })
                .unwrap_or_else(|| cl.subclass.clone());
            head.push_str(&format!("   subclass: {name}"));
        }
        if self.stack_len > 1 {
            head.push_str(if self.position == 0 {
                "   [FIRST CLASS]"
            } else {
                "   [multiclass]"
            });
        }
        head
    }
}

impl Render for ClassInfoView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                div()
                    .id(("cls-feat", i))
                    .px_2()
                    .rounded_sm()
                    .when(i == self.sel, |d| {
                        d.bg(gpui::rgb(0x35_35_48))
                            .text_color(gpui::rgb(0xe8_e8_f0))
                    })
                    .when(i != self.sel, |d| d.text_color(gpui::rgb(0x9c_af_c9)))
                    .child(name.clone())
            })
            .collect();
        let desc = self
            .rows
            .get(self.sel)
            .map_or("", |(_, d)| d.as_str())
            .to_string();
        div()
            .w(gpui::px(620.))
            .max_h(gpui::px(560.))
            .bg(gpui::rgb(0x1c_1d_28))
            .border_1()
            .border_color(gpui::rgb(0x4a_4c_66))
            .rounded_md()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_lg().child(self.header()))
            .child(
                div()
                    .id("cls-list")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .gap_0()
                    .children(rows),
            )
            .child(
                div()
                    .id("cls-desc")
                    .max_h(gpui::px(180.))
                    .overflow_y_scroll()
                    .text_sm()
                    .text_color(gpui::rgb(0xba_b2_c8))
                    .child(desc),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(0x6c_70_86))
                    .child("j/k feature   esc close"),
            )
            .into_any_element()
    }
}
