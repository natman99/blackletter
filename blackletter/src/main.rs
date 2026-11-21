//! Blackletter: D&D 5e character sheets + dungeon tracking, one GPUI app.

mod autosave;
mod campaign;
mod dungeon_view;
mod files;
mod root;
mod session;

use gpui::{App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px};
use root::RootView;

fn main() {
    let session_path = session::session_path();
    let session = session_path
        .as_ref()
        .map(|p| session::load(p))
        .unwrap_or_default();
    let (w, h) = session.window_size.unwrap_or((1100., 760.));

    let app = gpui_platform::application();
    app.run(move |cx: &mut App| {
        cx.bind_keys(root::keybindings());
        cx.bind_keys(dungeon_view::keybindings());
        autosave::Autosave::init(cx);
        let window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    gpui::size(px(w), px(h)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Blackletter".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| RootView::new(cx, session, session_path)),
        ) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("blackletter: {e}");
                std::process::exit(1);
            }
        };
        // The root takes the keyboard the moment it exists, so the global
        // ctrl-1/ctrl-2 chords work before any click. A restored dungeon
        // mode also builds (and focuses) its view here.
        let _ = window.update(cx, |view, window, cx| {
            if view.mode() == root::Mode::Dungeon {
                view.ensure_dungeon(window, cx);
            } else {
                window.focus(view.focus_handle(), cx);
            }
        });
    });
}
