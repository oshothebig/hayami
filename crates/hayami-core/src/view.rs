//! Display row model of the diff view with a cursor.
//!
//! The view never materializes all rows. It keeps prefix sums of row counts
//! per file and per hunk, so locating a row is two binary searches and
//! [`DiffView::rows`] only builds the rows of the requested (visible) range.

use std::ops::Range;
use std::sync::Arc;

use crate::diff::{FileContent, FileDiff, LineKind};

/// Address of a display row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowAddr {
    /// File header.
    FileHeader {
        /// File index.
        file: usize,
    },
    /// Placeholder for binary files and files without content changes.
    Notice {
        /// File index.
        file: usize,
    },
    /// Hunk header (`@@ ... @@`).
    HunkHeader {
        /// File index.
        file: usize,
        /// Hunk index.
        hunk: usize,
    },
    /// A line of a hunk.
    Line {
        /// File index.
        file: usize,
        /// Hunk index.
        hunk: usize,
        /// Line index within the hunk.
        line: usize,
    },
}

impl RowAddr {
    /// File the row belongs to.
    pub fn file(self) -> usize {
        match self {
            RowAddr::FileHeader { file }
            | RowAddr::Notice { file }
            | RowAddr::HunkHeader { file, .. }
            | RowAddr::Line { file, .. } => file,
        }
    }
}

/// One line as displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineView<'a> {
    /// Kind.
    pub kind: LineKind,
    /// 1-based old line number.
    pub old_no: Option<u32>,
    /// 1-based new line number.
    pub new_no: Option<u32>,
    /// Text without line terminator.
    pub text: &'a str,
}

/// Content of a display row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowContent<'a> {
    /// File header.
    FileHeader {
        /// The file.
        file: &'a FileDiff,
        /// Whether the file is collapsed.
        collapsed: bool,
    },
    /// Placeholder text.
    Notice(&'static str),
    /// Hunk header text.
    HunkHeader(String),
    /// A diff line.
    Line(LineView<'a>),
}

/// A display row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayRow<'a> {
    /// Row index.
    pub index: usize,
    /// Address.
    pub addr: RowAddr,
    /// Content.
    pub content: RowContent<'a>,
}

/// The diff view: files, layout and cursor.
#[derive(Debug, Clone)]
pub struct DiffView {
    files: Arc<Vec<FileDiff>>,
    collapsed: Vec<bool>,
    /// Start row of each file, plus the total row count at the end.
    file_starts: Vec<usize>,
    /// Per file: row offset (relative to the file header) of each hunk header.
    hunk_starts: Vec<Vec<usize>>,
    cursor: usize,
}

impl DiffView {
    /// Creates a view with all files expanded and the cursor at the top.
    pub fn new(files: Arc<Vec<FileDiff>>) -> Self {
        let collapsed = vec![false; files.len()];
        let mut view = Self {
            files,
            collapsed,
            file_starts: Vec::new(),
            hunk_starts: Vec::new(),
            cursor: 0,
        };
        view.relayout();
        view
    }

    /// Files.
    pub fn files(&self) -> &Arc<Vec<FileDiff>> {
        &self.files
    }

    fn relayout(&mut self) {
        self.hunk_starts = (0..self.files.len()).map(|f| self.layout_file(f)).collect();
        self.rebuild_file_starts();
    }

    /// Hunk start offsets of one file in the current layout.
    fn layout_file(&self, file: usize) -> Vec<usize> {
        let mut offset = 1;
        self.files[file]
            .hunks()
            .iter()
            .map(|hunk| {
                let start = offset;
                offset += 1 + hunk.lines.len();
                start
            })
            .collect()
    }

    fn rebuild_file_starts(&mut self) {
        let mut starts = Vec::with_capacity(self.files.len() + 1);
        let mut total = 0;
        for f in 0..self.files.len() {
            starts.push(total);
            total += self.file_row_count(f);
        }
        starts.push(total);
        self.file_starts = starts;
    }

    fn file_row_count(&self, file: usize) -> usize {
        if self.collapsed[file] {
            return 1;
        }
        let diff = &self.files[file];
        match &diff.content {
            FileContent::Binary => 2,
            FileContent::Text { hunks, .. } if hunks.is_empty() => 2,
            FileContent::Text { hunks, .. } => {
                1 + hunks.iter().map(|h| 1 + h.lines.len()).sum::<usize>()
            }
        }
    }

    /// Total number of rows.
    pub fn row_count(&self) -> usize {
        *self.file_starts.last().unwrap_or(&0)
    }

    /// First row of `file`.
    pub fn file_start(&self, file: usize) -> usize {
        self.file_starts[file]
    }

    /// Address of `row`. `row` must be less than [`Self::row_count`].
    pub fn locate(&self, row: usize) -> RowAddr {
        let file = self.file_starts.partition_point(|&s| s <= row) - 1;
        let offset = row - self.file_starts[file];
        if offset == 0 {
            return RowAddr::FileHeader { file };
        }
        let starts = &self.hunk_starts[file];
        if starts.is_empty() {
            return RowAddr::Notice { file };
        }
        let hunk = starts.partition_point(|&s| s <= offset) - 1;
        match offset - starts[hunk] {
            0 => RowAddr::HunkHeader { file, hunk },
            n => RowAddr::Line {
                file,
                hunk,
                line: n - 1,
            },
        }
    }

    /// Row index of `addr`, or the file header if the file is collapsed.
    pub fn row_of(&self, addr: RowAddr) -> usize {
        let file = addr.file();
        let start = self.file_starts[file];
        if self.collapsed[file] {
            return start;
        }
        match addr {
            RowAddr::FileHeader { .. } => start,
            RowAddr::Notice { .. } => start + 1,
            RowAddr::HunkHeader { hunk, .. } => start + self.hunk_starts[file][hunk],
            RowAddr::Line { hunk, line, .. } => start + self.hunk_starts[file][hunk] + 1 + line,
        }
    }

    /// Builds the rows in `range` (clamped to the row count). Only these rows
    /// are touched, so this is cheap for any diff size.
    pub fn rows(&self, range: Range<usize>) -> Vec<DisplayRow<'_>> {
        let end = range.end.min(self.row_count());
        (range.start.min(end)..end)
            .map(|index| {
                let addr = self.locate(index);
                DisplayRow {
                    index,
                    addr,
                    content: self.content(addr),
                }
            })
            .collect()
    }

    fn content(&self, addr: RowAddr) -> RowContent<'_> {
        match addr {
            RowAddr::FileHeader { file } => RowContent::FileHeader {
                file: &self.files[file],
                collapsed: self.collapsed[file],
            },
            RowAddr::Notice { file } => RowContent::Notice(match self.files[file].content {
                FileContent::Binary => "Binary file not shown",
                FileContent::Text { .. } => "No content changes",
            }),
            RowAddr::HunkHeader { file, hunk } => {
                RowContent::HunkHeader(self.files[file].hunks()[hunk].header())
            }
            RowAddr::Line { file, hunk, line } => {
                let diff = &self.files[file];
                let l = &diff.hunks()[hunk].lines[line];
                RowContent::Line(LineView {
                    kind: l.kind,
                    old_no: l.old.map(|n| n + 1),
                    new_no: l.new.map(|n| n + 1),
                    text: diff.line_text(l),
                })
            }
        }
    }

    /// Cursor row.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// File under the cursor.
    pub fn current_file(&self) -> Option<usize> {
        (self.row_count() > 0).then(|| self.locate(self.cursor).file())
    }

    /// Sets the cursor (clamped).
    pub fn set_cursor(&mut self, row: usize) {
        self.cursor = row.min(self.row_count().saturating_sub(1));
    }

    /// Moves the cursor by `delta` rows.
    pub fn move_by(&mut self, delta: isize) {
        self.set_cursor(self.cursor.saturating_add_signed(delta));
    }

    /// Moves to the first row.
    pub fn top(&mut self) {
        self.cursor = 0;
    }

    /// Moves to the last row.
    pub fn bottom(&mut self) {
        self.set_cursor(usize::MAX);
    }

    /// Moves the cursor to the header of `file`.
    pub fn jump_to_file(&mut self, file: usize) {
        if file < self.files.len() {
            self.cursor = self.file_starts[file];
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::diff::FileStatus;

    pub(crate) fn numbered(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    /// Two text files and a binary file.
    pub(crate) fn sample() -> Arc<Vec<FileDiff>> {
        let old = numbered(20);
        let new = old
            .replace("line 2\n", "line two\n")
            .replace("line 18\n", "line eighteen\nextra\n");
        Arc::new(vec![
            FileDiff::from_blobs(
                "a.rs".into(),
                FileStatus::Modified,
                Some(old.as_bytes()),
                Some(new.as_bytes()),
            ),
            FileDiff::from_blobs("b.bin".into(), FileStatus::Added, None, Some(b"\0\0")),
            FileDiff::from_blobs("c.txt".into(), FileStatus::Added, None, Some(b"x\ny\n")),
        ])
    }

    #[test]
    fn layout_and_locate_roundtrip() {
        let view = DiffView::new(sample());
        let a_hunks = view.files()[0].hunks();
        assert_eq!(a_hunks.len(), 2);
        let a_rows = 1 + a_hunks.iter().map(|h| 1 + h.lines.len()).sum::<usize>();
        assert_eq!(view.file_start(1), a_rows);
        assert_eq!(view.row_count(), a_rows + 2 + 1 + 1 + 2);
        for row in 0..view.row_count() {
            assert_eq!(view.row_of(view.locate(row)), row, "row {row}");
        }
        assert_eq!(view.locate(0), RowAddr::FileHeader { file: 0 });
        assert_eq!(view.locate(1), RowAddr::HunkHeader { file: 0, hunk: 0 });
        assert_eq!(
            view.locate(2),
            RowAddr::Line {
                file: 0,
                hunk: 0,
                line: 0
            }
        );
        assert_eq!(view.locate(a_rows + 1), RowAddr::Notice { file: 1 });
    }

    #[test]
    fn rows_contain_text_and_numbers() {
        let view = DiffView::new(sample());
        let rows = view.rows(0..4);
        assert_eq!(rows.len(), 4);
        assert!(
            matches!(rows[1].content, RowContent::HunkHeader(ref h) if h.starts_with("@@ -1,"))
        );
        match &rows[3].content {
            RowContent::Line(l) => {
                assert_eq!(l.text, "line 2");
                assert_eq!(l.kind, LineKind::Removed);
                assert_eq!(l.old_no, Some(2));
            }
            other => panic!("{other:?}"),
        }
        let end = view.row_count();
        assert_eq!(view.rows(end - 1..end + 10).len(), 1);
        assert!(view.rows(end + 5..end + 10).is_empty());
    }

    #[test]
    fn cursor_moves_and_clamps() {
        let mut view = DiffView::new(sample());
        view.move_by(-3);
        assert_eq!(view.cursor(), 0);
        view.bottom();
        assert_eq!(view.cursor(), view.row_count() - 1);
        assert_eq!(view.current_file(), Some(2));
        view.jump_to_file(1);
        assert_eq!(view.locate(view.cursor()), RowAddr::FileHeader { file: 1 });
        view.top();
        assert_eq!(view.cursor(), 0);
    }

    #[test]
    fn empty_view() {
        let view = DiffView::new(Arc::new(Vec::new()));
        assert_eq!(view.row_count(), 0);
        assert!(view.rows(0..10).is_empty());
        assert_eq!(view.current_file(), None);
    }

    /// Builds a diff with at least `lines` changed lines spread over files.
    pub(crate) fn large_diff(files: usize, lines_per_file: usize) -> Arc<Vec<FileDiff>> {
        let old: String = (0..lines_per_file)
            .map(|i| format!("fn f{i}() {{ let x = {i}; }}\n"))
            .collect();
        let new: String = (0..lines_per_file)
            .map(|i| {
                if i % 3 == 0 {
                    format!("fn f{i}() {{ let y = {}; }}\n", i * 2)
                } else {
                    format!("fn f{i}() {{ let x = {i}; }}\n")
                }
            })
            .collect();
        Arc::new(
            (0..files)
                .map(|f| {
                    FileDiff::from_blobs(
                        format!("src/file{f:03}.rs"),
                        FileStatus::Modified,
                        Some(old.as_bytes()),
                        Some(new.as_bytes()),
                    )
                })
                .collect(),
        )
    }

    /// Computing the visible rows must not depend on the diff size.
    #[test]
    fn visible_rows_are_fast_for_large_diffs() {
        let files = large_diff(20, 1_000);
        let view = DiffView::new(files);
        assert!(view.row_count() >= 10_000, "{}", view.row_count());
        let visible = 60;
        let start = std::time::Instant::now();
        let mut built = 0;
        let step = view.row_count() / 200;
        for i in 0..200 {
            built += view.rows(i * step..i * step + visible).len();
        }
        let elapsed = start.elapsed();
        assert!(built >= 200 * visible - visible);
        let per_frame = elapsed / 200;
        eprintln!(
            "rows: {}, visible-range build: {per_frame:?} per frame",
            view.row_count()
        );
        // Generous bound for unoptimized CI builds; typical is a few µs.
        assert!(
            per_frame < std::time::Duration::from_millis(5),
            "{per_frame:?}"
        );
    }
}
