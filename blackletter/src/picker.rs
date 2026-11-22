//! SRD picker overlay (plan Task 4.2): one searchable list component,
//! many uses — add spell, add item, add feat, set race, add feature/trait.
//!
//! The picker is its own entity with its own key context (`Picker`), so the
//! sheet's single-letter chords never fire while it is open: the field's
//! input handler owns printable keys, and only non-printable arrows move the
//! selection. The owner opens it with a callback that receives the chosen
//! corpus index (or a cancel), then dismisses it.

use std::sync::Arc;

use blackletter_types::{database::corpus::Corpus, library::Library};
use gpui::{
    App, Context, FocusHandle, Focusable, KeyBinding, Render, SharedString, Window, actions, div,
    prelude::*,
};

use crate::field::{Field, FieldElement, FieldOwner, field_keys};

pub const PICKER_CTX: &str = "Picker";

actions!(picker, [PickerUp, PickerDown]);

/// Keys the picker answers: the shared field table (enter = confirm,
/// escape = cancel) plus arrow movement. NB: no j/k here — printable keys
/// go to the field's input handler, never to a keybinding, so j/k would
/// type letters instead of moving.
pub fn picker_keys() -> Vec<KeyBinding> {
    let mut kb = field_keys(PICKER_CTX);
    let ctx = Some(PICKER_CTX);
    kb.push(KeyBinding::new("up", PickerUp, ctx));
    kb.push(KeyBinding::new("down", PickerDown, ctx));
    kb
}

/// What the picker lists. Every kind maps onto one corpus table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    Spell,
    Item,
    Feat,
    Race,
    Trait,
    Feature,
    /// Wizard-only kinds (plan Task 5.1): the sheet has no class/
    /// background picker, the creation flow does.
    Class,
    Background,
    Subrace,
}

impl PickKind {
    pub fn label(self) -> &'static str {
        match self {
            PickKind::Spell => "spell",
            PickKind::Item => "item",
            PickKind::Feat => "feat",
            PickKind::Race => "race",
            PickKind::Trait => "trait",
            PickKind::Feature => "feature",
            PickKind::Class => "class",
            PickKind::Background => "background",
            PickKind::Subrace => "subrace",
        }
    }
}

/// The picker's answer to the entity that opened it.
pub enum PickerOutcome {
    /// The chosen entry's globally-unique SRD index.
    Chosen(String),
    Cancelled,
}

/// One candidate row: display label, detail text, and the lowercase
/// haystack the filter matches against.
#[derive(Clone)]
struct PickRow {
    index: String,
    label: String,
    detail: String,
    hay: String,
}

/// The owner's callback: receives the outcome once, then the picker dies.
type ChoiceFn = Box<dyn FnMut(PickerOutcome, &mut Window, &mut App) + Send>;

/// Rows visible at once; the window scrolls to keep the selection in view.
const VISIBLE: usize = 12;

pub struct Picker {
    kind: PickKind,
    corpus: Arc<Corpus>,
    /// Every row for the kind, in corpus order (filter input for `rows`).
    all: Vec<PickRow>,
    /// The filtered view of `all`; selection indexes this.
    rows: Vec<PickRow>,
    sel: usize,
    scroll: usize,
    field: Field,
    focus_handle: FocusHandle,
    on_choice: Option<ChoiceFn>,
}

impl Picker {
    pub fn new(
        kind: PickKind,
        corpus: Arc<Corpus>,
        library: Option<&Library>,
        on_choice: impl FnMut(PickerOutcome, &mut Window, &mut App) + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let all = rows_for(&corpus, library, kind);
        let rows = all.clone();
        let mut field = Field::new("");
        field.caret_to_end();
        let mut me = Self {
            kind,
            corpus,
            all,
            rows,
            sel: 0,
            scroll: 0,
            field,
            focus_handle: cx.focus_handle(),
            on_choice: Some(Box::new(on_choice)),
        };
        me.refilter();
        me
    }

    #[cfg(test)]
    pub fn row_indexes(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.index.clone()).collect()
    }

    /// Re-run the filter over `all`. Tokens must all appear in the row's
    /// haystack (name + index, hyphens folded to spaces, so "water walk"
    /// finds `water-walk`); name-prefix matches sort first.
    fn refilter(&mut self) {
        let q = self.field.text().trim().to_lowercase();
        let name = q.replace('-', " ");
        self.rows = self
            .all
            .iter()
            .filter(|r| q.split_whitespace().all(|t| r.hay.contains(t)))
            .cloned()
            .collect();
        // Stable sort by match quality: name-prefix, name-contains, else.
        let rank = |r: &PickRow| {
            let n = r.label.to_lowercase();
            if n.starts_with(&name) {
                0
            } else if n.contains(&name) {
                1
            } else {
                2
            }
        };
        self.rows.sort_by_key(|r| rank(r));
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
        self.scroll = self.scroll.min(self.sel);
        if self.sel >= self.scroll + VISIBLE {
            self.scroll = self.sel - VISIBLE + 1;
        }
    }

    fn move_sel(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as isize;
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
        if self.sel < self.scroll {
            self.scroll = self.sel;
        }
        if self.sel >= self.scroll + VISIBLE {
            self.scroll = self.sel - VISIBLE + 1;
        }
    }

    /// Keep only rows whose index is in `indexes` (e.g. the wizard's
    /// subrace picker showing only the chosen race's subraces).
    pub fn restrict(&mut self, indexes: &[String]) {
        self.all.retain(|r| indexes.contains(&r.index));
        self.refilter();
        self.sel = 0;
        self.scroll = 0;
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = self.rows.get(self.sel).map(|r| r.index.clone());
        let Some(mut cb) = self.on_choice.take() else {
            return;
        };
        match chosen {
            Some(index) => cb(PickerOutcome::Chosen(index), window, cx),
            None => cb(PickerOutcome::Cancelled, window, cx),
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut cb) = self.on_choice.take() {
            cb(PickerOutcome::Cancelled, window, cx);
        }
    }
}

/// Build every row for a kind from the corpus. Edition-tagged: the corpus
/// is loaded per-edition and the header shows which one is on screen.
fn rows_for(corpus: &Corpus, library: Option<&Library>, kind: PickKind) -> Vec<PickRow> {
    let mut out = srd_rows(corpus, kind);
    // Custom entries ride the same rows, tagged and prefixed (Task 4.3):
    // a library spell *is* a sheet::Spell, so apply needs no conversion.
    if let Some(lib) = library {
        out.extend(custom_rows(lib, kind));
    }
    out
}

fn srd_rows(corpus: &Corpus, kind: PickKind) -> Vec<PickRow> {
    let row = |index: String, label: String, detail: String| PickRow {
        hay: format!("{} {}", label.to_lowercase(), index.replace('-', " ")),
        index,
        label,
        detail,
    };
    let join = |v: &[String]| v.join("\n\n");
    match kind {
        PickKind::Spell => corpus
            .spells
            .iter()
            .map(|s| {
                let mut detail = join(&s.desc);
                let meta = format!(
                    "Range: {} · Casting: {} · Duration: {} · Components: {}{}{}",
                    s.range,
                    s.casting_time,
                    s.duration,
                    s.components.join(", "),
                    if s.concentration {
                        " · concentration"
                    } else {
                        ""
                    },
                    if s.ritual { " · ritual" } else { "" },
                );
                if !detail.is_empty() {
                    detail.push_str("\n\n");
                }
                detail.push_str(&meta);
                row(
                    s.index.clone(),
                    format!("{} — L{} {}", s.name, s.level, s.school.name),
                    detail,
                )
            })
            .collect(),
        PickKind::Item => corpus
            .items
            .iter()
            .map(|e| {
                let mut bits = vec![e.equipment_category.name.clone()];
                if let Some(wc) = &e.weapon_category {
                    bits.push(wc.clone());
                }
                if let Some(ac) = &e.armor_class {
                    bits.push(format!("AC {}", ac.base));
                }
                if let Some(c) = &e.cost {
                    bits.push(format!("{} {}", c.quantity, c.unit));
                }
                let detail = join(&e.desc);
                row(
                    e.index.clone(),
                    format!("{} — {}", e.name, bits.join(" · ")),
                    detail,
                )
            })
            .collect(),
        PickKind::Feat => corpus
            .feats
            .iter()
            .map(|f| {
                let mut detail = join(&f.desc);
                let pre: Vec<String> = f
                    .prerequisites
                    .iter()
                    .map(|p| format!("{} {}+", p.ability_score.name, p.minimum_score))
                    .collect();
                if !pre.is_empty() {
                    if !detail.is_empty() {
                        detail.push_str("\n\n");
                    }
                    detail.push_str(&format!("Prerequisite: {}.", pre.join(", ")));
                }
                row(f.index.clone(), f.name.clone(), detail)
            })
            .collect(),
        PickKind::Race => corpus
            .races
            .iter()
            .map(|r| {
                let ab: Vec<String> = r
                    .ability_bonuses
                    .iter()
                    .map(|b| format!("+{} {}", b.bonus, b.ability_score.name))
                    .collect();
                let traits: Vec<&str> = r.traits.iter().map(|t| t.name.as_str()).collect();
                let detail = format!(
                    "Speed {} ft. {}\n\nTraits: {}.\n\n{}",
                    r.speed,
                    if ab.is_empty() {
                        String::new()
                    } else {
                        format!("Ability bonuses: {}.", ab.join(", "))
                    },
                    traits.join("; "),
                    r.language_desc
                );
                row(r.index.clone(), r.name.clone(), detail)
            })
            .collect(),
        PickKind::Trait => corpus
            .traits
            .iter()
            .map(|t| row(t.index.clone(), t.name.clone(), join(&t.desc)))
            .collect(),
        PickKind::Class => corpus
            .classes
            .iter()
            .map(|c| {
                let saves: Vec<&str> = c.saving_throws.iter().map(|s| s.name.as_str()).collect();
                let detail = format!(
                    "d{} hit die · saves: {} · proficiencies: {}.",
                    c.hit_die,
                    if saves.is_empty() {
                        "none".into()
                    } else {
                        saves.join(", ")
                    },
                    c.proficiencies
                        .iter()
                        .map(|p| p.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                row(
                    c.index.clone(),
                    format!("{} — d{}", c.name, c.hit_die),
                    detail,
                )
            })
            .collect(),
        PickKind::Background => corpus
            .backgrounds
            .iter()
            .map(|b| {
                let skills: Vec<&str> = b
                    .starting_proficiencies
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect();
                let feat = b
                    .feature
                    .as_ref()
                    .map(|f| f.name.clone())
                    .unwrap_or_default();
                let detail = format!(
                    "Proficiencies: {}. {}{}",
                    skills.join(", "),
                    feat,
                    b.starting_gold
                        .as_ref()
                        .map(|g| format!(" · starting gold {} gp", g.quantity))
                        .unwrap_or_default()
                );
                row(b.index.clone(), b.name.clone(), detail)
            })
            .collect(),
        PickKind::Subrace => corpus
            .subraces
            .iter()
            .map(|sr| {
                let ab: Vec<String> = sr
                    .ability_bonuses
                    .iter()
                    .map(|b| format!("+{} {}", b.bonus, b.ability_score.name))
                    .collect();
                let traits: Vec<&str> = sr.racial_traits.iter().map(|t| t.name.as_str()).collect();
                let detail = format!(
                    "{} ({}) · {}. Traits: {}.",
                    sr.name,
                    sr.race.name,
                    ab.join(", "),
                    traits.join("; ")
                );
                row(
                    sr.index.clone(),
                    format!("{} ({})", sr.name, sr.race.name),
                    detail,
                )
            })
            .collect(),
        PickKind::Feature => corpus
            .features
            .iter()
            .map(|f| {
                let owner = f
                    .class
                    .as_ref()
                    .map(|c| c.name.clone())
                    .or_else(|| f.subclass.as_ref().map(|c| c.name.clone()))
                    .unwrap_or_else(|| "general".into());
                row(
                    f.index.clone(),
                    format!("{} — {} L{}", f.name, owner, f.level),
                    join(&f.desc),
                )
            })
            .collect(),
    }
}

/// Library rows for a kind, indexed `custom:<name>` and tagged `[custom]`.
fn custom_rows(lib: &Library, kind: PickKind) -> Vec<PickRow> {
    let row = |name: &str, label: String, detail: String| PickRow {
        hay: format!(
            "{} {} custom homebrew",
            label.to_lowercase(),
            name.to_lowercase()
        ),
        index: Library::custom_index(name),
        label,
        detail,
    };
    match kind {
        PickKind::Spell => lib
            .spells
            .values()
            .map(|sp| {
                let meta = format!(
                    "Range: {} · Casting: {} · Duration: {}{}{}",
                    sp.range,
                    sp.cast_time,
                    sp.duration,
                    if sp.concentration {
                        " · concentration"
                    } else {
                        ""
                    },
                    if sp.ritual { " · ritual" } else { "" },
                );
                let detail = if sp.desc.is_empty() {
                    meta
                } else {
                    format!("{}\n\n{}", sp.desc, meta)
                };
                row(
                    &sp.name,
                    format!("{} — L{} {} [custom]", sp.name, sp.level, sp.school),
                    detail,
                )
            })
            .collect(),
        PickKind::Item => lib
            .items
            .values()
            .map(|it| {
                row(
                    &it.name,
                    format!("{} [custom]", it.name),
                    it.description.clone(),
                )
            })
            .collect(),
        PickKind::Feat => lib
            .feats
            .values()
            .map(|f| {
                let detail = f.feature.as_ref().map_or_else(String::new, |fe| {
                    fe.effects
                        .iter()
                        .map(|e| format!("{e}"))
                        .collect::<Vec<_>>()
                        .join("; ")
                });
                row(&f.name, format!("{} [custom]", f.name), detail)
            })
            .collect(),
        PickKind::Race => lib
            .races
            .values()
            .map(|r| {
                let ab: Vec<String> = r
                    .ability_bonuses
                    .iter()
                    .map(|b| format!("+{} {}", b.bonus, b.score))
                    .collect();
                let detail = format!(
                    "Speed {}. {}",
                    r.speed.map_or("—".into(), |s| format!("{s} ft")),
                    ab.join(", ")
                );
                row(&r.name, format!("{} [custom]", r.name), detail)
            })
            .collect(),
        PickKind::Trait => lib
            .traits
            .values()
            .map(|t| row(&t.name, format!("{} [custom]", t.name), t.desc.clone()))
            .collect(),
        PickKind::Feature => lib
            .features
            .values()
            .map(|f| {
                let detail = f
                    .effects
                    .iter()
                    .map(|e| format!("{e}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                row(&f.name, format!("{} [custom]", f.name), detail)
            })
            .collect(),
        // The library holds no classes/backgrounds/subraces (yet): the
        // wizard drives those from the corpus alone.
        PickKind::Class | PickKind::Background | PickKind::Subrace => Vec::new(),
    }
}

/// `$after` hook for `field_key_actions!`: every text change re-filters.
pub fn after_picker_change(p: &mut Picker, _cx: &mut Context<Picker>) {
    p.refilter();
}

impl FieldOwner for Picker {
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

impl gpui::EntityInputHandler for Picker {
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
        self.refilter();
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

impl Focusable for Picker {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Picker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(VISIBLE)
            .map(|(i, r)| {
                let selected = i == self.sel;
                div()
                    .id(("pick", i))
                    .px_2()
                    .py_0()
                    .rounded_sm()
                    .text_ellipsis()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(selected, |d| d.bg(gpui::rgb(0x35_35_48)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.sel = i;
                        this.confirm(window, cx);
                    }))
                    .child(r.label.clone())
            })
            .collect();
        let detail = self
            .rows
            .get(self.sel)
            .map(|r| {
                let d = &r.detail;
                let clipped: &str = if d.len() > 1800 {
                    let mut end = 1800;
                    while end > 0 && !d.is_char_boundary(end) {
                        end -= 1;
                    }
                    &d[..end]
                } else {
                    d
                };
                div()
                    .max_h(gpui::px(200.))
                    .overflow_hidden()
                    .text_xs()
                    .text_color(gpui::rgb(0x9c_af_c9))
                    .child(clipped.to_string())
            })
            .unwrap_or_else(|| div().text_xs().child("no matches"));
        let base = div()
            .track_focus(&self.focus_handle)
            .key_context(PICKER_CTX)
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
                        "SRD {} ({}) — {} matches · type to filter · ↑/↓ move · enter picks · esc closes",
                        self.kind.label(),
                        self.corpus.edition,
                        self.rows.len()
                    )),
            )
            .child(div().w(gpui::px(360.)).child(FieldElement::new(cx.entity())))
            .children(visible)
            .child(detail)
            .on_action(cx.listener(|this, _: &PickerUp, _, cx| {
                this.move_sel(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PickerDown, _, cx| {
                this.move_sel(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldSubmit, window, cx| {
                this.confirm(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldCancel, window, cx| {
                this.cancel(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldBackspace, _, cx| {
                this.field_mut().backspace();
                this.refilter();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldDelete, _, cx| {
                this.field_mut().delete_forward();
                this.refilter();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldDeleteWordBack, _, cx| {
                this.field_mut().delete_word_back();
                this.refilter();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldLeft, _, cx| {
                this.field_mut().step_caret(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::field::FieldRight, _, cx| {
                this.field_mut().step_caret(1);
                cx.notify();
            }));
        crate::field_key_actions!(base, cx, after_picker_change)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackletter_types::database::reference::Edition;
    use gpui::{Entity, TestAppContext};
    use std::path::Path;

    fn corpus() -> Arc<Corpus> {
        // The corpus is vendored in-repo under `srd/` — tests never reach
        // outside the project.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../srd/2014/en");
        Arc::new(Corpus::load(&dir, Edition::Y2014).expect("vendored corpus loads"))
    }

    fn picker(cx: &mut TestAppContext, kind: PickKind) -> Entity<Picker> {
        cx.new(|cx| Picker::new(kind, corpus(), None, |_, _, _| {}, cx))
    }

    #[gpui::test]
    fn custom_entries_merge_into_the_rows(cx: &mut TestAppContext) {
        let mut lib = Library::default();
        lib.spells.insert(
            "Moonfire Bolt".into(),
            blackletter_types::sheet::spells::Spell {
                name: "Moonfire Bolt".into(),
                level: 4,
                ..Default::default()
            },
        );
        let p = cx.new(|cx| Picker::new(PickKind::Spell, corpus(), Some(&lib), |_, _, _| {}, cx));
        p.update(cx, |p, _| {
            p.field.insert("moonfire");
            p.refilter();
            assert_eq!(p.rows.len(), 1);
            assert_eq!(p.rows[0].index, "custom:Moonfire Bolt");
            assert!(p.rows[0].label.contains("[custom]"));
        });
    }

    #[gpui::test]
    fn filter_finds_water_walk(cx: &mut TestAppContext) {
        let p = picker(cx, PickKind::Spell);
        p.update(cx, |p, _| {
            p.field.insert("water walk");
            p.refilter();
            assert!(!p.rows.is_empty(), "'water walk' must match");
            assert_eq!(p.rows[0].index, "water-walk");
        });
    }

    #[gpui::test]
    fn default_corpus_is_2014_only(cx: &mut TestAppContext) {
        // Edition rule: 2024 rows never leak into the default corpus.
        let c = corpus();
        assert_eq!(c.edition, Edition::Y2014);
        let p = picker(cx, PickKind::Spell);
        p.update(cx, |p, _| {
            p.field.insert("circle of power");
            p.refilter();
            assert!(
                p.rows.iter().all(|r| r.index != "circle-of-power"),
                "2024-only spell must not appear"
            );
        });
    }

    #[gpui::test]
    fn selection_wraps_and_filter_ranks_prefix_first(cx: &mut TestAppContext) {
        let p = picker(cx, PickKind::Race);
        p.update(cx, |p, _| {
            p.field.insert("elf");
            p.refilter();
            // Prefix matches rank above contains-matches.
            assert!(p.rows[0].label.to_lowercase().starts_with("elf"));
            let n = p.rows.len();
            assert!(n >= 2, "elf should match Elf and Half-Elf");
            // Selection wraps in both directions.
            p.move_sel(-1);
            assert_eq!(p.sel, n - 1);
            p.move_sel(1);
            assert_eq!(p.sel, 0);
        });
    }
}
