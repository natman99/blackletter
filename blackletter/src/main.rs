//! Blackletter: D&D 5e character sheets + dungeon tracking, one GPUI app.

mod root;

use gpui::{App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px};
use root::RootView;

fn main() {
    let app = gpui_platform::application();
    app.run(move |cx: &mut App| {
        if let Err(e) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    gpui::size(px(1100.), px(760.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Blackletter".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| RootView::new()),
        ) {
            eprintln!("blackletter: {e}");
            std::process::exit(1);
        }
    });
}
