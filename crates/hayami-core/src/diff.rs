//! Diff model: files, hunks and lines, plus the line diff of two texts.
//!
//! Line numbers in this module are 0-based indices into [`SourceText`];
//! the UI adds 1 when displaying them.

use std::sync::Arc;

use gix::diff::blob::{Algorithm, Diff, InternedInput};

/// Number of context lines around changes, like `git diff`.
pub const CONTEXT_LINES: u32 = 3;

/// Bytes inspected for NUL when detecting binary content (same as git).
const BINARY_PROBE: usize = 8000;

/// A file's full text split into lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceText {
    text: String,
    /// Byte offset of each line start; one entry per line.
    line_starts: Vec<usize>,
}

impl SourceText {
    /// Splits `text` into lines (`\n` terminated; a trailing line without a
    /// newline counts as a line).
    pub fn new(text: String) -> Self {
        let mut line_starts = Vec::new();
        if !text.is_empty() {
            line_starts.push(0);
            line_starts.extend(
                text.match_indices('\n')
                    .map(|(i, _)| i + 1)
                    .filter(|&i| i < text.len()),
            );
        }
        Self { text, line_starts }
    }

    /// Whole text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Number of lines.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Byte range of line `index` excluding its line terminator.
    pub fn line_range(&self, index: usize) -> std::ops::Range<usize> {
        let start = self.line_starts[index];
        let mut end = self
            .line_starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.text.len());
        let bytes = self.text.as_bytes();
        if end > start && bytes[end - 1] == b'\n' {
            end -= 1;
        }
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        start..end
    }

    /// Line `index` without its line terminator.
    pub fn line(&self, index: usize) -> &str {
        &self.text[self.line_range(index)]
    }
}

/// Kind of a diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineKind {
    /// Unchanged line shown for context.
    Context,
    /// Line only in the new version.
    Added,
    /// Line only in the old version.
    Removed,
}

/// One line of a hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffLine {
    /// Kind.
    pub kind: LineKind,
    /// Line index in the old text (context and removed lines).
    pub old: Option<u32>,
    /// Line index in the new text (context and added lines).
    pub new: Option<u32>,
}

/// A hunk: changed lines with surrounding context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// First old line index.
    pub old_start: u32,
    /// Number of old lines.
    pub old_len: u32,
    /// First new line index.
    pub new_start: u32,
    /// Number of new lines.
    pub new_len: u32,
    /// Lines in display order.
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    /// `@@ -a,b +c,d @@` header with 1-based line numbers like `git diff`.
    pub fn header(&self) -> String {
        let start = |s: u32, len: u32| if len == 0 { s } else { s + 1 };
        format!(
            "@@ -{},{} +{},{} @@",
            start(self.old_start, self.old_len),
            self.old_len,
            start(self.new_start, self.new_len),
            self.new_len
        )
    }
}

/// How a file changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// New file.
    Added,
    /// Removed file.
    Deleted,
    /// Changed in place.
    Modified,
    /// Moved from `from` (possibly with changes).
    Renamed {
        /// Previous path.
        from: String,
    },
}

impl FileStatus {
    /// One-letter marker like `git status --short`.
    pub fn marker(&self) -> char {
        match self {
            FileStatus::Added => 'A',
            FileStatus::Deleted => 'D',
            FileStatus::Modified => 'M',
            FileStatus::Renamed { .. } => 'R',
        }
    }
}

/// Content of a file diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContent {
    /// Text diff with both full versions (needed for highlighting).
    Text {
        /// Old version (empty for added files).
        old: Arc<SourceText>,
        /// New version (empty for deleted files).
        new: Arc<SourceText>,
        /// Hunks; empty when only the path or mode changed.
        hunks: Vec<Hunk>,
    },
    /// Binary file; only a placeholder is shown.
    Binary,
}

/// Diff of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// Path in the new version (old path for deleted files).
    pub path: String,
    /// Change kind.
    pub status: FileStatus,
    /// Lines added.
    pub additions: usize,
    /// Lines removed.
    pub deletions: usize,
    /// Content.
    pub content: FileContent,
}

impl FileDiff {
    /// Builds a file diff from raw blob contents (`None` for a missing side).
    pub fn from_blobs(
        path: String,
        status: FileStatus,
        old: Option<&[u8]>,
        new: Option<&[u8]>,
    ) -> Self {
        let old = old.unwrap_or_default();
        let new = new.unwrap_or_default();
        if is_binary(old) || is_binary(new) {
            return Self {
                path,
                status,
                additions: 0,
                deletions: 0,
                content: FileContent::Binary,
            };
        }
        let old = SourceText::new(String::from_utf8_lossy(old).into_owned());
        let new = SourceText::new(String::from_utf8_lossy(new).into_owned());
        let hunks = diff_lines(old.as_str(), new.as_str(), CONTEXT_LINES);
        let count = |kind| {
            hunks
                .iter()
                .flat_map(|h| &h.lines)
                .filter(|l| l.kind == kind)
                .count()
        };
        Self {
            path,
            status,
            additions: count(LineKind::Added),
            deletions: count(LineKind::Removed),
            content: FileContent::Text {
                old: Arc::new(old),
                new: Arc::new(new),
                hunks,
            },
        }
    }

    /// Hunks of a text diff; empty for binary files.
    pub fn hunks(&self) -> &[Hunk] {
        match &self.content {
            FileContent::Text { hunks, .. } => hunks,
            FileContent::Binary => &[],
        }
    }

    /// Text of a diff line.
    pub fn line_text(&self, line: &DiffLine) -> &str {
        match &self.content {
            FileContent::Text { old, new, .. } => match (line.new, line.old) {
                (Some(n), _) => new.line(n as usize),
                (None, Some(o)) => old.line(o as usize),
                (None, None) => "",
            },
            FileContent::Binary => "",
        }
    }
}

/// Whether `data` looks binary (NUL byte in the first 8000 bytes, like git).
pub fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(BINARY_PROBE)].contains(&0)
}

/// Line diff of two texts grouped into hunks with `context` lines.
pub fn diff_lines(old: &str, new: &str, context: u32) -> Vec<Hunk> {
    let input = InternedInput::new(old, new);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let changes: Vec<_> = diff.hunks().collect();
    let old_len = input.before.len() as u32;
    let new_len = input.after.len() as u32;

    let mut hunks = Vec::new();
    let mut i = 0;
    while i < changes.len() {
        // Merge changes whose context would overlap or touch.
        let mut j = i;
        while j + 1 < changes.len()
            && changes[j + 1].before.start - changes[j].before.end <= 2 * context
        {
            j += 1;
        }
        let first = &changes[i];
        let last = &changes[j];
        let lead = first.before.start.min(context);
        let old_start = first.before.start - lead;
        let new_start = first.after.start - lead;
        let old_end = (last.before.end + context).min(old_len);
        let trail = old_end - last.before.end;
        let new_end = (last.after.end + trail).min(new_len);

        let mut lines = Vec::new();
        let (mut o, mut n) = (old_start, new_start);
        for change in &changes[i..=j] {
            while o < change.before.start {
                lines.push(DiffLine {
                    kind: LineKind::Context,
                    old: Some(o),
                    new: Some(n),
                });
                o += 1;
                n += 1;
            }
            for r in change.before.clone() {
                lines.push(DiffLine {
                    kind: LineKind::Removed,
                    old: Some(r),
                    new: None,
                });
            }
            for a in change.after.clone() {
                lines.push(DiffLine {
                    kind: LineKind::Added,
                    old: None,
                    new: Some(a),
                });
            }
            o = change.before.end;
            n = change.after.end;
        }
        while o < old_end {
            lines.push(DiffLine {
                kind: LineKind::Context,
                old: Some(o),
                new: Some(n),
            });
            o += 1;
            n += 1;
        }
        hunks.push(Hunk {
            old_start,
            old_len: old_end - old_start,
            new_start,
            new_len: new_end - new_start,
            lines,
        });
        i = j + 1;
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn source_text_lines() {
        let t = SourceText::new("a\r\nb\n\nc".into());
        assert_eq!(t.line_count(), 4);
        assert_eq!(t.line(0), "a");
        assert_eq!(t.line(1), "b");
        assert_eq!(t.line(2), "");
        assert_eq!(t.line(3), "c");
        assert_eq!(SourceText::new(String::new()).line_count(), 0);
        assert_eq!(SourceText::new("x\n".into()).line_count(), 1);
    }

    #[test]
    fn single_change_has_context() {
        let old = numbered(10);
        let new = old.replace("line 5\n", "line five\n");
        let hunks = diff_lines(&old, &new, 3);
        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        assert_eq!(
            (h.old_start, h.old_len, h.new_start, h.new_len),
            (1, 7, 1, 7)
        );
        assert_eq!(h.header(), "@@ -2,7 +2,7 @@");
        let kinds: Vec<_> = h.lines.iter().map(|l| l.kind).collect();
        use LineKind::*;
        assert_eq!(
            kinds,
            [
                Context, Context, Context, Removed, Added, Context, Context, Context
            ]
        );
        assert_eq!(h.lines[3].old, Some(4));
        assert_eq!(h.lines[4].new, Some(4));
    }

    #[test]
    fn distant_changes_make_separate_hunks() {
        let old = numbered(30);
        let new = old
            .replace("line 2\n", "line two\n")
            .replace("line 28\n", "line twenty-eight\n");
        let hunks = diff_lines(&old, &new, 3);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].old_start, 0);
        assert_eq!(hunks[1].old_start + hunks[1].old_len, 30);
    }

    #[test]
    fn close_changes_merge() {
        let old = numbered(30);
        let new = old.replace("line 10\n", "x\n").replace("line 16\n", "y\n");
        assert_eq!(diff_lines(&old, &new, 3).len(), 1);
    }

    #[test]
    fn added_and_deleted_files() {
        let added = FileDiff::from_blobs("a".into(), FileStatus::Added, None, Some(b"x\ny\n"));
        assert_eq!((added.additions, added.deletions), (2, 0));
        assert_eq!(added.hunks()[0].header(), "@@ -0,0 +1,2 @@");
        let deleted = FileDiff::from_blobs("a".into(), FileStatus::Deleted, Some(b"x\n"), None);
        assert_eq!((deleted.additions, deleted.deletions), (0, 1));
        assert_eq!(deleted.line_text(&deleted.hunks()[0].lines[0]), "x");
    }

    #[test]
    fn binary_detection() {
        let f = FileDiff::from_blobs(
            "img.png".into(),
            FileStatus::Modified,
            Some(b"\x89PNG\0\0"),
            Some(b"\x89PNG\0\x01"),
        );
        assert_eq!(f.content, FileContent::Binary);
        assert!(f.hunks().is_empty());
    }

    #[test]
    fn missing_trailing_newline_is_a_change() {
        let hunks = diff_lines("a\nb", "a\nb\n", 3);
        assert_eq!(hunks.len(), 1);
    }
}
