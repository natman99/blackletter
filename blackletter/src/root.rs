//! The root view: mode tabs, the active mode's view, the status bar.
//!
//! The status bar is the app's only feedback channel (chonk rule): every
//! open/save/load/import path reports success AND failure here, never stderr.

use gpui::{
    Context, Entity, FocusHandle, Focusable, KeyBinding, Render, SharedString, Window, actions,
    div, prelude::*,
};

use crate::autosave::Autosave;
use crate::dungeon_view::{self, DungeonDoc, DungeonView};
use crate::session::Session;
use crate::sheet_view::{self, SheetDoc, SheetView};
use blackletter_types::{database::corpus::Corpus, library::Library};
use std::path::PathBuf;
use std::sync::Arc;

actions!(
    blackletter,
    [
        ModeSheet,
        ModeDungeon,
        NewCampaign,
        OpenCampaign,
        PrevDungeon,
        NextDungeon
    ]
);

/// The app-wide keymap. Context-scoped contexts arrive with the modes; these
/// are global because mode switching and campaign open work from anywhere.
pub fn keybindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("ctrl-1", ModeSheet, None),
        KeyBinding::new("ctrl-2", ModeDungeon, None),
        KeyBinding::new("ctrl-n", NewCampaign, None),
        KeyBinding::new("ctrl-o", OpenCampaign, None),
        KeyBinding::new("ctrl-comma", PrevDungeon, None),
        KeyBinding::new("ctrl-period", NextDungeon, None),
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
    /// The SRD corpus, loaded once at startup; `None` disables the pickers.
    corpus: Option<Arc<Corpus>>,
    /// The custom-content library (plan Task 4.3), loaded at startup and
    /// saved on every change; every sheet view shares this one entity.
    library: Entity<Library>,
    /// Where the library persists; `None` (tests) keeps it in memory only.
    library_path: Option<PathBuf>,
    dungeon: Option<Entity<DungeonView>>,
    sheet: Option<Entity<SheetView>>,
}

impl RootView {
    pub fn new(
        cx: &mut gpui::App,
        session: Session,
        session_path: Option<PathBuf>,
        corpus: Option<Arc<Corpus>>,
        library: Library,
        library_path: Option<PathBuf>,
    ) -> Self {
        let library = cx.new(|_| library);
        let mode = session.mode.into();
        let status = if corpus.is_some() {
            None
        } else {
            Some("SRD corpus not found — pickers disabled".into())
        };
        Self {
            mode,
            status,
            focus_handle: cx.focus_handle(),
            session,
            session_path,
            corpus,
            library,
            library_path,
            dungeon: None,
            sheet: None,
        }
    }

    /// The shared custom-content library entity (plan Task 4.3).
    pub fn library(&self) -> &Entity<Library> {
        &self.library
    }

    /// Cache the library to disk (Task 4.3: everything added shows up in
    /// future sessions). False when no path is configured or the write
    /// failed; the caller reports.
    pub fn save_library(&self, cx: &gpui::App) -> bool {
        let Some(path) = &self.library_path else {
            return false;
        };
        self.library.read(cx).save(path).is_ok()
    }

    #[cfg(test)]
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
        if mode == Mode::Dungeon {
            self.ensure_dungeon(window, cx);
        } else {
            self.ensure_sheet(window, cx);
        }
        cx.notify();
    }

    /// Build (if needed) and focus the dungeon view. Also called at startup
    /// when the session restores into dungeon mode.
    pub fn ensure_dungeon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dungeon.is_none() {
            let view = self.build_dungeon(cx);
            window.focus(&view.read(cx).focus_handle(cx), cx);
        }
    }

    /// Build (if needed) and focus the sheet view.
    pub fn ensure_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sheet.is_none() {
            let view = self.build_sheet(cx);
            window.focus(&view.read(cx).focus_handle(cx), cx);
        }
    }

    /// Build the sheet view (first character of the campaign, else a fresh
    /// doc) and register its autosave saver.
    fn build_sheet(&mut self, cx: &mut Context<Self>) -> Entity<SheetView> {
        let (doc, dir) = self.sheet_doc_or_new();
        let root = cx.entity().downgrade();
        let view = sheet_view::build(
            doc,
            dir.clone(),
            self.corpus.clone(),
            self.library.clone(),
            root,
            cx,
        );
        let weak = view.downgrade();
        let id = view.entity_id();
        cx.global_mut::<Autosave>().register(
            id,
            Box::new(move |cx| weak.update(cx, |v, _| v.save_if_dirty()).unwrap_or(false)),
        );
        self.sheet = Some(view.clone());
        view
    }

    fn sheet_doc_or_new(&self) -> (SheetDoc, Option<PathBuf>) {
        if let Some(root) = &self.session.campaign {
            let campaign = crate::campaign::Campaign { root: root.clone() };
            let dir = campaign.characters_dir();
            if let Some(first) = campaign.list_characters().into_iter().next()
                && let Ok(doc) = SheetDoc::load(&first)
            {
                return (doc, Some(dir));
            }
            return (SheetDoc::new(campaign.meta().name), Some(dir));
        }
        (SheetDoc::new("New Sheet"), None)
    }

    /// Cycle the open sheet within the campaign's `characters/` folder.
    pub fn switch_sheet(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.session.campaign.clone() else {
            self.set_status("no campaign open (ctrl-O)", cx);
            return;
        };
        let campaign = crate::campaign::Campaign { root };
        let list = campaign.list_characters();
        if list.is_empty() {
            self.set_status("no sheets in this campaign", cx);
            return;
        }
        let current = self
            .sheet
            .as_ref()
            .and_then(|v| v.read(cx).doc().file.clone());
        let idx = list
            .iter()
            .position(|p| Some(p) == current.as_ref())
            .unwrap_or(0);
        let next = (idx as i32 + delta).rem_euclid(list.len() as i32) as usize;
        let path = list[next].clone();
        if let Some(view) = self.sheet.take() {
            let id = view.entity_id();
            view.update(cx, |v, _| {
                v.save_if_dirty();
            });
            cx.global_mut::<Autosave>().unregister(id);
        }
        match SheetDoc::load(&path) {
            Ok(doc) => {
                let name = doc.character.name.clone();
                let view = self.build_sheet_with(doc, Some(campaign.characters_dir()), cx);
                window.focus(&view.read(cx).focus_handle(cx), cx);
                self.set_status(format!("opened sheet {name}"), cx);
            }
            Err(e) => self.set_status(format!("could not open {}: {e}", path.display()), cx),
        }
    }

    fn build_sheet_with(
        &mut self,
        doc: SheetDoc,
        dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Entity<SheetView> {
        let root = cx.entity().downgrade();
        let view = sheet_view::build(
            doc,
            dir,
            self.corpus.clone(),
            self.library.clone(),
            root,
            cx,
        );
        let weak = view.downgrade();
        let id = view.entity_id();
        cx.global_mut::<Autosave>().register(
            id,
            Box::new(move |cx| weak.update(cx, |v, _| v.save_if_dirty()).unwrap_or(false)),
        );
        self.sheet = Some(view.clone());
        view
    }

    /// Build the dungeon view (first dungeon of the campaign, else a fresh
    /// doc) and register its autosave saver.
    fn build_dungeon(&mut self, cx: &mut Context<Self>) -> Entity<DungeonView> {
        let (doc, dir) = self.dungeon_doc_or_new();
        self.build_dungeon_with(doc, dir, cx)
    }

    fn build_dungeon_with(
        &mut self,
        doc: DungeonDoc,
        dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Entity<DungeonView> {
        let root = cx.entity().downgrade();
        let view = dungeon_view::build(doc, dir, root, cx);
        let weak = view.downgrade();
        let id = view.entity_id();
        cx.global_mut::<Autosave>().register(
            id,
            Box::new(move |cx| weak.update(cx, |v, _| v.save_if_dirty()).unwrap_or(false)),
        );
        self.dungeon = Some(view.clone());
        view
    }

    fn dungeon_doc_or_new(&self) -> (DungeonDoc, Option<PathBuf>) {
        if let Some(root) = &self.session.campaign {
            let campaign = crate::campaign::Campaign { root: root.clone() };
            let dir = campaign.dungeons_dir();
            if let Some(first) = campaign.list_dungeons().into_iter().next()
                && let Ok(doc) = DungeonDoc::load(&first)
            {
                return (doc, Some(dir));
            }
            return (DungeonDoc::new(campaign.meta().name), Some(dir));
        }
        (DungeonDoc::new("New Dungeon"), None)
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

    fn on_prev_dungeon(&mut self, _: &PrevDungeon, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Sheet {
            self.switch_sheet(-1, window, cx);
        } else {
            self.switch_dungeon(-1, window, cx);
        }
    }

    fn on_next_dungeon(&mut self, _: &NextDungeon, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Sheet {
            self.switch_sheet(1, window, cx);
        } else {
            self.switch_dungeon(1, window, cx);
        }
    }

    /// Cycle the open dungeon within the campaign's `dungeons/` folder.
    /// The outgoing doc is saved first if dirty.
    pub fn switch_dungeon(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.session.campaign.clone() else {
            self.set_status("no campaign open (ctrl-O)", cx);
            return;
        };
        let campaign = crate::campaign::Campaign { root };
        let list = campaign.list_dungeons();
        if list.is_empty() {
            self.set_status("no dungeons in this campaign", cx);
            return;
        }
        let current = self
            .dungeon
            .as_ref()
            .and_then(|v| v.read(cx).doc().file.clone());
        let idx = list
            .iter()
            .position(|p| Some(p) == current.as_ref())
            .unwrap_or(0);
        let next = (idx as i32 + delta).rem_euclid(list.len() as i32) as usize;
        let path = list[next].clone();
        if let Some(view) = self.dungeon.take() {
            let id = view.entity_id();
            view.update(cx, |v, _| {
                v.save_if_dirty();
            });
            cx.global_mut::<Autosave>().unregister(id);
        }
        match DungeonDoc::load(&path) {
            Ok(doc) => {
                let name = doc.dungeon.name.clone();
                let view = self.build_dungeon_with(doc, Some(campaign.dungeons_dir()), cx);
                window.focus(&view.read(cx).focus_handle(cx), cx);
                self.set_status(format!("opened dungeon {name}"), cx);
            }
            Err(e) => self.set_status(format!("could not open {}: {e}", path.display()), cx),
        }
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
        // The mode views are campaign-scoped: rebuild them lazily so the new
        // campaign's docs (and their save dirs) take over.
        self.dungeon = None;
        self.sheet = None;
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

    #[allow(dead_code)] // read by tests; the tab bar will use it
    pub fn dungeon_view(&self) -> Option<&gpui::Entity<DungeonView>> {
        self.dungeon.as_ref()
    }

    #[allow(dead_code)] // read by sheet_view tests
    pub fn sheet_view(&self) -> Option<&gpui::Entity<SheetView>> {
        self.sheet.as_ref()
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
            .on_action(cx.listener(Self::on_prev_dungeon))
            .on_action(cx.listener(Self::on_next_dungeon))
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
                div().flex_1().flex().bg(gpui::rgb(0x24_24_30)).child(
                    match (self.mode, self.dungeon.clone(), self.sheet.clone()) {
                        (Mode::Dungeon, Some(view), _) => view.into_any_element(),
                        (Mode::Dungeon, None, _) => div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_2xl()
                            .child("Dungeon mode — open a campaign (ctrl-O)")
                            .into_any_element(),
                        (Mode::Sheet, _, Some(view)) => view.into_any_element(),
                        (Mode::Sheet, _, None) => div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_2xl()
                            .child("Sheet mode — open a campaign (ctrl-O)")
                            .into_any_element(),
                    },
                ),
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
        cx.update(|cx| {
            cx.bind_keys(keybindings());
            crate::autosave::Autosave::init(cx);
        });
        let window = cx.add_window(|_, cx| {
            RootView::new(cx, Session::default(), None, None, Library::default(), None)
        });
        window
            .update(cx, |view, window, cx| window.focus(&view.focus_handle, cx))
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
        cx.update(|cx| {
            cx.bind_keys(keybindings());
            crate::autosave::Autosave::init(cx);
        });
        let window = cx.add_window(|_, cx| {
            RootView::new(
                cx,
                Session::default(),
                Some(path.clone()),
                None,
                Library::default(),
                None,
            )
        });
        window
            .update(cx, |view, window, cx| window.focus(&view.focus_handle, cx))
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
