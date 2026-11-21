//! The root view: mode tabs, the active mode's view, the status bar.
//!
//! The status bar is the app's only feedback channel (chonk rule): every
//! open/save/load/import path reports success AND failure here, never stderr.

use gpui::{
    Context, FocusHandle, KeyBinding, Render, SharedString, Window, actions, div, prelude::*,
};

actions!(blackletter, [ModeSheet, ModeDungeon]);

/// The app-wide keymap. Context-scoped contexts arrive with the modes; these
/// two are global because switching modes must work from anywhere.
pub fn keybindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("ctrl-1", ModeSheet, None),
        KeyBinding::new("ctrl-2", ModeDungeon, None),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Sheet,
    Dungeon,
}

pub struct RootView {
    mode: Mode,
    status: Option<SharedString>,
    focus_handle: FocusHandle,
}

impl RootView {
    pub fn new(cx: &gpui::App) -> Self {
        Self {
            mode: Mode::default(),
            status: None,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Report a line in the status bar (chonk rule: success lines too).
    pub fn set_status(&mut self, line: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = Some(line.into());
        cx.notify();
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            cx.notify();
        }
    }

    fn on_mode_sheet(&mut self, _: &ModeSheet, _: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(Mode::Sheet, cx);
    }

    fn on_mode_dungeon(&mut self, _: &ModeDungeon, _: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(Mode::Dungeon, cx);
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    fn tab(&self, label: &'static str, active: bool, cx: &Context<Self>) -> impl IntoElement {
        div()
            .id(label)
            .cursor_pointer()
            .px_3()
            .py_1()
            .rounded_md()
            .when(active, |d| d.bg(gpui::rgb(0x45_47_66)))
            .text_color(gpui::rgb(if active { 0xcd_d6_f4 } else { 0x6c_70_86 }))
            .on_click(cx.listener(move |this, _, _, cx| match label {
                "Sheet" => this.set_mode(Mode::Sheet, cx),
                _ => this.set_mode(Mode::Dungeon, cx),
            }))
            .child(label)
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .bg(gpui::rgb(0x1e_1e_2e))
            .text_color(gpui::rgb(0xcd_d6_f4))
            .on_action(cx.listener(Self::on_mode_sheet))
            .on_action(cx.listener(Self::on_mode_dungeon))
            // Top bar: mode tabs.
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(self.tab("Sheet", self.mode == Mode::Sheet, cx))
                    .child(self.tab("Dungeon", self.mode == Mode::Dungeon, cx)),
            )
            // Center: the active mode's view.
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::rgb(0x24_24_30))
                    .text_2xl()
                    .child(match self.mode {
                        Mode::Sheet => "Sheet mode — nothing open",
                        Mode::Dungeon => "Dungeon mode — nothing open",
                    }),
            )
            // Bottom: the status bar (chonk: 22px + gap).
            .child(
                div()
                    .h(gpui::px(22.))
                    .flex()
                    .items_center()
                    .px_2()
                    .text_sm()
                    .text_color(gpui::rgb(0x9c_af_c9))
                    .child(self.status.clone().unwrap_or_default()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn open(cx: &mut TestAppContext) -> (gpui::WindowHandle<RootView>, gpui::Entity<RootView>) {
        cx.update(|cx| cx.bind_keys(keybindings()));
        let window = cx.add_window(|_, cx| RootView::new(cx));
        window
            .update(cx, |view, window, cx| window.focus(view.focus_handle(), cx))
            .unwrap();
        let root = window.root(cx).unwrap();
        (window, root)
    }

    fn mode(cx: &TestAppContext, root: &gpui::Entity<RootView>) -> Mode {
        cx.read(|cx| root.read(cx).mode())
    }

    #[gpui::test]
    fn ctrl_chords_switch_modes(cx: &mut TestAppContext) {
        let (window, root) = open(cx);
        assert_eq!(mode(cx, &root), Mode::Sheet);
        cx.simulate_keystrokes(window.into(), "ctrl-2");
        cx.run_until_parked();
        assert_eq!(mode(cx, &root), Mode::Dungeon);
        cx.simulate_keystrokes(window.into(), "ctrl-1");
        cx.run_until_parked();
        assert_eq!(mode(cx, &root), Mode::Sheet);
    }
}
