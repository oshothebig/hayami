//! Conversion from GPUI key events to `hayami_core::keymap::KeyPress`.

use gpui::Keystroke;
use hayami_core::keymap::KeyPress;

pub fn to_press(keystroke: &Keystroke) -> KeyPress {
    let m = &keystroke.modifiers;
    let mut press = KeyPress {
        key: keystroke.key.clone(),
        cmd: m.platform,
        ctrl: m.control,
        alt: m.alt,
        shift: m.shift,
    };
    // Normalize "G" to "shift-g" in case the platform reports the shifted
    // character as the key.
    if press.key.len() == 1 && press.key.chars().all(|c| c.is_ascii_uppercase()) {
        press.key = press.key.to_ascii_lowercase();
        press.shift = true;
    }
    press
}
