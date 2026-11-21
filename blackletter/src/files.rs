//! Native file/folder pickers via rfd (xdg-portal backend only — no GTK).
//!
//! The dialog runs on the background executor so the UI never blocks while
//! the portal is open (chonk pattern); the result is delivered back through
//! a plain callback that runs on the app context.

use gpui::App;
use rfd::AsyncFileDialog;
use std::path::PathBuf;

/// Folder picker → `on_picked(None)` when the user cancels.
pub fn pick_folder(on_picked: impl FnOnce(Option<PathBuf>, &mut App) + 'static, cx: &mut App) {
    let bg = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let picked = bg
            .spawn(async {
                AsyncFileDialog::new()
                    .pick_folder()
                    .await
                    .map(|h| h.path().to_path_buf())
            })
            .await;
        cx.update(|cx| on_picked(picked, cx));
    })
    .detach();
}

/// JSON file picker, starting in `dir` when given.
#[allow(dead_code)] // consumed by document open (Phase 2)
pub fn pick_json(
    dir: Option<PathBuf>,
    on_picked: impl FnOnce(Option<PathBuf>, &mut App) + 'static,
    cx: &mut App,
) {
    let bg = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let picked = bg
            .spawn(async move {
                let mut dialog = AsyncFileDialog::new().add_filter("JSON", &["json"]);
                if let Some(dir) = dir {
                    dialog = dialog.set_directory(dir);
                }
                dialog.pick_file().await.map(|h| h.path().to_path_buf())
            })
            .await;
        cx.update(|cx| on_picked(picked, cx));
    })
    .detach();
}
