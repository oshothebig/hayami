use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Decorations, Entity, IntoElement,
    ParentElement as _, QuitMode, Render, Result, Styled as _, TitlebarOptions, Window,
    WindowOptions, component::TitleBar, div, prelude::FluentBuilder as _,
};

const WINDOW_TITLE: &str = "hayami";

struct Hayami;

impl Render for Hayami {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().when(
            matches!(window.window_decorations(), Decorations::Client { .. }),
            |this| this.child(TitleBar::new().child(WINDOW_TITLE)),
        )
    }
}

fn open_hayami_window(cx: &mut App) -> Result<(AnyWindowHandle, Entity<Hayami>)> {
    gpui_kit::init(cx);
    gpui_kit::open_window(
        WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(WINDOW_TITLE.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        cx,
        |_, cx| cx.new(|_| Hayami),
    )
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(|cx| {
            open_hayami_window(cx).expect("failed to open the hayami window");
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, TestAppContext, base::Root, test::TestWindowExt as _};

    use super::open_hayami_window;

    #[gpui_kit::test]
    fn opens_the_hayami_view_in_a_kit_window(cx: &mut TestAppContext) {
        let (window, view) = cx.update(open_hayami_window).unwrap();
        let root = window.downcast::<Root>().expect("GPUI Kit root");
        let content = root.read_with(cx, |root, _| root.view().clone()).unwrap();
        assert_eq!(content.entity_id(), view.entity_id());

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("title-bar").is_none());
        })
        .unwrap();
    }
}
