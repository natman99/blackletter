//! The one-line text field, ported near-verbatim from shale
//! (`shale/crates/app/src/field.rs` + `workspace/keys.rs::field_keys!`).
//!
//! A line of text with a caret that edits by character rather than byte, the
//! utf-16 translation gpui's input handler speaks, and a painted form with a
//! caret in it. The entity that owns a [`Field`] implements
//! [`EntityInputHandler`] on top of the helpers here, and draws itself with
//! [`FieldElement`] — a separate element because gpui only routes typed
//! characters to a handler registered during the current frame's paint, which
//! is the one moment the field's bounds are known.
//!
//! Key rules that survived the port (shale lessons, binding here): selection
//! is one `anchor` + caret and **every edit takes the selection first**; gpui
//! passes `range: None` for printable keys so the field's own selection is the
//! fallback; one key table (`field_keys`), never per-surface `on_key`;
//! clipboard verbs are actions so ctrl-c works while a picker holds the
//! keyboard.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, ShapedLine, SharedString, Style as ElementStyle, UTF16Selection, Window, actions, fill,
    font, point, px, relative,
};

use crate::dungeon_view::Palette;

/// Padding inside the field, in pixels. An `f32` because most of what the
/// field does with it is hit-testing arithmetic against a character width.
pub const PAD: f32 = 4.;

actions!(
    field,
    [
        FieldSubmit,
        FieldCancel,
        FieldBackspace,
        FieldDelete,
        FieldDeleteWordBack,
        FieldLeft,
        FieldRight,
        FieldSelectLeft,
        FieldSelectRight,
        FieldWordLeft,
        FieldWordRight,
        FieldSelectWordLeft,
        FieldSelectWordRight,
        FieldSelectToStart,
        FieldSelectToEnd,
        FieldSelectAll,
        FieldHome,
        FieldEnd,
        FieldCopy,
        FieldCut,
        FieldPaste,
    ]
);

/// The keys a one-line field answers, in whichever context holds one. One
/// table, because it is one widget — shale learned three hand-copied tables
/// drift apart. Printable characters never reach a keybinding; they arrive
/// through the input handler.
pub fn field_keys(ctx: &'static str) -> Vec<gpui::KeyBinding> {
    let ctx = Some(ctx);
    let mut kb: Vec<gpui::KeyBinding> = Vec::new();
    macro_rules! bind {
        ($($key:literal => $action:ident),* $(,)?) => {
            $( kb.push(gpui::KeyBinding::new($key, $action, ctx)); )*
        };
    }
    bind!(
        "enter" => FieldSubmit,
        "escape" => FieldCancel,
        "backspace" => FieldBackspace,
        "delete" => FieldDelete,
        "ctrl-backspace" => FieldDeleteWordBack,
        "left" => FieldLeft,
        "right" => FieldRight,
        // Shift extends the caret. A field is one line, so `shift-up` and
        // `shift-down` are the ends of it, as in a shell's line editor.
        "shift-left" => FieldSelectLeft,
        "shift-right" => FieldSelectRight,
        "shift-up" => FieldSelectToStart,
        "shift-down" => FieldSelectToEnd,
        "home" => FieldHome,
        "end" => FieldEnd,
        "shift-home" => FieldSelectToStart,
        "shift-end" => FieldSelectToEnd,
        "ctrl-left" => FieldWordLeft,
        "ctrl-right" => FieldWordRight,
        "ctrl-shift-left" => FieldSelectWordLeft,
        "ctrl-shift-right" => FieldSelectWordRight,
        // `ctrl-a` is select-all here, not the shell's start-of-line.
        "ctrl-a" => FieldSelectAll,
        "ctrl-c" => FieldCopy,
        "ctrl-x" => FieldCut,
        "ctrl-v" => FieldPaste,
    );
    kb
}

/// The selection and clipboard keys every field owner answers, wired onto the
/// element that carries its key context. `$after` is the owner's chance to
/// react to the text changing.
#[macro_export]
macro_rules! field_key_actions {
    ($el:expr, $cx:expr, $after:path) => {
        $el.on_action(
            $cx.listener(|this, _: &$crate::field::FieldSelectLeft, _window, cx| {
                this.field_mut().select_left();
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldSelectRight, _window, cx| {
                this.field_mut().select_right();
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldWordLeft, _window, cx| {
                this.field_mut().caret_word_left();
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldWordRight, _window, cx| {
                this.field_mut().caret_word_right();
                cx.notify();
            }),
        )
        .on_action($cx.listener(
            |this, _: &$crate::field::FieldSelectWordLeft, _window, cx| {
                this.field_mut().select_word_left();
                cx.notify();
            },
        ))
        .on_action($cx.listener(
            |this, _: &$crate::field::FieldSelectWordRight, _window, cx| {
                this.field_mut().select_word_right();
                cx.notify();
            },
        ))
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldSelectToStart, _window, cx| {
                this.field_mut().select_to_start();
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldSelectToEnd, _window, cx| {
                this.field_mut().select_to_end();
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldHome, _window, cx| {
                this.field_mut().caret_to_start();
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldEnd, _window, cx| {
                this.field_mut().caret_to_end();
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldSelectAll, _window, cx| {
                this.field_mut().select_all();
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldCopy, _window, cx| {
                if !$crate::field::copy(this, cx) {
                    return;
                }
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldCut, _window, cx| {
                if !$crate::field::cut(this, cx) {
                    return;
                }
                $after(this, cx);
                cx.notify();
            }),
        )
        .on_action(
            $cx.listener(|this, _: &$crate::field::FieldPaste, _window, cx| {
                if !$crate::field::paste(this, cx) {
                    return;
                }
                $after(this, cx);
                cx.notify();
            }),
        )
    };
}

/// A single line of text with a caret and, optionally, a selection.
///
/// The selection is one number — `anchor`, the end that stopped moving — with
/// the caret as the other end. No edit has to ask which end is which: the
/// range is normalised at the one place that reads it.
pub struct Field {
    /// What the field holds so far.
    text: SharedString,
    /// Byte offset in `text` where the caret sits. gpui's input handler speaks
    /// utf-16, so every conversion goes through the two offset helpers.
    caret: usize,
    /// The far end of a selection, if there is one.
    anchor: Option<usize>,
}

impl Field {
    pub fn new(initial: &str) -> Self {
        let text = SharedString::from(initial.to_string());
        Self {
            caret: text.len(),
            text,
            anchor: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    // ----- selection -----

    /// The selected range, low offset first, or `None` when nothing is
    /// selected. An anchor equal to the caret is not a selection.
    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        if anchor == self.caret {
            return None;
        }
        Some(anchor.min(self.caret)..anchor.max(self.caret))
    }

    /// The selected range, and the anchor cleared: what every edit calls
    /// before it changes the text, so "type over a selection" is the default.
    fn take_selection(&mut self) -> Option<Range<usize>> {
        let range = self.selection();
        self.anchor = None;
        range
    }

    pub fn selected_text(&self) -> Option<String> {
        let range = self.selection()?;
        Some(self.text[range].to_string())
    }

    pub fn select_left(&mut self) {
        self.anchor = Some(self.anchor.unwrap_or(self.caret));
        self.step_caret_unselected(-1);
    }

    pub fn select_right(&mut self) {
        self.anchor = Some(self.anchor.unwrap_or(self.caret));
        self.step_caret_unselected(1);
    }

    pub fn caret_to_start(&mut self) {
        self.anchor = None;
        self.caret = 0;
    }

    pub fn caret_to_end(&mut self) {
        self.anchor = None;
        self.caret = self.text.len();
    }

    pub fn select_to_start(&mut self) {
        self.anchor = Some(self.anchor.unwrap_or(self.caret));
        self.caret = 0;
    }

    pub fn select_to_end(&mut self) {
        self.anchor = Some(self.anchor.unwrap_or(self.caret));
        self.caret = self.text.len();
    }

    /// `ctrl`+`left`/`right`: the word boundary the caret lands on.
    pub fn caret_word_left(&mut self) {
        self.anchor = None;
        self.caret = word_back(&self.text, self.caret);
    }

    pub fn caret_word_right(&mut self) {
        self.anchor = None;
        self.caret = word_forward(&self.text, self.caret);
    }

    /// `ctrl`+`shift`+`left`/right`: the selection grows by a word. Only the
    /// caret end moves — the anchor stays where the key was first pressed.
    pub fn select_word_left(&mut self) {
        self.anchor = Some(self.anchor.unwrap_or(self.caret));
        self.caret = word_back(&self.text, self.caret);
    }

    pub fn select_word_right(&mut self) {
        self.anchor = Some(self.anchor.unwrap_or(self.caret));
        self.caret = word_forward(&self.text, self.caret);
    }

    /// The whole line, which is what `ctrl-a` means here.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    /// Delete what is selected, if anything.
    pub fn delete_selection(&mut self) {
        let Some(range) = self.take_selection() else {
            return;
        };
        let mut new = self.text.to_string();
        new.replace_range(range.clone(), "");
        self.caret = range.start;
        self.text = SharedString::from(new);
    }

    /// The selection as character indices, which is what the painted band wants.
    pub fn selection_char_range(&self) -> Option<(usize, usize)> {
        let range = self.selection()?;
        Some((
            self.text[..range.start].chars().count(),
            self.text[..range.end].chars().count(),
        ))
    }

    /// Type at the caret. A selection is what gets typed over.
    pub fn insert(&mut self, text: &str) {
        let mut new = self.text.to_string();
        if let Some(range) = self.take_selection() {
            new.replace_range(range.clone(), "");
            self.caret = range.start;
        }
        new.replace_range(self.caret..self.caret, text);
        self.caret += text.len();
        self.text = SharedString::from(new);
    }

    pub fn backspace(&mut self) {
        if let Some(range) = self.take_selection() {
            let mut new = self.text.to_string();
            new.replace_range(range.clone(), "");
            self.caret = range.start;
            self.text = SharedString::from(new);
            return;
        }
        if self.caret == 0 {
            return;
        }
        let mut new = self.text.to_string();
        // One *character*, not one byte: a name can hold non-ASCII text.
        let previous = new[..self.caret]
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        let start = self.caret - previous;
        new.replace_range(start..self.caret, "");
        self.caret = start;
        self.text = SharedString::from(new);
    }

    pub fn delete_forward(&mut self) {
        if let Some(range) = self.take_selection() {
            let mut new = self.text.to_string();
            new.replace_range(range.clone(), "");
            self.caret = range.start;
            self.text = SharedString::from(new);
            return;
        }
        if self.caret >= self.text.len() {
            return;
        }
        let mut new = self.text.to_string();
        let width = new[self.caret..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        new.replace_range(self.caret..self.caret + width, "");
        self.text = SharedString::from(new);
    }

    /// Delete the word before the caret — `ctrl-backspace`.
    pub fn delete_word_back(&mut self) {
        if let Some(range) = self.take_selection() {
            let mut new = self.text.to_string();
            new.replace_range(range.clone(), "");
            self.caret = range.start;
            self.text = SharedString::from(new);
            return;
        }
        let start = word_back(&self.text, self.caret);
        if start >= self.caret {
            return;
        }
        let mut new = self.text.to_string();
        new.replace_range(start..self.caret, "");
        self.caret = start;
        self.text = SharedString::from(new);
    }

    /// Move the caret by `delta` characters, clamped to the line. With a
    /// selection out, a plain motion collapses it instead of stepping.
    pub fn step_caret(&mut self, delta: isize) {
        if let Some(range) = self.selection() {
            self.caret = if delta < 0 { range.start } else { range.end };
            self.anchor = None;
            return;
        }
        self.step_caret_unselected(delta);
    }

    fn step_caret_unselected(&mut self, delta: isize) {
        let mut new = self.caret as isize;
        if delta < 0 {
            new -= self.text[..self.caret]
                .chars()
                .next_back()
                .map_or(0, |c| c.len_utf8()) as isize;
        } else if delta > 0 {
            new += self.text[self.caret..]
                .chars()
                .next()
                .map_or(0, |c| c.len_utf8()) as isize;
        }
        self.caret = new.clamp(0, self.text.len() as isize) as usize;
    }

    /// The caret as a character index, which is what a shaped line wants.
    pub fn caret_char_index(&self) -> usize {
        self.text[..self.caret].chars().count()
    }

    /// The text the field paints: the caret always sits after a character, so
    /// an empty field still needs something to shape.
    pub fn painted(&self) -> SharedString {
        if self.text.is_empty() {
            SharedString::from(" ")
        } else {
            self.text.clone()
        }
    }

    // ----- utf16 <-> utf8 helpers (gpui's input handler speaks utf-16) -----

    pub fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.text.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.text.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    /// The selection gpui should see — the real one, not the caret. An IME
    /// confirm or a platform paste replaces what this reports.
    pub fn caret_selection(&self) -> UTF16Selection {
        let range = self.selection();
        UTF16Selection {
            range: self.offset_to_utf16(range.as_ref().map_or(self.caret, |r| r.start))
                ..self.offset_to_utf16(range.as_ref().map_or(self.caret, |r| r.end)),
            reversed: self.anchor.is_some_and(|anchor| anchor > self.caret),
        }
    }

    /// The text inside a utf-16 range, and the same range back.
    pub fn text_in_utf16(&self, range: Range<usize>) -> (String, Range<usize>) {
        let start = self.offset_from_utf16(range.start);
        let end = self.offset_from_utf16(range.end);
        (self.text[start..end].to_string(), range)
    }

    /// Apply what the platform handed us: a typed character, a paste, or an
    /// IME confirmation. A field is one line, so anything carrying a newline
    /// is flattened to its first line.
    pub fn replace_utf16(&mut self, range: Option<Range<usize>>, new_text: &str) {
        let first_line = new_text.lines().next().unwrap_or("");
        // A typed character reaches here with no range at all, so the field's
        // own selection is the only thing that can say what it should land on.
        let range = range.map(|r| self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end));
        let range = range.or_else(|| self.selection());
        self.anchor = None;
        if let Some(range) = range {
            let start = range.start;
            let end = range.end;
            let mut new = self.text.to_string();
            new.replace_range(start..end, first_line);
            self.caret = start + first_line.len();
            self.text = SharedString::from(new);
        } else {
            self.insert(first_line);
        }
    }

    /// Which character a click at `x` (relative to the field's left edge)
    /// landed on, as a utf-16 offset. Click-to-caret lands with the sheet
    /// fields' mouse wiring (Phase 3); keyboard-first until then.
    #[allow(dead_code)]
    pub fn utf16_index_for_x(&self, x: f32, char_width: f32) -> usize {
        let column = ((x - PAD) / char_width).max(0.).round() as usize;
        let offset = self
            .text
            .char_indices()
            .map(|(i, _)| i)
            .nth(column)
            .unwrap_or(self.text.len());
        self.offset_to_utf16(offset)
    }
}

/// What [`FieldElement`] needs from the entity that owns a [`Field`], so the
/// field can be painted and wired to the input handler without knowing who the
/// owner is.
pub trait FieldOwner {
    fn field(&self) -> &Field;
    fn field_mut(&mut self) -> &mut Field;
    /// The resolved font family, so the field matches the surrounding text.
    fn family(&self) -> SharedString;
    /// Text size in points.
    fn size(&self) -> f32;
    /// The focus the input handler is registered for.
    fn focus(&self) -> FocusHandle;
}

// ----- the clipboard, shared by every field -----

/// `false` is the answer when there was nothing to do, which is the caller's
/// cue to leave the field alone rather than repaint it.
pub fn copy(owner: &impl FieldOwner, cx: &mut App) -> bool {
    let Some(text) = owner.field().selected_text() else {
        return false;
    };
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    true
}

pub fn cut(owner: &mut impl FieldOwner, cx: &mut App) -> bool {
    let Some(text) = owner.field().selected_text() else {
        return false;
    };
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    owner.field_mut().delete_selection();
    true
}

pub fn paste(owner: &mut impl FieldOwner, cx: &mut App) -> bool {
    let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
        return false;
    };
    owner.field_mut().insert(&text);
    true
}

/// Rough advance of one character, used only for the two geometry questions
/// the IME asks. The field's own painting uses the shaped line.
pub fn approx_char_width() -> f32 {
    f32::from(px(8.))
}

/// Line height for a field at `size` points.
pub fn line_height(size: f32) -> f32 {
    size * 1.4
}

/// The painted field: a shaded box, the text, the caret, and the
/// [`ElementInputHandler`] registration that makes printable characters reach
/// its owner.
pub struct FieldElement<E> {
    owner: Entity<E>,
}

impl<E: FieldOwner + EntityInputHandler + 'static> FieldElement<E> {
    pub fn new(owner: Entity<E>) -> Self {
        Self { owner }
    }
}

impl<E: FieldOwner + EntityInputHandler + 'static> IntoElement for FieldElement<E> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: FieldOwner + EntityInputHandler + 'static> Element for FieldElement<E> {
    type RequestLayoutState = ();
    type PrepaintState = ShapedLine;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let size = self.owner.read(cx).size();
        let mut style = ElementStyle::default();
        style.size.width = relative(1.).into();
        style.size.height = px(line_height(size) + PAD * 2.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let owner = self.owner.read(cx);
        let text = owner.field().painted();
        let family = owner.family();
        let size = owner.size();
        let run = gpui::TextRun {
            len: text.len(),
            font: font(family),
            color: Palette::text(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_line(text, px(size), &[run], None)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // A shaded box with an underline as its edge: accent while it holds the
        // keyboard, muted otherwise.
        window.paint_quad(fill(bounds, Palette::room_fill()));
        let focused = self.owner.read(cx).focus().is_focused(window);
        window.paint_quad(fill(
            Bounds::from_corners(
                point(bounds.left(), bounds.bottom() - px(1.)),
                point(bounds.right(), bounds.bottom()),
            ),
            if focused {
                Palette::accent()
            } else {
                Palette::room_border()
            },
        ));

        let origin = point(bounds.left() + px(PAD), bounds.top() + px(PAD));
        // The selection band goes *under* the text.
        if let Some((start, end)) = self.owner.read(cx).field().selection_char_range() {
            window.paint_quad(fill(
                Bounds::from_corners(
                    point(
                        origin.x + prepaint.x_for_index(start),
                        bounds.top() + px(PAD),
                    ),
                    point(
                        origin.x + prepaint.x_for_index(end),
                        bounds.bottom() - px(PAD),
                    ),
                ),
                Palette::select(),
            ));
        }
        let size = self.owner.read(cx).size();
        let _ = prepaint.paint(
            origin,
            px(line_height(size)),
            gpui::TextAlign::default(),
            None,
            window,
            cx,
        );

        let caret = self.owner.read(cx).field().caret_char_index();
        let x = origin.x + prepaint.x_for_index(caret);
        window.paint_quad(fill(
            Bounds::from_corners(
                point(x, bounds.top() + px(PAD)),
                point(x + px(1.5), bounds.bottom() - px(PAD)),
            ),
            Palette::text(),
        ));

        // Typed characters only reach a handler registered during this frame's
        // paint, with this frame's bounds.
        let focus = self.owner.read(cx).focus();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.owner.clone()),
            cx,
        );
    }
}

// ----- word boundaries (ported from shale_core::buffer::words) -----

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Newline,
    Whitespace,
    Word,
    Punct,
}

fn class_of(c: char) -> WordClass {
    if c == '\n' {
        WordClass::Newline
    } else if c.is_whitespace() {
        WordClass::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        WordClass::Word
    } else {
        WordClass::Punct
    }
}

/// The offset `ctrl`+`left` lands on: the start of the run before `at`, or —
/// sitting on a blank or at a run's start — the start of the previous run.
pub fn word_back(text: &str, at: usize) -> usize {
    if at == 0 {
        return 0;
    }
    let len = text.len();
    let at = at.min(len);
    let start_of_prev = |i: usize| {
        let mut s = i.saturating_sub(1);
        while s > 0 && !text.is_char_boundary(s) {
            s -= 1;
        }
        s
    };
    let prev_class = |i: usize| text[start_of_prev(i)..i].chars().next().map(class_of);

    let mut i = at;
    // In the middle of a run: back up to that run's start.
    if i < len
        && let Some(cls) = text[i..].chars().next().map(class_of)
        && cls != WordClass::Whitespace
        && cls != WordClass::Newline
    {
        while i > 0 && prev_class(i) == Some(cls) {
            i = start_of_prev(i);
        }
        if i < at {
            return i;
        }
    }
    // At a run start (or on a blank): cross blanks, then back over the previous run.
    while i > 0 {
        match prev_class(i) {
            Some(WordClass::Whitespace) | Some(WordClass::Newline) => i = start_of_prev(i),
            _ => break,
        }
    }
    if i == 0 {
        return 0;
    }
    if let Some(cls) = prev_class(i) {
        while i > 0 && prev_class(i) == Some(cls) {
            i = start_of_prev(i);
        }
    }
    i
}

/// The forward twin of [`word_back`], with the same classes and the same walk,
/// so `ctrl`+`left` then `ctrl`+`right` comes back to the boundary it left.
pub fn word_forward(text: &str, at: usize) -> usize {
    let len = text.len();
    let at = at.min(len);
    if at == len {
        return len;
    }
    let next_class = |i: usize| text[i..].chars().next().map(class_of);
    let past_next = |i: usize| i + text[i..].chars().next().map_or(0, |ch| ch.len_utf8());

    let mut i = at;
    // In the middle of a run: forward to that run's end.
    if let Some(cls) = next_class(i)
        && cls != WordClass::Whitespace
        && cls != WordClass::Newline
    {
        while i < len && next_class(i) == Some(cls) {
            i = past_next(i);
        }
        if i > at {
            return i;
        }
    }
    // On a blank (or at a run's end): cross blanks, then over the next run.
    while i < len {
        match next_class(i) {
            Some(WordClass::Whitespace) | Some(WordClass::Newline) => i = past_next(i),
            _ => break,
        }
    }
    if i < len
        && let Some(cls) = next_class(i)
    {
        while i < len && next_class(i) == Some(cls) {
            i = past_next(i);
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::Field;

    #[test]
    fn words_are_the_unit_ctrl_moves_and_selects() {
        let mut field = Field::new("tag:monster order:cr");
        field.select_word_left();
        assert_eq!(field.selected_text().as_deref(), Some("cr"));
        field.select_word_left();
        field.select_word_left();
        assert_eq!(field.selected_text().as_deref(), Some("order:cr"));

        field.caret_word_left();
        assert_eq!(field.selection(), None);
        assert_eq!(field.caret, 4, "the start of `monster`");
        field.caret_word_right();
        assert_eq!(field.caret, 11);
    }

    #[test]
    fn a_selection_reads_the_same_whichever_way_it_was_made() {
        let mut left = Field::new("goblin");
        left.select_left();
        left.select_left();
        assert_eq!(left.selection(), Some(4..6));
        assert_eq!(left.selected_text().as_deref(), Some("in"));

        let mut right = Field::new("goblin");
        right.caret_to_start();
        right.select_right();
        right.select_right();
        assert_eq!(right.selection(), Some(0..2));
        assert_eq!(right.selected_text().as_deref(), Some("go"));
    }

    #[test]
    fn the_selection_reported_to_gpui_is_the_real_one() {
        let mut field = Field::new("goblin");
        field.caret_to_start();
        field.select_right();
        let forward = field.caret_selection();
        assert_eq!(forward.range, 0..1);
        assert!(!forward.reversed);

        let mut field = Field::new("goblin");
        field.select_left();
        let back = field.caret_selection();
        assert_eq!(back.range, 5..6);
        assert!(back.reversed);
    }

    #[test]
    fn a_selection_over_a_wide_character_is_still_one_character() {
        let mut field = Field::new("gobelin");
        field.replace_utf16(Some(5..6), "é");
        assert_eq!(field.text(), "gobelén");
        field.select_left();
        assert_eq!(field.selected_text().as_deref(), Some("é"));
        assert_eq!(field.selection_char_range(), Some((5, 6)));
    }

    #[test]
    fn every_edit_types_over_the_selection() {
        type Edit = (&'static str, Box<dyn Fn(&mut Field)>);
        let cases: [Edit; 4] = [
            ("g", Box::new(|f| f.insert("g"))),
            ("", Box::new(|f| f.backspace())),
            ("", Box::new(|f| f.delete_forward())),
            ("", Box::new(|f| f.delete_word_back())),
        ];
        for (expected, edit) in cases {
            let mut field = Field::new("goblin");
            field.caret_to_start();
            field.select_to_end();
            edit(&mut field);
            assert_eq!(field.text(), expected);
            assert_eq!(field.selection(), None);
        }
    }
}
