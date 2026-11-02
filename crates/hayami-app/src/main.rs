//! hayami: keyboard-driven pull request review app for macOS.

mod keychain;
mod keys;
mod review;
mod root;
mod theme;

use gpui::{
    App, Application, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions,
    prelude::*, px, size,
};

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from(hayami_core::APP_NAME)),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| root::RootView::new(window, cx)),
        )
        .expect("failed to open the main window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
