//! Canvas geometry + painting for the dungeon map.
//!
//! Geometry contract: the current floor's rooms define a grid bounding box;
//! `fit` maps grid cells to screen pixels (fit-to-window × zoom, centered,
//! plus pan). The same `Geometry` drives painting and click hit-testing, so
//! they can never disagree.

use blackletter_dungeon::model::{
    Connection, Direction, Dungeon, EnemyState, FloorId, Lock, Room, RoomId, TrapState,
};
use glam::Vec2;
use gpui::{Bounds, Corners, Edges, Hsla, PathBuilder, Pixels, Point, Window, px};

use crate::dungeon_view::Palette;

/// Grid cell size in logical px at zoom 1.0 before fit-scaling; rooms are
/// drawn as squares of `cell` with a small gutter between them.
const GUTTER: f32 = 0.14;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    /// Screen position of the top-left corner of grid cell (min.x, min.y).
    pub origin: Point<Pixels>,
    /// Grid min cell (inclusive).
    pub min: glam::IVec2,
    /// Cell pitch (room square + gutter).
    pub cell: Pixels,
}

/// Grid bounding box of the rooms on `floor`, or `None` for an empty floor.
pub fn floor_bounds(d: &Dungeon, floor: FloorId) -> Option<(glam::IVec2, glam::IVec2)> {
    let mut it = d.rooms.iter().filter(|r| r.floor == floor);
    let first = it.next()?;
    let (mut min, mut max) = (first.position, first.position);
    for r in it {
        min = min.min(r.position);
        max = max.max(r.position);
    }
    Some((min, max))
}

/// Fit the floor's grid into `bounds`, applying zoom and pan.
pub fn fit(
    bounds: Bounds<Pixels>,
    min: glam::IVec2,
    max: glam::IVec2,
    zoom: f32,
    pan: Vec2,
) -> Geometry {
    let gw = (max.x - min.x + 1) as f32;
    let gh = (max.y - min.y + 1) as f32;
    let bw = f32::from(bounds.size.width);
    let bh = f32::from(bounds.size.height);
    let fit_cell = (bw / gw).min(bh / gh).max(8.0);
    let cell = fit_cell * zoom;
    let origin = Point {
        x: bounds.origin.x + (bounds.size.width - px(gw * cell)) / 2.0 + px(pan.x),
        y: bounds.origin.y + (bounds.size.height - px(gh * cell)) / 2.0 + px(pan.y),
    };
    Geometry {
        origin,
        min,
        cell: px(cell),
    }
}

/// The grid cell under a screen point.
pub fn cell_at(g: Geometry, p: Point<Pixels>) -> glam::IVec2 {
    let x = ((f32::from(p.x) - f32::from(g.origin.x)) / f32::from(g.cell)).floor();
    let y = ((f32::from(p.y) - f32::from(g.origin.y)) / f32::from(g.cell)).floor();
    g.min + glam::IVec2::new(x as i32, y as i32)
}

/// The room square (cell minus gutter) for a grid cell.
pub fn cell_rect(g: Geometry, c: glam::IVec2) -> Bounds<Pixels> {
    let pitch = f32::from(g.cell);
    let side = pitch * (1.0 - GUTTER);
    let off = pitch * GUTTER / 2.0;
    Bounds {
        origin: Point {
            x: g.origin.x + px((c.x - g.min.x) as f32 * pitch + off),
            y: g.origin.y + px((c.y - g.min.y) as f32 * pitch + off),
        },
        size: gpui::size(px(side), px(side)),
    }
}

/// The center of a room square.
pub fn cell_center(g: Geometry, c: glam::IVec2) -> Point<Pixels> {
    let r = cell_rect(g, c);
    Point {
        x: r.origin.x + r.size.width / 2.0,
        y: r.origin.y + r.size.height / 2.0,
    }
}

fn hsla(h: f32, s: f32, l: f32, a: f32) -> Hsla {
    Hsla { h, s, l, a }
}

/// Room fill/border for its state.
pub fn room_colors(room: &Room, selected: bool, party: bool) -> (Hsla, Hsla, f32) {
    let mut fill = Palette::room_fill();
    let mut border = Palette::room_border();
    let mut bw: f32 = 1.0;
    if room.secret && !room.searched {
        border = hsla(0.58, 0.35, 0.55, 1.0); // muted violet
    }
    if room.searched {
        fill = hsla(0.62, 0.12, 0.16, 1.0); // dimmed slate
    }
    match room.enemies {
        EnemyState::Active => {
            border = Palette::danger();
            bw = 2.0;
        }
        EnemyState::Cleared => fill = hsla(0.36, 0.25, 0.18, 1.0),
        EnemyState::Unknown => {}
    }
    if room.trap == TrapState::Active {
        border = Palette::warn();
        bw = bw.max(2.0);
    }
    if room.is_exit {
        border = Palette::accent();
        bw = bw.max(2.0);
    }
    if party {
        bw = bw.max(3.0);
        border = Palette::party();
    }
    if selected {
        border = Palette::select();
        bw = bw.max(3.0);
    }
    (fill, border, bw)
}

/// Paint everything for one frame. `frame` is an owned snapshot taken in
/// `render`; `geo` is recomputed in the paint closure from the real bounds.
pub struct Frame {
    pub d: Dungeon,
    pub floor: FloorId,
    pub cursor: glam::IVec2,
    pub cursor_room: Option<RoomId>,
    pub selected_door: Option<blackletter_dungeon::model::DoorId>,
    pub pending_door: bool,
    pub path: Option<Vec<RoomId>>,
    pub path_safe: bool,
}

pub fn paint(frame: &Frame, g: Geometry, window: &mut Window, cx: &mut gpui::App) {
    // Rooms (current floor only).
    for room in frame.d.rooms.iter().filter(|r| r.floor == frame.floor) {
        let rect = cell_rect(g, room.position);
        let party = frame.d.party_position == Some(room.id);
        let (fill, border, bw) = room_colors(room, frame.cursor_room == Some(room.id), party);
        window.paint_quad(gpui::PaintQuad {
            bounds: rect,
            corner_radii: Corners::all(px(6.0)),
            background: fill.into(),
            border_widths: Edges::all(px(bw)),
            border_color: border,
            border_style: if room.secret && !room.searched {
                gpui::BorderStyle::Dashed
            } else {
                gpui::BorderStyle::Solid
            },
        });

        // Room name + id label.
        let label = match &room.name {
            Some(n) => format!("{} ·{}", n, room.id),
            None => format!("#{}", room.id),
        };
        paint_label(
            window,
            cx,
            rect.origin.x + px(6.0),
            rect.origin.y + px(5.0),
            &label,
            11.0,
            Palette::text(),
        );

        // State glyphs (bottom strip): E nemies / T rap / L oot / X exit / S ecret.
        let mut glyphs = String::new();
        if room.enemies != EnemyState::Unknown {
            glyphs.push_str(if room.enemies == EnemyState::Cleared {
                "e✓"
            } else {
                "e!"
            });
        }
        if room.trap != TrapState::None {
            glyphs.push_str(if room.trap == TrapState::Disarmed {
                "t✓"
            } else {
                "t!"
            });
        }
        if room.loot != blackletter_dungeon::model::LootState::Unknown {
            glyphs.push_str(
                if room.loot == blackletter_dungeon::model::LootState::Taken {
                    "o✓"
                } else {
                    "o!"
                },
            );
        }
        if room.is_exit {
            glyphs.push_str(" x");
        }
        if !glyphs.is_empty() {
            paint_label(
                window,
                cx,
                rect.origin.x + px(6.0),
                rect.origin.y + rect.size.height - px(18.0),
                &glyphs,
                11.0,
                Palette::muted(),
            );
        }
    }

    // Doors whose *from* room is on this floor. Batched per color class.
    let mut normal = PathBuilder::stroke(px(2.0));
    let mut locked = PathBuilder::stroke(px(3.0));
    let mut trapped = PathBuilder::stroke(px(3.0));
    let mut secret = PathBuilder::stroke(px(2.0)).dash_array(&[px(4.0), px(3.0)]);
    let mut sel = PathBuilder::stroke(px(3.5));
    let mut normal_n = 0;
    let mut locked_n = 0;
    let mut trapped_n = 0;
    let mut secret_n = 0;
    let mut sel_n = 0;

    for door in &frame.d.doors {
        let Some(from) = frame.d.room(door.from) else {
            continue;
        };
        if from.floor != frame.floor {
            continue;
        }
        let Some(target) = frame.d.room(door.target) else {
            continue;
        };

        match door.connection {
            Connection::Horizontal(dir) => {
                if target.floor != frame.floor {
                    continue;
                }
                let a = cell_center(g, from.position);
                let b = cell_center(g, target.position);
                let mid = gpui::point((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                // A short bar perpendicular to the connection, at the shared edge.
                let half = f32::from(g.cell) * 0.22;
                let (dx, dy) = dir_offset_px(dir, half);
                let p1 = gpui::point(mid.x - px(dx), mid.y - px(dy));
                let p2 = gpui::point(mid.x + px(dx), mid.y + px(dy));
                let is_sel = frame.selected_door == Some(door.id);
                if door.open && !is_sel {
                    continue; // open door: no bar (the gap is the signal)
                }
                let (pb, n) = if is_sel {
                    (&mut sel, &mut sel_n)
                } else if door.trapped {
                    (&mut trapped, &mut trapped_n)
                } else if door.lock.is_some() {
                    (&mut locked, &mut locked_n)
                } else if door.secret {
                    (&mut secret, &mut secret_n)
                } else {
                    (&mut normal, &mut normal_n)
                };
                pb.move_to(p1);
                pb.line_to(p2);
                *n += 1;
                if let Some(Lock { dc }) = door.lock {
                    // Centered above the bar, in the light text color: the
                    // amber/red bar behind it would swallow a warn-colored label.
                    let label = format!("DC{dc}");
                    let run = gpui::TextRun {
                        len: label.len(),
                        font: gpui::font("monospace"),
                        color: Palette::text(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped =
                        window
                            .text_system()
                            .shape_line(label.into(), px(10.0), &[run], None);
                    let top = f32::from(mid.y) - half - 14.0;
                    let left = f32::from(mid.x) - f32::from(shaped.width()) / 2.0;
                    let _ = shaped.paint(
                        gpui::point(px(left), px(top)),
                        px(12.0),
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            }
            Connection::Vertical { kind } => {
                // Stair marker: a small triangle in the room's top-right corner.
                let rect = cell_rect(g, from.position);
                let x = rect.origin.x + rect.size.width - px(16.0);
                let y = rect.origin.y + px(6.0);
                let _ = kind; // Stairs/Ladder/Shaft draw the same marker
                let up = target.floor < from.floor; // floor 0 is the topmost
                let mut tri = PathBuilder::fill();
                tri.add_polygon(
                    &[
                        gpui::point(x, y + px(if up { 0.0 } else { 10.0 })),
                        gpui::point(x - px(6.0), y + px(if up { 10.0 } else { 0.0 })),
                        gpui::point(x + px(6.0), y + px(if up { 10.0 } else { 0.0 })),
                    ],
                    true,
                );
                if let Ok(path) = tri.build() {
                    window.paint_path(path, Palette::accent());
                }
            }
        }
    }

    for (pb, n, color) in [
        (normal, normal_n, Palette::room_border()),
        (locked, locked_n, Palette::warn()),
        (trapped, trapped_n, Palette::danger()),
        (secret, secret_n, Palette::muted()),
        (sel, sel_n, Palette::select()),
    ] {
        if n > 0
            && let Ok(path) = pb.build()
        {
            window.paint_path(path, color);
        }
    }

    // Path overlay through room centers.
    if let Some(path) = &frame.path
        && path.len() > 1
    {
        let mut pb = PathBuilder::stroke(px(3.0)).dash_array(&[px(6.0), px(4.0)]);
        // Only segments with both rooms on this floor: a cross-floor link
        // would otherwise draw a straight line through empty cells.
        for w in path.windows(2) {
            let (Some(a), Some(b)) = (frame.d.room(w[0]), frame.d.room(w[1])) else {
                continue;
            };
            if a.floor != frame.floor || b.floor != frame.floor {
                continue;
            }
            pb.move_to(cell_center(g, a.position));
            pb.line_to(cell_center(g, b.position));
        }
        if let Ok(p) = pb.build() {
            window.paint_path(
                p,
                if frame.path_safe {
                    Palette::party()
                } else {
                    Palette::danger()
                },
            );
        }
    }

    // Party marker: filled circle on the party's room.
    if let Some(pid) = frame.d.party_position
        && let Some(r) = frame.d.room(pid)
        && r.floor == frame.floor
    {
        let c = cell_center(g, r.position);
        let rad = f32::from(g.cell) * 0.10;
        let mut circ = PathBuilder::fill();
        circ.move_to(gpui::point(c.x - px(rad), c.y));
        circ.relative_arc_to(
            gpui::point(px(rad), px(rad)),
            px(0.0),
            false,
            true,
            gpui::point(px(rad * 2.0), px(0.0)),
        );
        circ.relative_arc_to(
            gpui::point(px(rad), px(rad)),
            px(0.0),
            false,
            true,
            gpui::point(px(-rad * 2.0), px(0.0)),
        );
        if let Ok(p) = circ.build() {
            window.paint_path(p, Palette::party());
        }
    }

    // Cursor cell outline (even on empty cells — that's where doors get armed).
    let cur = cell_rect(g, frame.cursor);
    window.paint_quad(gpui::PaintQuad {
        bounds: cur,
        corner_radii: Corners::all(px(6.0)),
        background: gpui::transparent_black().into(),
        border_widths: Edges::all(px(if frame.pending_door { 2.5 } else { 1.5 })),
        border_color: if frame.pending_door {
            Palette::warn()
        } else {
            Palette::select()
        },
        border_style: gpui::BorderStyle::Dashed,
    });
}

fn dir_offset_px(dir: Direction, half: f32) -> (f32, f32) {
    // Perpendicular to the connection direction.
    let (x, y) = match dir {
        Direction::E | Direction::W => (0.0, 1.0),
        Direction::N | Direction::S => (1.0, 0.0),
        Direction::NE | Direction::SW => (1.0, 1.0),
        Direction::NW | Direction::SE => (-1.0, 1.0),
    };
    let inv = 1.0 / f32::sqrt(2.0);
    (x * half * inv, y * half * inv)
}

fn paint_label(
    window: &mut Window,
    cx: &mut gpui::App,
    x: Pixels,
    y: Pixels,
    text: &str,
    size: f32,
    color: Hsla,
) {
    let run = gpui::TextRun {
        len: text.len(),
        font: gpui::font("monospace"),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window
        .text_system()
        .shape_line(text.to_string().into(), px(size), &[run], None);
    let _ = shaped.paint(
        gpui::point(x, y),
        px(size * 1.2),
        gpui::TextAlign::Left,
        None,
        window,
        cx,
    );
}
