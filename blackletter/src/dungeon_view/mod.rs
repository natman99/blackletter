//! The dungeon map view: canvas rendering + full keyboard CRUD.
//!
//! Key model (mirrors the old tracker, plus keyboard door selection):
//! the cursor is a *cell*; `hjkl`/`yubn` move it 8-way; `d` arms a door and
//! the next direction (or `,`/`.` for stairs) places it; `shift`+direction
//! selects the door that way for editing (`t` trap, `s` secret, `c` open,
//! `L` lock cycle, `Del` delete); room toggles apply when the cursor is on a
//! room and no door is selected. `?` shows the full keymap in-app.

mod doc;
mod paint;

use std::path::PathBuf;

use glam::Vec2;
use gpui::{
    Context, FocusHandle, Focusable, KeyBinding, MouseButton, Pixels, Point, Render, SharedString,
    WeakEntity, Window, actions, div, prelude::*, px,
};

use blackletter_dungeon::dungeon::path;
use blackletter_dungeon::model::{
    Connection, Direction, Door, DoorId, Dungeon, FloorId, Lock, Room, RoomId, VerticalKind,
};

use crate::root::RootView;
pub use doc::DungeonDoc;
use paint::Geometry;

pub const DUNGEON_CTX: &str = "Dungeon";

actions!(
    dungeon,
    [
        MoveWest,
        MoveSouth,
        MoveNorth,
        MoveEast,
        MoveNw,
        MoveNe,
        MoveSw,
        MoveSe,
        SelectDoorWest,
        SelectDoorSouth,
        SelectDoorNorth,
        SelectDoorEast,
        SelectDoorNw,
        SelectDoorNe,
        SelectDoorSw,
        SelectDoorSe,
        ArmDoor,
        FloorUp,
        FloorDown,
        CycleFloorUp,
        CycleFloorDown,
        PartyHere,
        JumpParty,
        JumpEntrance,
        PathBetween,
        PathEntrance,
        DeleteThing,
        ToggleTrap,
        ToggleSecret,
        ToggleC,
        ToggleExit,
        ToggleEnemies,
        ToggleLoot,
        CycleLock,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        Undo,
        Redo,
        SaveDungeon,
        Cancel,
        ToggleHelp,
    ]
);

pub fn keybindings() -> Vec<KeyBinding> {
    let ctx = Some(DUNGEON_CTX);
    let mut kb: Vec<KeyBinding> = Vec::new();
    macro_rules! bind {
        ($($key:literal => $action:ident),* $(,)?) => {
            $( kb.push(KeyBinding::new($key, $action, ctx)); )*
        };
    }
    bind!(
        "h" => MoveWest,
        "j" => MoveSouth,
        "k" => MoveNorth,
        "l" => MoveEast,
        "y" => MoveNw,
        "u" => MoveNe,
        "b" => MoveSw,
        "n" => MoveSe,
        "shift-h" => SelectDoorWest,
        "shift-j" => SelectDoorSouth,
        "shift-k" => SelectDoorNorth,
        "shift-l" => SelectDoorEast,
        "shift-y" => SelectDoorNw,
        "shift-u" => SelectDoorNe,
        "shift-b" => SelectDoorSw,
        "shift-n" => SelectDoorSe,
        "d" => ArmDoor,
        "," => FloorUp,
        "." => FloorDown,
        "[" => CycleFloorUp,
        "]" => CycleFloorDown,
        "space" => PartyHere,
        "shift-t" => JumpParty,
        "g" => JumpEntrance,
        "p" => PathBetween,
        "shift-p" => PathEntrance,
        "delete" => DeleteThing,
        "backspace" => DeleteThing,
        "t" => ToggleTrap,
        "s" => ToggleSecret,
        "c" => ToggleC,
        "x" => ToggleExit,
        "e" => ToggleEnemies,
        "o" => ToggleLoot,
        "ctrl-l" => CycleLock,
        "+" => ZoomIn,
        "=" => ZoomReset,
        "-" => ZoomOut,
        "0" => ZoomReset,
        "ctrl-z" => Undo,
        "ctrl-shift-z" => Redo,
        "ctrl-y" => Redo,
        "ctrl-s" => SaveDungeon,
        "escape" => Cancel,
        "?" => ToggleHelp,
    );
    kb
}

/// Dark palette shared with the root chrome.
pub struct Palette;
impl Palette {
    fn hsla(h: f32, s: f32, l: f32) -> gpui::Hsla {
        gpui::Hsla { h, s, l, a: 1.0 }
    }
    pub fn bg() -> gpui::Hsla {
        Self::hsla(0.64, 0.24, 0.10)
    }
    pub fn room_fill() -> gpui::Hsla {
        Self::hsla(0.63, 0.22, 0.20)
    }
    pub fn room_border() -> gpui::Hsla {
        Self::hsla(0.62, 0.18, 0.38)
    }
    pub fn text() -> gpui::Hsla {
        Self::hsla(0.60, 0.10, 0.85)
    }
    pub fn muted() -> gpui::Hsla {
        Self::hsla(0.60, 0.10, 0.55)
    }
    pub fn accent() -> gpui::Hsla {
        Self::hsla(0.60, 0.70, 0.65)
    }
    pub fn danger() -> gpui::Hsla {
        Self::hsla(0.99, 0.65, 0.62)
    }
    pub fn warn() -> gpui::Hsla {
        Self::hsla(0.10, 0.75, 0.62)
    }
    pub fn party() -> gpui::Hsla {
        Self::hsla(0.33, 0.55, 0.60)
    }
    pub fn select() -> gpui::Hsla {
        Self::hsla(0.50, 0.65, 0.65)
    }
}

pub struct DungeonView {
    doc: DungeonDoc,
    /// Folder new saves land in (from the open campaign).
    dungeons_dir: Option<PathBuf>,
    cursor: glam::IVec2,
    active_floor: FloorId,
    selected_door: Option<DoorId>,
    pending_door: bool,
    path_overlay: Option<(Vec<RoomId>, bool)>,
    zoom: f32,
    pan: Vec2,
    geo: Option<Geometry>,
    show_help: bool,
    dragging: Option<Point<Pixels>>,
    focus_handle: FocusHandle,
    root: WeakEntity<RootView>,
}

impl DungeonView {
    pub fn new(
        doc: DungeonDoc,
        dungeons_dir: Option<PathBuf>,
        root: WeakEntity<RootView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let entrance = doc.dungeon.room(doc.dungeon.entrance);
        let (cursor, active_floor) = entrance
            .map(|r| (r.position, r.floor))
            .unwrap_or((glam::IVec2::ZERO, FloorId(0)));
        Self {
            doc,
            dungeons_dir,
            cursor,
            active_floor,
            selected_door: None,
            pending_door: false,
            path_overlay: None,
            zoom: 1.0,
            pan: Vec2::ZERO,
            geo: None,
            show_help: false,
            dragging: None,
            focus_handle: cx.focus_handle(),
            root,
        }
    }

    #[allow(dead_code)] // used by tests; the floor badge will read it too
    pub fn active_floor_id(&self) -> FloorId {
        self.active_floor
    }

    #[allow(dead_code)] // read by tests and the future tab bar
    pub fn doc(&self) -> &DungeonDoc {
        &self.doc
    }

    #[allow(dead_code)] // campaign switches land in Phase 2.5
    pub fn set_dungeons_dir(&mut self, dir: Option<PathBuf>) {
        self.dungeons_dir = dir;
    }

    fn report(&self, line: impl Into<SharedString>, cx: &mut Context<Self>) {
        let line = line.into();
        let root = self.root.clone();
        cx.defer(move |cx| {
            root.update(cx, |root, cx| root.set_status(line, cx)).ok();
        });
    }

    fn cursor_room(&self) -> Option<RoomId> {
        self.doc.dungeon.room_at(self.active_floor, self.cursor)
    }

    /// The single mutation path: snapshot/undo via the doc, drop the stale
    /// path overlay, repaint.
    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Dungeon)) {
        self.doc.edit(f);
        self.path_overlay = None;
        cx.notify();
    }

    /// Fallible mutation: reports the error to the status bar on failure.
    fn try_edit<T>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Dungeon) -> Result<T, blackletter_dungeon::model::DungeonError>,
    ) -> Option<T> {
        match self.doc.try_edit(f) {
            Ok(t) => {
                self.path_overlay = None;
                cx.notify();
                Some(t)
            }
            Err(e) => {
                self.report(format!("{e}"), cx);
                None
            }
        }
    }

    fn move_cursor(&mut self, dir: Direction, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_door {
            self.place_horizontal(dir, cx);
            return;
        }
        self.cursor += dir.offset();
        self.selected_door = None;
        self.path_overlay = None;
        self.keep_cursor_visible(window);
        cx.notify();
    }

    fn place_horizontal(&mut self, dir: Direction, cx: &mut Context<Self>) {
        self.pending_door = false;
        let Some(from) = self.cursor_room() else {
            self.report("doors start from a room, not an empty cell", cx);
            return;
        };
        if let Some(target) = self.try_edit(cx, |d| d.add_horizontal_door(from, dir)) {
            self.report(format!("door {dir:?} placed → room {target}"), cx);
        }
    }

    fn place_vertical(&mut self, up: bool, cx: &mut Context<Self>) {
        self.pending_door = false;
        let Some(from) = self.cursor_room() else {
            self.report("stairs start from a room, not an empty cell", cx);
            return;
        };
        let target = self.try_edit(cx, |d| d.add_vertical_door(from, up, VerticalKind::Stairs));
        if let Some(target) = target {
            self.report(
                format!("stairs {} → room {target}", if up { "up" } else { "down" }),
                cx,
            );
        }
    }

    fn select_door(&mut self, dir: Direction, cx: &mut Context<Self>) {
        let Some(from) = self.cursor_room() else {
            self.selected_door = None;
            cx.notify();
            return;
        };
        // Doors are stored once per connection, from either side.
        let found = self.doc.dungeon.doors.iter().find(|d| {
            (d.from == from && matches!(d.connection, Connection::Horizontal(dd) if dd == dir))
                || (d.target == from
                    && matches!(d.connection, Connection::Horizontal(dd) if dd == dir.opposite()))
        });
        match found {
            Some(d) => {
                self.selected_door = Some(d.id);
                self.report(
                    format!("door {} selected — t/s/c/L edit, Del delete", d.id),
                    cx,
                );
            }
            None => {
                self.selected_door = None;
                self.report("no door that way", cx);
            }
        }
        cx.notify();
    }

    fn edit_door(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Door) -> String) {
        let Some(id) = self.selected_door else {
            self.report("no door selected (shift+hjkl selects one)", cx);
            return;
        };
        let mut msg = String::new();
        let edited = self.doc.dungeon.doors.iter().any(|d| d.id == id);
        if !edited {
            self.selected_door = None;
            self.report("that door is gone", cx);
            return;
        }
        self.edit(cx, |d| {
            if let Some(door) = d.doors.iter_mut().find(|d| d.id == id) {
                msg = f(door);
            }
        });
        self.report(format!("door {id}: {msg}"), cx);
    }

    fn edit_room(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Room) -> String) {
        let Some(rid) = self.cursor_room() else {
            self.report("no room under the cursor", cx);
            return;
        };
        let mut msg = String::new();
        self.edit(cx, |d| {
            if let Some(r) = d.room_mut(rid) {
                msg = f(r);
            }
        });
        self.report(format!("room {rid}: {msg}"), cx);
    }

    fn step_floor(&mut self, delta: i32, wrap: bool, cx: &mut Context<Self>) {
        let n = self.doc.dungeon.floors.len() as i32;
        if n == 0 {
            return;
        }
        let idx = self
            .doc
            .dungeon
            .floors
            .iter()
            .position(|f| f.id == self.active_floor)
            .unwrap_or(0) as i32;
        let next = if wrap {
            (idx + delta).rem_euclid(n)
        } else {
            (idx + delta).clamp(0, n - 1)
        };
        self.active_floor = self.doc.dungeon.floors[next as usize].id;
        self.selected_door = None;
        cx.notify();
    }

    fn jump_to(&mut self, rid: RoomId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(r) = self.doc.dungeon.room(rid) {
            self.cursor = r.position;
            self.active_floor = r.floor;
            self.center_cursor(window);
            cx.notify();
        }
    }

    /// Pan so the cursor cell sits at the canvas center.
    fn center_cursor(&mut self, window: &mut Window) {
        let Some(geo) = self.geo else { return };
        let b = window.bounds();
        let c = paint::cell_center(geo, self.cursor);
        self.pan += glam::Vec2::new(
            f32::from(b.origin.x + b.size.width / 2.0 - c.x),
            f32::from(b.origin.y + b.size.height / 2.0 - c.y),
        );
    }

    fn show_path(&mut self, from: RoomId, to: RoomId, cx: &mut Context<Self>) {
        match path::find_path(&self.doc.dungeon, from, to) {
            Some(p) => {
                let safe = path::path_is_safe(&self.doc.dungeon, &p);
                self.report(
                    format!(
                        "route to room {to}: {} rooms, {}",
                        p.len(),
                        if safe { "safe" } else { "UNSAFE" }
                    ),
                    cx,
                );
                self.path_overlay = Some((p, safe));
                cx.notify();
            }
            None => self.report("no route", cx),
        }
    }

    fn keep_cursor_visible(&mut self, window: &mut Window) {
        let Some(geo) = self.geo else { return };
        let bounds = window.bounds();
        let rect = paint::cell_rect(geo, self.cursor);
        let mut pan = self.pan;
        if rect.origin.x < bounds.origin.x {
            pan.x += f32::from(bounds.origin.x - rect.origin.x) + 20.0;
        }
        if rect.origin.y < bounds.origin.y {
            pan.y += f32::from(bounds.origin.y - rect.origin.y) + 20.0;
        }
        let right = rect.origin.x + rect.size.width;
        if right > bounds.origin.x + bounds.size.width {
            pan.x -= f32::from(right - bounds.origin.x - bounds.size.width) + 20.0;
        }
        let bottom = rect.origin.y + rect.size.height;
        if bottom > bounds.origin.y + bounds.size.height {
            pan.y -= f32::from(bottom - bounds.origin.y - bounds.size.height) + 20.0;
        }
        self.pan = pan;
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        match self.dungeons_dir.clone() {
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
            None => self.report("open a campaign (ctrl-O) before saving dungeons", cx),
        }
    }

    /// Autosave hook: write if dirty and a campaign dir is known.
    pub fn save_if_dirty(&mut self) -> bool {
        if !self.doc.dirty {
            return false;
        }
        let Some(dir) = self.dungeons_dir.clone() else {
            return false;
        };
        self.doc.save_to_dir(&dir).is_ok()
    }

    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.zoom = (self.zoom * factor).clamp(0.2, 8.0);
        cx.notify();
    }

    // ---- action handlers ---------------------------------------------------

    fn on_move(&mut self, dir: Direction, window: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(dir, window, cx);
    }

    fn on_select_door(&mut self, dir: Direction, _: &mut Window, cx: &mut Context<Self>) {
        self.select_door(dir, cx);
    }

    fn on_arm_door(&mut self, _: &ArmDoor, _: &mut Window, cx: &mut Context<Self>) {
        self.pending_door = false;
        if self.cursor_room().is_some() {
            self.pending_door = true;
            self.report(
                "door armed — hjkl/yubn place it, ,/. stairs, esc cancels",
                cx,
            );
        } else {
            self.report("arm doors from a room, not an empty cell", cx);
        }
    }

    fn on_floor_up(&mut self, _: &FloorUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.pending_door {
            self.place_vertical(true, cx);
        } else {
            self.step_floor(-1, false, cx);
        }
    }

    fn on_floor_down(&mut self, _: &FloorDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.pending_door {
            self.place_vertical(false, cx);
        } else {
            self.step_floor(1, false, cx);
        }
    }

    fn on_cycle_floor_up(&mut self, _: &CycleFloorUp, _: &mut Window, cx: &mut Context<Self>) {
        self.step_floor(-1, true, cx);
    }

    fn on_cycle_floor_down(&mut self, _: &CycleFloorDown, _: &mut Window, cx: &mut Context<Self>) {
        self.step_floor(1, true, cx);
    }

    fn on_party_here(&mut self, _: &PartyHere, _: &mut Window, cx: &mut Context<Self>) {
        match self.cursor_room() {
            Some(rid) => {
                self.edit(cx, |d| d.set_party_position(Some(rid)));
                self.report(format!("party → room {rid}"), cx);
            }
            None => self.report("no room under the cursor", cx),
        }
    }

    fn on_jump_party(&mut self, _: &JumpParty, window: &mut Window, cx: &mut Context<Self>) {
        match self.doc.dungeon.party_position {
            Some(rid) => self.jump_to(rid, window, cx),
            None => self.report("party is off the map", cx),
        }
    }

    fn on_jump_entrance(&mut self, _: &JumpEntrance, window: &mut Window, cx: &mut Context<Self>) {
        let e = self.doc.dungeon.entrance;
        self.jump_to(e, window, cx);
    }

    fn on_path_between(&mut self, _: &PathBetween, _: &mut Window, cx: &mut Context<Self>) {
        let from = self
            .doc
            .dungeon
            .party_position
            .unwrap_or(self.doc.dungeon.entrance);
        match self.cursor_room() {
            Some(to) => self.show_path(from, to, cx),
            None => self.report("route to a room — move the cursor onto one", cx),
        }
    }

    fn on_path_entrance(&mut self, _: &PathEntrance, _: &mut Window, cx: &mut Context<Self>) {
        let e = self.doc.dungeon.entrance;
        match self.cursor_room() {
            Some(from) => self.show_path(from, e, cx),
            None => self.report("route from a room — move the cursor onto one", cx),
        }
    }

    fn on_delete(&mut self, _: &DeleteThing, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_door {
            if self.try_edit(cx, |d| d.try_delete_door(id)).is_some() {
                self.selected_door = None;
                self.report(format!("door {id} deleted"), cx);
            }
        } else if let Some(rid) = self.cursor_room()
            && self.try_edit(cx, |d| d.try_delete_room(rid)).is_some()
        {
            self.report(format!("room {rid} deleted"), cx);
        }
    }

    fn on_toggle_trap(&mut self, _: &ToggleTrap, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_door.is_some() {
            self.edit_door(cx, |d| {
                d.trapped = !d.trapped;
                format!("trapped {}", if d.trapped { "yes" } else { "no" })
            });
        } else {
            self.edit_room(cx, |r| {
                r.trap = r.trap.cycle();
                format!("trap {:?}", r.trap)
            });
        }
    }

    fn on_toggle_secret(&mut self, _: &ToggleSecret, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_door.is_some() {
            self.edit_door(cx, |d| {
                d.secret = !d.secret;
                format!("secret {}", if d.secret { "yes" } else { "no" })
            });
        } else {
            self.edit_room(cx, |r| {
                r.secret = !r.secret;
                format!("secret {}", if r.secret { "yes" } else { "no" })
            });
        }
    }

    fn on_toggle_c(&mut self, _: &ToggleC, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_door.is_some() {
            self.edit_door(cx, |d| {
                d.open = !d.open;
                format!("open {}", if d.open { "yes" } else { "no" })
            });
        } else {
            self.edit_room(cx, |r| {
                r.searched = !r.searched;
                format!("searched {}", if r.searched { "yes" } else { "no" })
            });
        }
    }

    fn on_toggle_exit(&mut self, _: &ToggleExit, _: &mut Window, cx: &mut Context<Self>) {
        self.edit_room(cx, |r| {
            r.is_exit = !r.is_exit;
            format!("exit {}", if r.is_exit { "yes" } else { "no" })
        });
    }

    fn on_toggle_enemies(&mut self, _: &ToggleEnemies, _: &mut Window, cx: &mut Context<Self>) {
        self.edit_room(cx, |r| {
            r.enemies = r.enemies.cycle();
            format!("enemies {:?}", r.enemies)
        });
    }

    fn on_toggle_loot(&mut self, _: &ToggleLoot, _: &mut Window, cx: &mut Context<Self>) {
        self.edit_room(cx, |r| {
            r.loot = r.loot.cycle();
            format!("loot {:?}", r.loot)
        });
    }

    fn on_cycle_lock(&mut self, _: &CycleLock, _: &mut Window, cx: &mut Context<Self>) {
        self.edit_door(cx, |d| {
            d.lock = match d.lock {
                None => Some(Lock::SRD),
                Some(l) if l.dc == Lock::SRD.dc => Some(Lock::HARD),
                Some(_) => None,
            };
            match d.lock {
                Some(Lock { dc }) => format!("locked DC {dc}"),
                None => "unlocked".into(),
            }
        });
    }

    fn on_zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(1.25, cx);
    }

    fn on_zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(1.0 / 1.25, cx);
    }

    fn on_zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
        cx.notify();
    }

    fn on_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.undo() {
            self.path_overlay = None;
            self.selected_door = None;
            self.report("undone", cx);
            cx.notify();
        } else {
            self.report("nothing to undo", cx);
        }
    }

    fn on_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.redo() {
            self.path_overlay = None;
            self.selected_door = None;
            self.report("redone", cx);
            cx.notify();
        } else {
            self.report("nothing to redo", cx);
        }
    }

    fn on_save(&mut self, _: &SaveDungeon, _: &mut Window, cx: &mut Context<Self>) {
        self.save(cx);
    }

    fn on_cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        self.pending_door = false;
        self.path_overlay = None;
        self.selected_door = None;
        cx.notify();
    }

    fn on_toggle_help(&mut self, _: &ToggleHelp, _: &mut Window, cx: &mut Context<Self>) {
        self.show_help = !self.show_help;
        cx.notify();
    }
}

impl Focusable for DungeonView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DungeonView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let win_bounds = window.bounds();
        // Geometry for hit-testing, from the window bounds (the canvas fills
        // the window, so the paint closure sees the same bounds).
        let geo = match paint::floor_bounds(&self.doc.dungeon, self.active_floor) {
            Some((min, max)) => paint::fit(win_bounds, min, max, self.zoom, self.pan),
            None => paint::fit(win_bounds, self.cursor, self.cursor, self.zoom, self.pan),
        };
        self.geo = Some(geo);

        let frame = paint::Frame {
            d: self.doc.dungeon.clone(),
            floor: self.active_floor,
            cursor: self.cursor,
            cursor_room: self.cursor_room(),
            selected_door: self.selected_door,
            pending_door: self.pending_door,
            path: self.path_overlay.as_ref().map(|(p, _)| p.clone()),
            path_safe: self.path_overlay.as_ref().is_some_and(|(_, s)| *s),
        };
        let (zoom, pan) = (self.zoom, self.pan);
        let cursor = self.cursor;

        let click = cx.listener(
            |view: &mut DungeonView, ev: &gpui::ClickEvent, window, cx| {
                view.focus_handle.focus(window, cx);
                let Some(geo) = view.geo else { return };
                let cell = paint::cell_at(geo, ev.position());
                view.cursor = cell;
                view.selected_door = None;
                if let Some(rid) = view.cursor_room() {
                    view.doc.edit(|d| d.set_party_position(Some(rid)));
                    view.path_overlay = None;
                }
                view.keep_cursor_visible(window);
                cx.notify();
            },
        );
        let down_mid = cx.listener(|view: &mut DungeonView, ev: &gpui::MouseDownEvent, _, _| {
            view.dragging = Some(ev.position);
        });
        let down_right = cx.listener(|view: &mut DungeonView, ev: &gpui::MouseDownEvent, _, _| {
            view.dragging = Some(ev.position);
        });
        let up_mid = cx.listener(|view: &mut DungeonView, _: &gpui::MouseUpEvent, _, _| {
            view.dragging = None;
        });
        let up_right = cx.listener(|view: &mut DungeonView, _: &gpui::MouseUpEvent, _, _| {
            view.dragging = None;
        });
        let moved = cx.listener(|view: &mut DungeonView, ev: &gpui::MouseMoveEvent, _, cx| {
            if let Some(start) = view.dragging {
                view.pan += Vec2::new(
                    f32::from(ev.position.x) - f32::from(start.x),
                    f32::from(ev.position.y) - f32::from(start.y),
                );
                view.dragging = Some(ev.position);
                cx.notify();
            }
        });
        let wheel = cx.listener(
            |view: &mut DungeonView, ev: &gpui::ScrollWheelEvent, _, cx| {
                let y = match ev.delta {
                    gpui::ScrollDelta::Lines(d) => d.y,
                    gpui::ScrollDelta::Pixels(d) => f32::from(d.y) / 20.0,
                };
                if y != 0.0 {
                    view.zoom_by(1.0 + y * 0.15, cx);
                }
            },
        );

        let floors: Vec<(SharedString, bool)> = self
            .doc
            .dungeon
            .floors
            .iter()
            .map(|f| {
                (
                    format!("{} {}", f.id.0 + 1, f.name).into(),
                    f.id == self.active_floor,
                )
            })
            .collect();
        let title = format!(
            "{}{} — zoom {:.0}% — floor {}",
            self.doc.dungeon.name,
            if self.doc.dirty { " *" } else { "" },
            self.zoom * 100.0,
            self.active_floor.0 + 1,
        );
        let show_help = self.show_help;

        div()
            .key_context(DUNGEON_CTX)
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .bg(Palette::bg())
            .overflow_hidden()
            .id("dungeon-canvas")
            .on_click(click)
            .on_mouse_down(MouseButton::Middle, down_mid)
            .on_mouse_down(MouseButton::Right, down_right)
            .on_mouse_up(MouseButton::Middle, up_mid)
            .on_mouse_up(MouseButton::Right, up_right)
            .on_mouse_move(moved)
            .on_scroll_wheel(wheel)
            .on_action(cx.listener(|v, _: &MoveWest, w, c| v.on_move(Direction::W, w, c)))
            .on_action(cx.listener(|v, _: &MoveSouth, w, c| v.on_move(Direction::S, w, c)))
            .on_action(cx.listener(|v, _: &MoveNorth, w, c| v.on_move(Direction::N, w, c)))
            .on_action(cx.listener(|v, _: &MoveEast, w, c| v.on_move(Direction::E, w, c)))
            .on_action(cx.listener(|v, _: &MoveNw, w, c| v.on_move(Direction::NW, w, c)))
            .on_action(cx.listener(|v, _: &MoveNe, w, c| v.on_move(Direction::NE, w, c)))
            .on_action(cx.listener(|v, _: &MoveSw, w, c| v.on_move(Direction::SW, w, c)))
            .on_action(cx.listener(|v, _: &MoveSe, w, c| v.on_move(Direction::SE, w, c)))
            .on_action(
                cx.listener(|v, _: &SelectDoorWest, w, c| v.on_select_door(Direction::W, w, c)),
            )
            .on_action(
                cx.listener(|v, _: &SelectDoorSouth, w, c| v.on_select_door(Direction::S, w, c)),
            )
            .on_action(
                cx.listener(|v, _: &SelectDoorNorth, w, c| v.on_select_door(Direction::N, w, c)),
            )
            .on_action(
                cx.listener(|v, _: &SelectDoorEast, w, c| v.on_select_door(Direction::E, w, c)),
            )
            .on_action(
                cx.listener(|v, _: &SelectDoorNw, w, c| v.on_select_door(Direction::NW, w, c)),
            )
            .on_action(
                cx.listener(|v, _: &SelectDoorNe, w, c| v.on_select_door(Direction::NE, w, c)),
            )
            .on_action(
                cx.listener(|v, _: &SelectDoorSw, w, c| v.on_select_door(Direction::SW, w, c)),
            )
            .on_action(
                cx.listener(|v, _: &SelectDoorSe, w, c| v.on_select_door(Direction::SE, w, c)),
            )
            .on_action(cx.listener(|v, a: &ArmDoor, w, c| v.on_arm_door(a, w, c)))
            .on_action(cx.listener(|v, a: &FloorUp, w, c| v.on_floor_up(a, w, c)))
            .on_action(cx.listener(|v, a: &FloorDown, w, c| v.on_floor_down(a, w, c)))
            .on_action(cx.listener(|v, a: &CycleFloorUp, w, c| v.on_cycle_floor_up(a, w, c)))
            .on_action(cx.listener(|v, a: &CycleFloorDown, w, c| v.on_cycle_floor_down(a, w, c)))
            .on_action(cx.listener(|v, a: &PartyHere, w, c| v.on_party_here(a, w, c)))
            .on_action(cx.listener(|v, a: &JumpParty, w, c| v.on_jump_party(a, w, c)))
            .on_action(cx.listener(|v, a: &JumpEntrance, w, c| v.on_jump_entrance(a, w, c)))
            .on_action(cx.listener(|v, a: &PathBetween, w, c| v.on_path_between(a, w, c)))
            .on_action(cx.listener(|v, a: &PathEntrance, w, c| v.on_path_entrance(a, w, c)))
            .on_action(cx.listener(|v, a: &DeleteThing, w, c| v.on_delete(a, w, c)))
            .on_action(cx.listener(|v, a: &ToggleTrap, w, c| v.on_toggle_trap(a, w, c)))
            .on_action(cx.listener(|v, a: &ToggleSecret, w, c| v.on_toggle_secret(a, w, c)))
            .on_action(cx.listener(|v, a: &ToggleC, w, c| v.on_toggle_c(a, w, c)))
            .on_action(cx.listener(|v, a: &ToggleExit, w, c| v.on_toggle_exit(a, w, c)))
            .on_action(cx.listener(|v, a: &ToggleEnemies, w, c| v.on_toggle_enemies(a, w, c)))
            .on_action(cx.listener(|v, a: &ToggleLoot, w, c| v.on_toggle_loot(a, w, c)))
            .on_action(cx.listener(|v, a: &CycleLock, w, c| v.on_cycle_lock(a, w, c)))
            .on_action(cx.listener(|v, a: &ZoomIn, w, c| v.on_zoom_in(a, w, c)))
            .on_action(cx.listener(|v, a: &ZoomOut, w, c| v.on_zoom_out(a, w, c)))
            .on_action(cx.listener(|v, a: &ZoomReset, w, c| v.on_zoom_reset(a, w, c)))
            .on_action(cx.listener(|v, a: &Undo, w, c| v.on_undo(a, w, c)))
            .on_action(cx.listener(|v, a: &Redo, w, c| v.on_redo(a, w, c)))
            .on_action(cx.listener(|v, a: &SaveDungeon, w, c| v.on_save(a, w, c)))
            .on_action(cx.listener(|v, a: &Cancel, w, c| v.on_cancel(a, w, c)))
            .on_action(cx.listener(|v, a: &ToggleHelp, w, c| v.on_toggle_help(a, w, c)))
            .child(
                gpui::canvas(
                    move |_, _, _| frame,
                    move |bounds, frame, window, cx| {
                        let geo = match paint::floor_bounds(&frame.d, frame.floor) {
                            Some((min, max)) => paint::fit(bounds, min, max, zoom, pan),
                            None => paint::fit(bounds, cursor, cursor, zoom, pan),
                        };
                        paint::paint(&frame, geo, window, cx);
                    },
                )
                .size_full(),
            )
            .children(floors.iter().enumerate().map(|(i, (name, active))| {
                div()
                    .absolute()
                    .top_2()
                    .left(px(8.0 + i as f32 * 110.0))
                    .px_2()
                    .py_1()
                    .rounded(px(4.0))
                    .bg(if *active {
                        Palette::accent()
                    } else {
                        Palette::room_fill()
                    })
                    .text_color(if *active {
                        Palette::bg()
                    } else {
                        Palette::muted()
                    })
                    .text_xs()
                    .child(name.clone())
            }))
            .child(
                div()
                    .absolute()
                    .top_2()
                    .right_2()
                    .text_xs()
                    .text_color(Palette::muted())
                    .child(title),
            )
            .when(show_help, |this| {
                this.child(
                    div()
                        .absolute()
                        .top_8()
                        .right_2()
                        .w(px(360.0))
                        .p_3()
                        .rounded(px(6.0))
                        .bg(Palette::room_fill())
                        .border_1()
                        .border_color(Palette::room_border())
                        .text_xs()
                        .text_color(Palette::text())
                        .child(HELP),
                )
            })
    }
}

const HELP: &str = r#"hjkl yubn     move cursor (8-way)
d + dir       place door (auto-creates room)
d + ,/.       stairs up / down
shift+hjkl    select door that way
  t s c ^L    door: trap / secret / open / lock DC
  Del         delete selected door (or cursor room)
space         party here        shift-T jump to party
g             jump entrance     , . [ ] switch floors
p             route party→cursor
shift-P       route cursor→entrance
t s c x e o   room: trap secret searched exit enemies loot
+ - 0         zoom in / out / reset   drag mid/right: pan
ctrl-z / ctrl-shift-z  undo / redo    ctrl-s save
?             this help               esc cancel/clear"#;

/// Constructor used by the root view: builds the entity and wires actions.
pub fn build(
    doc: DungeonDoc,
    dungeons_dir: Option<PathBuf>,
    root: WeakEntity<RootView>,
    cx: &mut Context<RootView>,
) -> gpui::Entity<DungeonView> {
    cx.new(|cx| DungeonView::new(doc, dungeons_dir, root, cx))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::RootView;
    use crate::session::Session;
    use gpui::{Entity, TestAppContext, WindowHandle};

    fn open_dungeon(cx: &mut TestAppContext) -> (WindowHandle<RootView>, Entity<RootView>) {
        cx.update(|cx| {
            cx.bind_keys(crate::root::keybindings());
            cx.bind_keys(keybindings());
            crate::autosave::Autosave::init(cx);
        });
        let window = cx.add_window(|_, cx| RootView::new(cx, Session::default(), None));
        window
            .update(cx, |view, window, cx| window.focus(view.focus_handle(), cx))
            .unwrap();
        cx.simulate_keystrokes(window.into(), "ctrl-2");
        cx.run_until_parked();
        (window, window.root(cx).unwrap())
    }

    fn dungeon(
        cx: &TestAppContext,
        root: &Entity<RootView>,
    ) -> blackletter_dungeon::model::Dungeon {
        cx.read(|cx| {
            root.read(cx)
                .dungeon_view()
                .expect("dungeon view")
                .read(cx)
                .doc()
                .dungeon
                .clone()
        })
    }

    #[gpui::test]
    fn keys_build_and_edit_a_dungeon(cx: &mut TestAppContext) {
        let (window, root) = open_dungeon(cx);

        // d + l: arm a door, place it east (auto-creates the room).
        cx.simulate_keystrokes(window.into(), "d l");
        cx.run_until_parked();
        let d = dungeon(cx, &root);
        assert_eq!(d.rooms.len(), 2);
        assert_eq!(d.doors.len(), 1);

        // Move onto the new room, toggle enemies, then undo the toggle.
        cx.simulate_keystrokes(window.into(), "l e");
        cx.run_until_parked();
        let d = dungeon(cx, &root);
        assert_eq!(
            d.room_at(d.floors[0].id, glam::IVec2::new(1, 0))
                .and_then(|id| d.room(id))
                .map(|r| r.enemies),
            Some(blackletter_dungeon::model::EnemyState::Active)
        );
        cx.simulate_keystrokes(window.into(), "ctrl-z");
        cx.run_until_parked();
        let d = dungeon(cx, &root);
        assert_eq!(d.rooms.len(), 2);

        // Select the door east and cycle its lock to the SRD DC.
        cx.simulate_keystrokes(window.into(), "h shift-l ctrl-l");
        cx.run_until_parked();
        let d = dungeon(cx, &root);
        assert_eq!(d.doors[0].lock, Some(Lock::SRD));

        // Delete the room under the cursor.
        cx.simulate_keystrokes(window.into(), "l delete");
        cx.run_until_parked();
        let d = dungeon(cx, &root);
        assert_eq!(d.rooms.len(), 1);
    }

    #[gpui::test]
    fn stairs_and_floor_switching(cx: &mut TestAppContext) {
        let (window, root) = open_dungeon(cx);
        // d + . : stairs down from the entrance.
        cx.simulate_keystrokes(window.into(), "d .");
        cx.run_until_parked();
        let d = dungeon(cx, &root);
        assert_eq!(d.floors.len(), 2);
        assert_eq!(d.rooms.len(), 2);

        // . steps to floor 2; the new room is there at the same cell.
        cx.simulate_keystrokes(window.into(), ".");
        cx.run_until_parked();
        let view = cx.read(|cx| root.read(cx).dungeon_view().unwrap().clone());
        let floor_rooms = cx.read(|cx| {
            let v = view.read(cx);
            v.doc()
                .dungeon
                .rooms
                .iter()
                .filter(|r| r.floor == v.active_floor_id())
                .count()
        });
        assert_eq!(floor_rooms, 1);
    }
}
