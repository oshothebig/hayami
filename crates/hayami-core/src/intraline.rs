//! Intra-line (word level) diff of a removed/added line pair.

use std::ops::Range;

use gix::diff::blob::{Algorithm, Diff, InternedInput};

/// Lines longer than this are not word-diffed.
const MAX_LINE_BYTES: usize = 2_000;

/// Changed byte ranges on each side of a line pair.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordDiff {
    /// Ranges in the old line that were removed.
    pub old: Vec<Range<usize>>,
    /// Ranges in the new line that were added.
    pub new: Vec<Range<usize>>,
}

/// Splits a line into tokens: runs of word characters, runs of whitespace,
/// and single other characters.
fn tokenize(line: &str) -> Vec<Range<usize>> {
    #[derive(PartialEq)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Other
        }
    };
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut current: Option<Class> = None;
    for (i, c) in line.char_indices() {
        let k = class(c);
        let continues = matches!((&current, &k), (Some(a), b) if a == b && *b != Class::Other);
        if !continues {
            if i > start {
                tokens.push(start..i);
            }
            start = i;
        }
        current = Some(k);
    }
    if line.len() > start {
        tokens.push(start..line.len());
    }
    tokens
}

/// Computes which words changed between `old` and `new`.
///
/// Returns empty ranges when the lines share no non-whitespace token (the
/// whole line is different, so emphasizing everything adds nothing) or when
/// a line is too long.
pub fn word_diff(old: &str, new: &str) -> WordDiff {
    if old.len() > MAX_LINE_BYTES || new.len() > MAX_LINE_BYTES || old == new {
        return WordDiff::default();
    }
    let old_tokens = tokenize(old);
    let new_tokens = tokenize(new);
    let mut input = InternedInput::default();
    input.update_before(old_tokens.iter().map(|r| &old[r.clone()]));
    input.update_after(new_tokens.iter().map(|r| &new[r.clone()]));
    let diff = Diff::compute(Algorithm::Myers, &input);

    let common_word = old_tokens
        .iter()
        .enumerate()
        .any(|(i, r)| !diff.is_removed(i as u32) && !old[r.clone()].trim().is_empty());
    if !common_word {
        return WordDiff::default();
    }
    let collect = |tokens: &[Range<usize>], changed: &dyn Fn(u32) -> bool| {
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for (i, r) in tokens.iter().enumerate() {
            if !changed(i as u32) {
                continue;
            }
            match ranges.last_mut() {
                Some(last) if last.end == r.start => last.end = r.end,
                _ => ranges.push(r.clone()),
            }
        }
        ranges
    };
    WordDiff {
        old: collect(&old_tokens, &|i| diff.is_removed(i)),
        new: collect(&new_tokens, &|i| diff.is_added(i)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts<'a>(line: &'a str, ranges: &[Range<usize>]) -> Vec<&'a str> {
        ranges.iter().map(|r| &line[r.clone()]).collect()
    }

    #[test]
    fn tokenizes_words_spaces_and_symbols() {
        let line = "let x_1 = foo(a,  b);";
        let tokens: Vec<&str> = tokenize(line).into_iter().map(|r| &line[r]).collect();
        assert_eq!(
            tokens,
            [
                "let", " ", "x_1", " ", "=", " ", "foo", "(", "a", ",", "  ", "b", ")", ";"
            ]
        );
    }

    #[test]
    fn marks_only_changed_words() {
        let old = "let total = compute(a, b);";
        let new = "let total = calculate(a, c);";
        let d = word_diff(old, new);
        assert_eq!(texts(old, &d.old), ["compute", "b"]);
        assert_eq!(texts(new, &d.new), ["calculate", "c"]);
    }

    #[test]
    fn adjacent_changes_merge() {
        // "-" and "c" are separate tokens but adjacent, so one range.
        let d = word_diff("x = a+b", "x = a-c");
        assert_eq!(texts("x = a-c", &d.new), ["-c"]);
        assert_eq!(texts("x = a+b", &d.old), ["+b"]);
    }

    #[test]
    fn insertion_in_the_middle() {
        let old = "foo(a)";
        let new = "foo(a, b)";
        let d = word_diff(old, new);
        assert!(d.old.is_empty());
        assert_eq!(texts(new, &d.new), [", b"]);
    }

    #[test]
    fn unicode_words_use_byte_ranges() {
        let old = "名前 = \"太郎\"";
        let new = "名前 = \"花子\"";
        let d = word_diff(old, new);
        assert_eq!(texts(old, &d.old), ["太郎"]);
        assert_eq!(texts(new, &d.new), ["花子"]);
    }

    #[test]
    fn completely_different_lines_are_not_emphasized() {
        assert_eq!(word_diff("alpha beta", "gamma delta"), WordDiff::default());
        assert_eq!(word_diff("same", "same"), WordDiff::default());
    }
}
