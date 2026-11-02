//! Colors used by the app (a single dark theme for the MVP).

use gpui::{Rgba, rgb};

pub fn bg() -> Rgba {
    rgb(0x1e1e1e)
}
pub fn panel_bg() -> Rgba {
    rgb(0x252526)
}
pub fn border() -> Rgba {
    rgb(0x3c3c3c)
}
pub fn text() -> Rgba {
    rgb(0xd4d4d4)
}
pub fn muted() -> Rgba {
    rgb(0x8b8b8b)
}
pub fn accent() -> Rgba {
    rgb(0x4fc1ff)
}
pub fn error() -> Rgba {
    rgb(0xf48771)
}
pub fn selection() -> Rgba {
    rgb(0x094771)
}
pub fn header_bg() -> Rgba {
    rgb(0x2d2d30)
}
pub fn hunk_bg() -> Rgba {
    rgb(0x1f2a3a)
}
pub fn added_bg() -> Rgba {
    rgb(0x1f3a26)
}
pub fn removed_bg() -> Rgba {
    rgb(0x43242a)
}
pub fn added_fg() -> Rgba {
    rgb(0x73c991)
}
pub fn removed_fg() -> Rgba {
    rgb(0xf14c4c)
}
