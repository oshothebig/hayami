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
