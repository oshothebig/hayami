//! Colors used by the app (a single dark theme for the MVP).

use gpui::{Rgba, rgb};
use hayami_core::highlight::HighlightKind;

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
pub fn added_emphasis_bg() -> Rgba {
    rgb(0x2f6a3e)
}
pub fn removed_emphasis_bg() -> Rgba {
    rgb(0x7a2f38)
}
pub fn filler_bg() -> Rgba {
    rgb(0x181818)
}

/// Text color of a syntax category.
pub fn syntax(kind: HighlightKind) -> Rgba {
    use HighlightKind as K;
    rgb(match kind {
        K::Attribute => 0xc8c8c8,
        K::Comment => 0x6a9955,
        K::Constant | K::VariableBuiltin => 0x569cd6,
        K::Function => 0xdcdcaa,
        K::Keyword => 0xc586c0,
        K::Number => 0xb5cea8,
        K::Operator | K::Punctuation => 0xd4d4d4,
        K::Property => 0x9cdcfe,
        K::String => 0xce9178,
        K::Escape => 0xd7ba7d,
        K::Tag => 0x569cd6,
        K::Type => 0x4ec9b0,
        K::Title => 0x569cd6,
        K::Link => 0x3794ff,
    })
}
