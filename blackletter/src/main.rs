//! Blackletter: D&D 5e character sheets + dungeon tracking, one GPUI app.

mod autosave;
mod campaign;
mod dungeon_view;
mod field;
mod files;
mod picker;
mod root;
mod session;
mod sheet_view;

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
        cx.bind_keys(sheet_view::keybindings());
        cx.bind_keys(field::field_keys(dungeon_view::FIELD_CTX));
        cx.bind_keys(picker::picker_keys());
        autosave::Autosave::init(cx);
        // The SRD corpus loads once at startup (vendored `srd/`, see Task
        // 4.1); a missing corpus only disables the pickers, never the app.
        let corpus = blackletter_types::database::corpus::Corpus::load_vendored()
            .ok()
            .map(std::sync::Arc::new);
        // The custom-content library (Task 4.3) is the second content
        // source: cached on disk, merged into every picker.
        let library = session::library_path()
            .and_then(|p| blackletter_types::library::Library::load(&p).ok())
            .unwrap_or_default();
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
            |_, cx| cx.new(|cx| RootView::new(cx, session, session_path, corpus, library)),
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
                view.ensure_sheet(window, cx);
            }
        });
    });
}
