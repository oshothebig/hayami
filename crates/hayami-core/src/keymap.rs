//! Key bindings and the key sequence state machine.
//!
//! All default bindings are defined in [`BINDINGS`]. The app converts
//! platform key events into [`KeyPress`] and feeds them to
//! [`KeyInterpreter::handle`], which resolves multi-key sequences such as
//! `g g`.

use std::fmt;

/// A single key press with modifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyPress {
    /// Key name: a lowercase character (`"j"`, `"["`) or a named key
    /// (`"enter"`, `"escape"`, `"up"`, `"backspace"`, `"tab"`, `"space"`).
    pub key: String,
    /// Command key.
    pub cmd: bool,
    /// Control key.
    pub ctrl: bool,
    /// Option key.
    pub alt: bool,
    /// Shift key.
    pub shift: bool,
}

impl KeyPress {
    /// A key without modifiers.
    pub fn plain(key: &str) -> Self {
        Self {
            key: key.to_owned(),
            cmd: false,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    /// Parses `"cmd-shift-p"`, `"ctrl-d"`, `"shift-g"`, `"j"`, `"-"`.
    pub fn parse(spec: &str) -> Option<Self> {
        let mut press = KeyPress::plain("");
        let mut rest = spec;
        loop {
            let (prefix, flag) = if let Some(r) = rest.strip_prefix("cmd-") {
                (r, &mut press.cmd)
            } else if let Some(r) = rest.strip_prefix("ctrl-") {
                (r, &mut press.ctrl)
            } else if let Some(r) = rest.strip_prefix("alt-") {
                (r, &mut press.alt)
            } else if let Some(r) = rest.strip_prefix("shift-") {
                (r, &mut press.shift)
            } else {
                break;
            };
            *flag = true;
            rest = prefix;
        }
        if rest.is_empty() {
            return None;
        }
        press.key = rest.to_owned();
        Some(press)
    }

    /// Whether this press would type a character into a text field: no
    /// command/control modifier and a single-character key.
    pub fn is_typing(&self) -> bool {
        !self.cmd && !self.ctrl && self.key.chars().count() == 1
    }
}

impl fmt::Display for KeyPress {
    /// macOS style label, e.g. `⌘⇧P`, `⌃D`, `G`, `j`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("⌃")?;
        }
        if self.alt {
            f.write_str("⌥")?;
        }
        let letter = self.key.len() == 1 && self.key.chars().all(|c| c.is_ascii_alphabetic());
        let shift_as_case = self.shift && letter && !self.cmd && !self.ctrl && !self.alt;
        if self.shift && !shift_as_case {
            f.write_str("⇧")?;
        }
        if self.cmd {
            f.write_str("⌘")?;
        }
        let key = match self.key.as_str() {
            "enter" => "↩".to_owned(),
            "escape" => "esc".to_owned(),
            "up" => "↑".to_owned(),
            "down" => "↓".to_owned(),
            k if shift_as_case || ((self.cmd || self.ctrl || self.alt) && letter) => {
                k.to_uppercase()
            }
            k => k.to_owned(),
        };
        f.write_str(&key)
    }
}

/// Where the keyboard focus is; decides which bindings are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyContext {
    /// PR list screen.
    PrList,
    /// File tree of the review screen.
    FileTree,
    /// Diff view of the review screen.
    Diff,
    /// A text field (palette query, token entry). Vim-style keys are off.
    TextInput,
}

/// Contexts in which a binding is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Everywhere.
    Global,
    /// PR list, file tree and diff (everything that is not a text field).
    Navigation,
    /// PR list only.
    PrList,
    /// Review screen (file tree and diff).
    Review,
    /// Diff only.
    Diff,
    /// File tree only.
    Tree,
    /// Text fields only.
    TextInput,
}

impl Scope {
    fn contains(self, cx: KeyContext) -> bool {
        use KeyContext as K;
        match self {
            Scope::Global => true,
            Scope::Navigation => cx != K::TextInput,
            Scope::PrList => cx == K::PrList,
            Scope::Review => matches!(cx, K::FileTree | K::Diff),
            Scope::Diff => cx == K::Diff,
            Scope::Tree => cx == K::FileTree,
            Scope::TextInput => cx == K::TextInput,
        }
    }
}

/// Everything the user can trigger by key or from the command palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// Move the cursor one row down.
    MoveDown,
    /// Move the cursor one row up.
    MoveUp,
    /// Move half a page down.
    HalfPageDown,
    /// Move half a page up.
    HalfPageUp,
    /// Go to the first row.
    Top,
    /// Go to the last row.
    Bottom,
    /// Open the selected item.
    Open,
    /// Jump to the next hunk.
    NextHunk,
    /// Jump to the previous hunk.
    PrevHunk,
    /// Jump to the next file.
    NextFile,
    /// Jump to the previous file.
    PrevFile,
    /// Collapse or expand the current file.
    ToggleCollapse,
    /// Collapse or expand all files.
    ToggleCollapseAll,
    /// Move the keyboard focus to the file tree.
    FocusFileTree,
    /// Move the keyboard focus to the diff.
    FocusDiff,
    /// Open the command palette.
    CommandPalette,
    /// Go back to the PR list.
    Back,
    /// Reload from GitHub.
    Reload,
    /// Quit the app.
    Quit,
    /// Confirm a text field.
    Confirm,
    /// Cancel a text field or pending key sequence.
    Cancel,
    /// Paste from the clipboard into a text field.
    Paste,
}

impl Command {
    /// Human readable name shown in the command palette.
    pub fn title(self) -> &'static str {
        match self {
            Command::MoveDown => "Move Down",
            Command::MoveUp => "Move Up",
            Command::HalfPageDown => "Half Page Down",
            Command::HalfPageUp => "Half Page Up",
            Command::Top => "Go to Top",
            Command::Bottom => "Go to Bottom",
            Command::Open => "Open Selected",
            Command::NextHunk => "Next Hunk",
            Command::PrevHunk => "Previous Hunk",
            Command::NextFile => "Next File",
            Command::PrevFile => "Previous File",
            Command::ToggleCollapse => "Toggle Collapse File",
            Command::ToggleCollapseAll => "Toggle Collapse All Files",
            Command::FocusFileTree => "Focus File Tree",
            Command::FocusDiff => "Focus Diff",
            Command::CommandPalette => "Command Palette",
            Command::Back => "Back to Pull Request List",
            Command::Reload => "Reload",
            Command::Quit => "Quit hayami",
            Command::Confirm => "Confirm",
            Command::Cancel => "Cancel",
            Command::Paste => "Paste",
        }
    }
}

/// Commands listed in the command palette (everything except text field
/// editing, which only makes sense while typing).
pub const PALETTE_COMMANDS: &[Command] = &[
    Command::NextHunk,
    Command::PrevHunk,
    Command::NextFile,
    Command::PrevFile,
    Command::ToggleCollapse,
    Command::ToggleCollapseAll,
    Command::FocusFileTree,
    Command::FocusDiff,
    Command::MoveDown,
    Command::MoveUp,
    Command::HalfPageDown,
    Command::HalfPageUp,
    Command::Top,
    Command::Bottom,
    Command::Open,
    Command::Back,
    Command::Reload,
    Command::CommandPalette,
    Command::Quit,
];

/// A default key binding.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    /// Space separated key presses, e.g. `"g g"` or `"cmd-shift-p"`.
    pub keys: &'static str,
    /// Command to run.
    pub command: Command,
    /// Where it is active.
    pub scope: Scope,
}

const fn bind(keys: &'static str, command: Command, scope: Scope) -> Binding {
    Binding {
        keys,
        command,
        scope,
    }
}

/// The single definition of all default key bindings.
pub const BINDINGS: &[Binding] = &[
    // Vim-style movement. Disabled in text fields.
    bind("j", Command::MoveDown, Scope::Navigation),
    bind("k", Command::MoveUp, Scope::Navigation),
    bind("down", Command::MoveDown, Scope::Global),
    bind("up", Command::MoveUp, Scope::Global),
    bind("ctrl-n", Command::MoveDown, Scope::TextInput),
    bind("ctrl-p", Command::MoveUp, Scope::TextInput),
    bind("ctrl-d", Command::HalfPageDown, Scope::Navigation),
    bind("ctrl-u", Command::HalfPageUp, Scope::Navigation),
    bind("g g", Command::Top, Scope::Navigation),
    bind("shift-g", Command::Bottom, Scope::Navigation),
    bind("enter", Command::Open, Scope::PrList),
    bind("enter", Command::FocusDiff, Scope::Tree),
    bind("escape", Command::FocusDiff, Scope::Tree),
    bind("n", Command::NextHunk, Scope::Diff),
    bind("p", Command::PrevHunk, Scope::Diff),
    bind("]", Command::NextFile, Scope::Review),
    bind("[", Command::PrevFile, Scope::Review),
    bind("o", Command::ToggleCollapse, Scope::Review),
    bind("shift-o", Command::ToggleCollapseAll, Scope::Review),
    // macOS style commands.
    bind("cmd-[", Command::Back, Scope::Review),
    bind("cmd-shift-e", Command::FocusFileTree, Scope::Review),
    bind("cmd-shift-p", Command::CommandPalette, Scope::Global),
    bind("cmd-r", Command::Reload, Scope::Global),
    bind("cmd-q", Command::Quit, Scope::Global),
    bind("enter", Command::Confirm, Scope::TextInput),
    bind("escape", Command::Cancel, Scope::TextInput),
    bind("cmd-v", Command::Paste, Scope::TextInput),
];

/// Result of feeding a key press to the interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// A binding matched.
    Command(Command),
    /// The press is a prefix of a longer binding; waiting for more keys.
    Pending,
    /// No binding; the app may treat it as text input.
    Unhandled,
}

/// Resolves key presses into commands, keeping multi-key sequences pending.
#[derive(Debug, Default, Clone)]
pub struct KeyInterpreter {
    pending: Vec<KeyPress>,
}

impl KeyInterpreter {
    /// Creates an interpreter with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keys typed so far of an unfinished sequence.
    pub fn pending(&self) -> &[KeyPress] {
        &self.pending
    }

    /// Drops any pending sequence.
    pub fn reset(&mut self) {
        self.pending.clear();
    }

    /// Feeds one key press.
    pub fn handle(&mut self, press: KeyPress, cx: KeyContext) -> KeyOutcome {
        if cx == KeyContext::TextInput {
            self.pending.clear();
        }
        if !self.pending.is_empty() && press.key == "escape" && !press.cmd && !press.ctrl {
            self.pending.clear();
            return KeyOutcome::Pending;
        }
        let mut sequence = std::mem::take(&mut self.pending);
        sequence.push(press.clone());
        match resolve(&sequence, cx) {
            Resolution::Exact(command) => KeyOutcome::Command(command),
            Resolution::Prefix => {
                self.pending = sequence;
                KeyOutcome::Pending
            }
            Resolution::None if sequence.len() > 1 => {
                // The pending prefix did not continue; retry this key alone.
                self.handle(press, cx)
            }
            Resolution::None => KeyOutcome::Unhandled,
        }
    }
}

enum Resolution {
    Exact(Command),
    Prefix,
    None,
}

fn resolve(sequence: &[KeyPress], cx: KeyContext) -> Resolution {
    let mut prefix = false;
    for binding in BINDINGS.iter().filter(|b| b.scope.contains(cx)) {
        let keys = parse_sequence(binding.keys);
        if keys.len() < sequence.len() || keys[..sequence.len()] != *sequence {
            continue;
        }
        if keys.len() == sequence.len() {
            return Resolution::Exact(binding.command);
        }
        prefix = true;
    }
    if prefix {
        Resolution::Prefix
    } else {
        Resolution::None
    }
}

fn parse_sequence(keys: &str) -> Vec<KeyPress> {
    keys.split(' ').filter_map(KeyPress::parse).collect()
}

/// Display label of the first binding of `command`, e.g. `"g g"` or `"⌘R"`.
pub fn key_label(command: Command) -> Option<String> {
    let binding = BINDINGS.iter().find(|b| b.command == command)?;
    Some(
        parse_sequence(binding.keys)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(spec: &str) -> KeyPress {
        KeyPress::parse(spec).unwrap()
    }

    #[test]
    fn parses_specs() {
        let p = key("cmd-shift-p");
        assert!(p.cmd && p.shift && !p.ctrl);
        assert_eq!(p.key, "p");
        assert_eq!(key("cmd-[").key, "[");
        assert_eq!(key("-").key, "-");
        assert!(KeyPress::parse("cmd-").is_none());
    }

    #[test]
    fn all_bindings_parse() {
        for b in BINDINGS {
            let keys = parse_sequence(b.keys);
            assert_eq!(keys.len(), b.keys.split(' ').count(), "{}", b.keys);
        }
    }

    #[test]
    fn single_keys_resolve() {
        let mut k = KeyInterpreter::new();
        assert_eq!(
            k.handle(key("j"), KeyContext::Diff),
            KeyOutcome::Command(Command::MoveDown)
        );
        assert_eq!(
            k.handle(key("shift-g"), KeyContext::PrList),
            KeyOutcome::Command(Command::Bottom)
        );
        assert_eq!(
            k.handle(key("cmd-r"), KeyContext::PrList),
            KeyOutcome::Command(Command::Reload)
        );
    }

    #[test]
    fn gg_waits_for_second_key() {
        let mut k = KeyInterpreter::new();
        assert_eq!(k.handle(key("g"), KeyContext::Diff), KeyOutcome::Pending);
        assert_eq!(k.pending(), &[key("g")]);
        assert_eq!(
            k.handle(key("g"), KeyContext::Diff),
            KeyOutcome::Command(Command::Top)
        );
        assert!(k.pending().is_empty());
    }

    #[test]
    fn broken_sequence_retries_last_key() {
        let mut k = KeyInterpreter::new();
        assert_eq!(k.handle(key("g"), KeyContext::Diff), KeyOutcome::Pending);
        assert_eq!(
            k.handle(key("j"), KeyContext::Diff),
            KeyOutcome::Command(Command::MoveDown)
        );
        assert_eq!(k.handle(key("g"), KeyContext::Diff), KeyOutcome::Pending);
        assert_eq!(k.handle(key("x"), KeyContext::Diff), KeyOutcome::Unhandled);
        assert!(k.pending().is_empty());
    }

    #[test]
    fn escape_cancels_pending() {
        let mut k = KeyInterpreter::new();
        k.handle(key("g"), KeyContext::Diff);
        assert_eq!(
            k.handle(key("escape"), KeyContext::Diff),
            KeyOutcome::Pending
        );
        assert!(k.pending().is_empty());
        assert_eq!(k.handle(key("g"), KeyContext::Diff), KeyOutcome::Pending);
    }

    #[test]
    fn vim_keys_are_disabled_in_text_input() {
        let mut k = KeyInterpreter::new();
        for spec in ["j", "k", "g", "shift-g", "ctrl-d"] {
            assert_eq!(
                k.handle(key(spec), KeyContext::TextInput),
                KeyOutcome::Unhandled,
                "{spec}"
            );
        }
        assert_eq!(
            k.handle(key("enter"), KeyContext::TextInput),
            KeyOutcome::Command(Command::Confirm)
        );
        assert_eq!(
            k.handle(key("down"), KeyContext::TextInput),
            KeyOutcome::Command(Command::MoveDown)
        );
        assert_eq!(
            k.handle(key("cmd-q"), KeyContext::TextInput),
            KeyOutcome::Command(Command::Quit)
        );
    }

    #[test]
    fn review_bindings() {
        let mut k = KeyInterpreter::new();
        let cases = [
            ("n", KeyContext::Diff, Command::NextHunk),
            ("p", KeyContext::Diff, Command::PrevHunk),
            ("]", KeyContext::Diff, Command::NextFile),
            ("[", KeyContext::FileTree, Command::PrevFile),
            ("o", KeyContext::Diff, Command::ToggleCollapse),
            ("cmd-shift-e", KeyContext::Diff, Command::FocusFileTree),
            ("cmd-[", KeyContext::Diff, Command::Back),
            ("cmd-shift-p", KeyContext::PrList, Command::CommandPalette),
            ("enter", KeyContext::FileTree, Command::FocusDiff),
            ("enter", KeyContext::PrList, Command::Open),
        ];
        for (spec, cx, command) in cases {
            assert_eq!(
                k.handle(key(spec), cx),
                KeyOutcome::Command(command),
                "{spec}"
            );
        }
        // Diff-only keys do nothing on the PR list.
        assert_eq!(
            k.handle(key("n"), KeyContext::PrList),
            KeyOutcome::Unhandled
        );
    }

    #[test]
    fn every_palette_command_has_a_title() {
        for command in PALETTE_COMMANDS {
            assert!(!command.title().is_empty());
        }
    }

    #[test]
    fn labels() {
        assert_eq!(key_label(Command::Top).unwrap(), "g g");
        assert_eq!(key_label(Command::Bottom).unwrap(), "G");
        assert_eq!(key_label(Command::Reload).unwrap(), "⌘R");
        assert_eq!(key_label(Command::HalfPageDown).unwrap(), "⌃D");
        assert!(key("j").is_typing());
        assert!(!key("cmd-j").is_typing());
        assert!(!key("enter").is_typing());
    }
}
