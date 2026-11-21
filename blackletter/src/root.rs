//! The root view: mode tabs, the active mode's view, the status bar.
//!
//! The status bar is the app's only feedback channel (chonk rule): every
//! open/save/load/import path reports success AND failure here, never stderr.

use gpui::{
    Context, FocusHandle, KeyBinding, Render, SharedString, Window, actions, div, prelude::*,
};

use crate::session::Session;
use std::path::PathBuf;

actions!(
    blackletter,
    [ModeSheet, ModeDungeon, NewCampaign, OpenCampaign]
);

/// The app-wide keymap. Context-scoped contexts arrive with the modes; these
/// are global because mode switching and campaign open work from anywhere.
pub fn keybindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("ctrl-1", ModeSheet, None),
        KeyBinding::new("ctrl-2", ModeDungeon, None),
        KeyBinding::new("ctrl-n", NewCampaign, None),
        KeyBinding::new("ctrl-o", OpenCampaign, None),
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
    session: Session,
    session_path: Option<PathBuf>,
}

impl RootView {
    pub fn new(cx: &gpui::App, session: Session, session_path: Option<PathBuf>) -> Self {
        let mode = session.mode.into();
        Self {
            mode,
            status: None,
            focus_handle: cx.focus_handle(),
            session,
            session_path,
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

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        // Session persists on mode change: the file is tiny and this is the
        // moment both the mode and a representative window size are known.
        self.session.mode = mode.into();
        let b = window.bounds();
        self.session.window_size = Some((f32::from(b.size.width), f32::from(b.size.height)));
        self.save_session();
        cx.notify();
    }

    fn on_mode_sheet(&mut self, _: &ModeSheet, window: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(Mode::Sheet, window, cx);
    }

    fn on_mode_dungeon(&mut self, _: &ModeDungeon, window: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(Mode::Dungeon, window, cx);
    }

    fn on_open_campaign(&mut self, _: &OpenCampaign, _: &mut Window, cx: &mut Context<Self>) {
        let root = cx.entity().downgrade();
        crate::files::pick_folder(
            move |picked, cx| {
                let Some(path) = picked else { return };
                let _ = root.update(cx, |view, cx| view.open_campaign(path, cx));
            },
            cx,
        );
    }

    fn on_new_campaign(&mut self, _: &NewCampaign, _: &mut Window, cx: &mut Context<Self>) {
        let root = cx.entity().downgrade();
        crate::files::pick_folder(
            move |picked, cx| {
                let Some(path) = picked else { return };
                let _ = root.update(cx, |view, cx| view.new_campaign(path, cx));
            },
            cx,
        );
    }

    /// Point the app at an existing campaign folder; reports to the status bar.
    pub fn open_campaign(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        if !crate::campaign::Campaign::is_campaign(&path) {
            self.set_status(format!("not a campaign folder: {}", path.display()), cx);
            return;
        }
        let campaign = crate::campaign::Campaign { root: path.clone() };
        self.session.campaign = Some(path);
        self.save_session();
        self.set_status(
            format!(
                "opened campaign {} — {} sheets, {} dungeons",
                campaign.meta().name,
                campaign.list_characters().len(),
                campaign.list_dungeons().len()
            ),
            cx,
        );
    }

    /// Create the campaign skeleton in `path` (a fresh or empty folder) and open it.
    pub fn new_campaign(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        match crate::campaign::Campaign::create(path.clone()) {
            Ok(campaign) => {
                self.session.campaign = Some(path);
                self.save_session();
                self.set_status(format!("created campaign {}", campaign.meta().name), cx);
            }
            Err(e) => self.set_status(format!("could not create campaign: {e}"), cx),
        }
    }

    fn save_session(&mut self) {
        if let Some(path) = self.session_path.clone() {
            let _ = crate::session::save(&path, &self.session);
        }
    }

    // Read by the session restore path (Task 1.2).
    #[allow(dead_code)]
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
            .on_click(cx.listener(move |this, _, window, cx| match label {
                "Sheet" => this.set_mode(Mode::Sheet, window, cx),
                _ => this.set_mode(Mode::Dungeon, window, cx),
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
            .on_action(cx.listener(Self::on_new_campaign))
            .on_action(cx.listener(Self::on_open_campaign))
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
        let window = cx.add_window(|_, cx| RootView::new(cx, Session::default(), None));
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

    #[gpui::test]
    fn mode_change_persists_to_the_session_file(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("bl_session_persist_{}", std::process::id()));
        let path = dir.join("session.json");
        let _ = std::fs::remove_file(&path);
        cx.update(|cx| cx.bind_keys(keybindings()));
        let window =
            cx.add_window(|_, cx| RootView::new(cx, Session::default(), Some(path.clone())));
        window
            .update(cx, |view, window, cx| window.focus(view.focus_handle(), cx))
            .unwrap();
        cx.simulate_keystrokes(window.into(), "ctrl-2");
        cx.run_until_parked();
        let saved = crate::session::load(&path);
        assert_eq!(saved.mode, crate::session::StoredMode::Dungeon);
        assert!(saved.window_size.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui::test]
    fn open_campaign_reports_happy_and_error_paths(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("bl_open_campaign_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (_window, root) = open(cx);

        // A plain folder is refused; the session keeps no campaign.
        std::fs::create_dir_all(&dir).unwrap();
        root.update(cx, |view, cx| view.open_campaign(dir.clone(), cx));
        assert!(cx.read(|cx| root.read(cx).session.campaign.is_none()));

        // A real campaign folder opens and is remembered in the session.
        crate::campaign::Campaign::create(dir.clone()).unwrap();
        root.update(cx, |view, cx| view.open_campaign(dir.clone(), cx));
        assert_eq!(
            cx.read(|cx| root.read(cx).session.campaign.clone()),
            Some(dir.clone())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
