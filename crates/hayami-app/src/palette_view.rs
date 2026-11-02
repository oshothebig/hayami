//! Command palette overlay.

use gpui::{Div, SharedString, div, prelude::*, px};
use hayami_core::palette::Palette;

use crate::theme;

const MAX_ITEMS: usize = 14;

pub fn render(palette: &Palette) -> Div {
    let selected = palette.selected();
    // Keep the selection visible within the window of shown items.
    let first = selected.saturating_sub(MAX_ITEMS - 1);
    let items = palette
        .items()
        .iter()
        .enumerate()
        .skip(first)
        .take(MAX_ITEMS)
        .map(|(ix, item)| {
            div()
                .h(px(24.0))
                .px_3()
                .flex()
                .items_center()
                .justify_between()
                .when(ix == selected, |d| d.bg(theme::selection()))
                .child(item.title)
                .child(
                    div()
                        .text_color(theme::muted())
                        .child(item.keys.clone().unwrap_or_default()),
                )
        });
    let panel = div()
        .w(px(560.0))
        .bg(theme::panel_bg())
        .border_1()
        .border_color(theme::accent())
        .shadow_lg()
        .flex()
        .flex_col()
        .child(
            div()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(theme::border())
                .child(SharedString::from(format!("> {}▏", palette.query.text()))),
        )
        .children(items)
        .when(palette.items().is_empty(), |d| {
            d.child(
                div()
                    .px_3()
                    .py_1()
                    .text_color(theme::muted())
                    .child("No matching commands"),
            )
        });
    div()
        .absolute()
        .top(px(48.0))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(panel)
}
