//! Minimal single-line text field state (palette query, token entry).

/// Single-line text with the cursor at the end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    text: String,
}

impl TextInput {
    /// Empty field.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Appends typed text, dropping control characters.
    pub fn insert(&mut self, typed: &str) {
        self.text.extend(typed.chars().filter(|c| !c.is_control()));
    }

    /// Deletes the last character.
    pub fn backspace(&mut self) {
        self.text.pop();
    }

    /// Clears the field.
    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// Takes the text out, leaving the field empty.
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.text)
    }

    /// Text masked for secrets: one bullet per character.
    pub fn masked(&self) -> String {
        "•".repeat(self.text.chars().count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits() {
        let mut t = TextInput::new();
        t.insert("ab\nc");
        assert_eq!(t.text(), "abc");
        t.backspace();
        assert_eq!(t.text(), "ab");
        assert_eq!(t.masked(), "••");
        assert_eq!(t.take(), "ab");
        assert_eq!(t.text(), "");
    }
}
