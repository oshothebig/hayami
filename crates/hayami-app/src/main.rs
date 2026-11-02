//! hayami: keyboard-driven pull request review app for macOS.

use gpui::{
    App, Application, Bounds, Context, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};

struct RootView;

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xd4d4d4))
            .child(hayami_core::APP_NAME)
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from(hayami_core::APP_NAME)),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| RootView),
        )
        .expect("failed to open the main window");
        cx.activate(true);
    });
}
