//! Command palette: fuzzy filtering of commands and a selection.

use crate::keymap::{Command, PALETTE_COMMANDS, key_label};
use crate::text_input::TextInput;

/// One entry shown in the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    /// Command to run.
    pub command: Command,
    /// Title.
    pub title: &'static str,
    /// Key binding label, if bound.
    pub keys: Option<String>,
}

/// Palette state.
#[derive(Debug, Clone)]
pub struct Palette {
    /// Query field.
    pub query: TextInput,
    items: Vec<PaletteItem>,
    selected: usize,
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}

impl Palette {
    /// Palette with an empty query listing all commands.
    pub fn new() -> Self {
        let mut palette = Self {
            query: TextInput::new(),
            items: Vec::new(),
            selected: 0,
        };
        palette.refilter();
        palette
    }

    /// Recomputes the matches after the query changed.
    pub fn refilter(&mut self) {
        let query = self.query.text().to_lowercase();
        let mut scored: Vec<(usize, usize, PaletteItem)> = PALETTE_COMMANDS
            .iter()
            .enumerate()
            .filter_map(|(order, &command)| {
                let title = command.title();
                fuzzy_score(&query, &title.to_lowercase()).map(|score| {
                    (
                        score,
                        order,
                        PaletteItem {
                            command,
                            title,
                            keys: key_label(command),
                        },
                    )
                })
            })
            .collect();
        scored.sort_by_key(|(score, order, _)| (*score, *order));
        self.items = scored.into_iter().map(|(_, _, item)| item).collect();
        self.selected = 0;
    }

    /// Matching items, best first.
    pub fn items(&self) -> &[PaletteItem] {
        &self.items
    }

    /// Selected index.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Moves the selection, wrapping around.
    pub fn move_by(&mut self, delta: isize) {
        let len = self.items.len() as isize;
        if len > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        }
    }

    /// Command of the selected item.
    pub fn selected_command(&self) -> Option<Command> {
        self.items.get(self.selected).map(|i| i.command)
    }
}

/// Subsequence match; lower scores are better (span of the match, with a
/// bonus for matches at word starts). `None` if `query` is not a
/// subsequence of `text`.
fn fuzzy_score(query: &str, text: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let text: Vec<char> = text.chars().collect();
    let mut first = None;
    let mut last = 0;
    let mut pos = 0;
    let mut word_starts = 0;
    for q in query.chars().filter(|c| !c.is_whitespace()) {
        let found = (pos..text.len()).find(|&i| text[i] == q)?;
        if found == 0 || text[found - 1] == ' ' {
            word_starts += 1;
        }
        first.get_or_insert(found);
        last = found;
        pos = found + 1;
    }
    let span = last + 1 - first.unwrap_or(0);
    Some((span * 4 + first.unwrap_or(0)).saturating_sub(word_starts * 3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_all_commands_initially() {
        let p = Palette::new();
        assert_eq!(p.items().len(), PALETTE_COMMANDS.len());
        assert_eq!(p.selected_command(), Some(PALETTE_COMMANDS[0]));
    }

    #[test]
    fn filters_by_subsequence() {
        let mut p = Palette::new();
        p.query.insert("nxhk");
        p.refilter();
        assert_eq!(p.selected_command(), Some(Command::NextHunk));
        p.query.clear();
        p.query.insert("zzzz");
        p.refilter();
        assert!(p.items().is_empty());
        assert_eq!(p.selected_command(), None);
    }

    #[test]
    fn prefers_word_start_matches() {
        let mut p = Palette::new();
        p.query.insert("next file");
        p.refilter();
        assert_eq!(p.selected_command(), Some(Command::NextFile));
        p.query.clear();
        p.query.insert("reload");
        p.refilter();
        assert_eq!(p.selected_command(), Some(Command::Reload));
    }

    #[test]
    fn selection_wraps() {
        let mut p = Palette::new();
        p.move_by(-1);
        assert_eq!(p.selected(), p.items().len() - 1);
        p.move_by(1);
        assert_eq!(p.selected(), 0);
    }

    #[test]
    fn items_show_key_labels() {
        let p = Palette::new();
        let item = p
            .items()
            .iter()
            .find(|i| i.command == Command::CommandPalette)
            .unwrap();
        assert_eq!(item.keys.as_deref(), Some("⇧⌘P"));
    }
}
