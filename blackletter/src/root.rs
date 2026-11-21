//! The root view: mode tabs, the active mode's view, the status bar.
//! Scaffold stage: a placeholder center until Phase 1 wires the modes.

use gpui::{Context, Render, Window, div, prelude::*};

pub struct RootView;

impl RootView {
    pub fn new() -> Self {
        Self
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgb(0x1e_1e_2e))
            .text_2xl()
            .text_color(gpui::rgb(0xcd_d6_f4))
            .child("Blackletter — scaffold")
    }
}
