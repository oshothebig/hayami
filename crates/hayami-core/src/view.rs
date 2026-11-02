//! Display row model of the diff view with a cursor.
//!
//! The view never materializes all rows. It keeps prefix sums of row counts
//! per file and per hunk, so locating a row is two binary searches and
//! [`DiffView::rows`] only builds the rows of the requested (visible) range.

use std::ops::Range;
use std::sync::Arc;

use crate::diff::{FileContent, FileDiff, LineKind};
use crate::highlight::{FileHighlights, HighlightKind, Span};
use crate::intraline::word_diff;

/// Unified or side-by-side display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// One column with removed and added lines interleaved.
    #[default]
    Unified,
    /// Old and new side by side.
    Split,
}

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
        /// Row within the hunk body: an index into [`crate::diff::Hunk::lines`]
        /// in unified mode, into [`crate::diff::Hunk::split`] in split mode.
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
    /// Syntax highlight spans (empty until highlighting finished).
    pub spans: &'a [Span],
    /// Byte ranges changed within the line (intra-line diff).
    pub emphasis: Vec<Range<usize>>,
}

/// A styled piece of a line, ready for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Byte range in the display text.
    pub range: Range<usize>,
    /// Syntax category, if any.
    pub kind: Option<HighlightKind>,
    /// Whether the piece changed within the line.
    pub emphasized: bool,
}

/// Width tabs are expanded to.
pub const TAB_WIDTH: usize = 4;

impl LineView<'_> {
    /// Display text (tabs expanded) and non-overlapping styled segments
    /// covering it.
    pub fn segments(&self) -> (String, Vec<Segment>) {
        let text = self.text;
        let mut cuts = vec![0, text.len()];
        for span in self.spans {
            cuts.extend([
                span.range.start.min(text.len()),
                span.range.end.min(text.len()),
            ]);
        }
        for r in &self.emphasis {
            cuts.extend([r.start.min(text.len()), r.end.min(text.len())]);
        }
        cuts.sort_unstable();
        cuts.dedup();

        // Offsets shift by TAB_WIDTH - 1 for every tab before them.
        let tabs: Vec<usize> = text.match_indices('\t').map(|(i, _)| i).collect();
        let map = |offset: usize| offset + tabs.partition_point(|&t| t < offset) * (TAB_WIDTH - 1);
        let display = text.replace('\t', &" ".repeat(TAB_WIDTH));

        let mut segments: Vec<Segment> = Vec::new();
        for pair in cuts.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            if start >= end {
                continue;
            }
            let kind = self
                .spans
                .iter()
                .find(|s| s.range.start <= start && start < s.range.end)
                .map(|s| s.kind);
            let emphasized = self
                .emphasis
                .iter()
                .any(|r| r.start <= start && start < r.end);
            let range = map(start)..map(end);
            match segments.last_mut() {
                Some(last) if last.kind == kind && last.emphasized == emphasized => {
                    last.range.end = range.end;
                }
                _ => segments.push(Segment {
                    range,
                    kind,
                    emphasized,
                }),
            }
        }
        (display, segments)
    }
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
    /// A diff line (unified mode).
    Line(LineView<'a>),
    /// A side-by-side row (split mode).
    Split {
        /// Old side.
        left: Option<LineView<'a>>,
        /// New side.
        right: Option<LineView<'a>>,
    },
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
    highlights: Vec<Option<Arc<FileHighlights>>>,
    mode: ViewMode,
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
            highlights: vec![None; files.len()],
            mode: ViewMode::Unified,
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

    /// Current display mode.
    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    /// Switches the display mode, keeping the cursor on the same line.
    pub fn set_mode(&mut self, mode: ViewMode) {
        if mode == self.mode {
            return;
        }
        let addr = (self.row_count() > 0).then(|| self.locate(self.cursor));
        let old_mode = self.mode;
        self.mode = mode;
        self.relayout();
        if let Some(addr) = addr {
            let mapped = match addr {
                RowAddr::Line { file, hunk, line } => {
                    let h = &self.files[file].hunks()[hunk];
                    let line = match old_mode {
                        // Unified line → split row containing it.
                        ViewMode::Unified => h.split_row_of(line),
                        // Split row → its new-side line, else the old side.
                        ViewMode::Split => {
                            let row = h.split[line];
                            row.right.or(row.left).unwrap_or(0) as usize
                        }
                    };
                    RowAddr::Line { file, hunk, line }
                }
                other => other,
            };
            self.cursor = self.row_of(mapped);
        }
    }

    /// Toggles between unified and split.
    pub fn toggle_mode(&mut self) {
        self.set_mode(match self.mode {
            ViewMode::Unified => ViewMode::Split,
            ViewMode::Split => ViewMode::Unified,
        });
    }

    /// Attaches syntax highlights of `file` (computed in the background).
    pub fn set_highlights(&mut self, file: usize, highlights: FileHighlights) {
        if let Some(slot) = self.highlights.get_mut(file) {
            *slot = Some(Arc::new(highlights));
        }
    }

    /// Number of body rows of a hunk in the current mode.
    fn hunk_body_rows(&self, hunk: &crate::diff::Hunk) -> usize {
        match self.mode {
            ViewMode::Unified => hunk.lines.len(),
            ViewMode::Split => hunk.split.len(),
        }
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
                offset += 1 + self.hunk_body_rows(hunk);
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
                1 + hunks
                    .iter()
                    .map(|h| 1 + self.hunk_body_rows(h))
                    .sum::<usize>()
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
            RowAddr::Line { file, hunk, line } => match self.mode {
                ViewMode::Unified => RowContent::Line(self.line_view(file, hunk, line)),
                ViewMode::Split => {
                    let row = self.files[file].hunks()[hunk].split[line];
                    RowContent::Split {
                        left: row.left.map(|l| self.line_view(file, hunk, l as usize)),
                        right: row.right.map(|l| self.line_view(file, hunk, l as usize)),
                    }
                }
            },
        }
    }

    /// Builds one line with its highlight spans and intra-line emphasis.
    fn line_view(&self, file: usize, hunk: usize, line: usize) -> LineView<'_> {
        let diff = &self.files[file];
        let h = &diff.hunks()[hunk];
        let l = &h.lines[line];
        let text = diff.line_text(l);
        let highlights = self.highlights[file].as_deref();
        let spans = match (l.kind, highlights) {
            (LineKind::Removed, Some(hl)) => side_spans(hl.old.as_ref(), l.old),
            (_, Some(hl)) => side_spans(hl.new.as_ref(), l.new),
            (_, None) => &[],
        };
        let emphasis = match (l.kind, h.partner[line]) {
            (LineKind::Context, _) | (_, None) => Vec::new(),
            (kind, Some(partner)) => {
                let other = diff.line_text(&h.lines[partner as usize]);
                if kind == LineKind::Removed {
                    word_diff(text, other).old
                } else {
                    word_diff(other, text).new
                }
            }
        };
        LineView {
            kind: l.kind,
            old_no: l.old.map(|n| n + 1),
            new_no: l.new.map(|n| n + 1),
            text,
            spans,
            emphasis,
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

    /// Whether `file` is collapsed.
    pub fn is_collapsed(&self, file: usize) -> bool {
        self.collapsed.get(file).copied().unwrap_or(false)
    }

    /// Collapses or expands `file`; the cursor moves to its header.
    pub fn toggle_collapse(&mut self, file: usize) {
        if file >= self.files.len() {
            return;
        }
        self.collapsed[file] = !self.collapsed[file];
        self.rebuild_file_starts();
        self.cursor = self.file_starts[file];
    }

    /// Collapses all files, or expands all if every file is collapsed.
    /// The cursor stays on the current file's header.
    pub fn toggle_collapse_all(&mut self) {
        let current = self.current_file();
        let collapse = self.collapsed.iter().any(|c| !c);
        self.collapsed.iter_mut().for_each(|c| *c = collapse);
        self.rebuild_file_starts();
        self.cursor = current.map_or(0, |f| self.file_starts[f]);
    }

    /// Visible hunk header rows of `file` (none when collapsed).
    fn hunk_rows(&self, file: usize) -> impl DoubleEndedIterator<Item = usize> + '_ {
        let start = self.file_starts[file];
        let starts: &[usize] = if self.collapsed[file] {
            &[]
        } else {
            &self.hunk_starts[file]
        };
        starts.iter().map(move |s| start + s)
    }

    /// Moves to the next hunk header after the cursor, across files.
    /// Returns false (and stays) if there is none.
    pub fn next_hunk(&mut self) -> bool {
        let Some(current) = self.current_file() else {
            return false;
        };
        let cursor = self.cursor;
        let target = (current..self.files.len())
            .flat_map(|f| self.hunk_rows(f))
            .find(|&row| row > cursor);
        target.map(|row| self.cursor = row).is_some()
    }

    /// Moves to the start of the current hunk, or to the previous hunk when
    /// already on a hunk header. Returns false if there is none.
    pub fn prev_hunk(&mut self) -> bool {
        let Some(current) = self.current_file() else {
            return false;
        };
        let cursor = self.cursor;
        let target = (0..=current)
            .rev()
            .flat_map(|f| self.hunk_rows(f).rev())
            .find(|&row| row < cursor);
        target.map(|row| self.cursor = row).is_some()
    }

    /// Moves to the next file's header.
    pub fn next_file(&mut self) -> bool {
        match self.current_file() {
            Some(f) if f + 1 < self.files.len() => {
                self.cursor = self.file_starts[f + 1];
                true
            }
            _ => false,
        }
    }

    /// Moves to the current file's header, or to the previous file's header
    /// when already on a header.
    pub fn prev_file(&mut self) -> bool {
        let Some(f) = self.current_file() else {
            return false;
        };
        if self.cursor > self.file_starts[f] {
            self.cursor = self.file_starts[f];
            true
        } else if f > 0 {
            self.cursor = self.file_starts[f - 1];
            true
        } else {
            false
        }
    }

    /// Moves the cursor to the header of `file`.
    pub fn jump_to_file(&mut self, file: usize) {
        if file < self.files.len() {
            self.cursor = self.file_starts[file];
        }
    }
}

fn side_spans(lines: Option<&crate::highlight::LineSpans>, index: Option<u32>) -> &[Span] {
    match (lines, index) {
        (Some(lines), Some(i)) => lines.get(i as usize).map_or(&[], Vec::as_slice),
        _ => &[],
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
    fn hunk_navigation_crosses_files() {
        let mut view = DiffView::new(sample());
        let h0 = view.row_of(RowAddr::HunkHeader { file: 0, hunk: 0 });
        let h1 = view.row_of(RowAddr::HunkHeader { file: 0, hunk: 1 });
        let c0 = view.row_of(RowAddr::HunkHeader { file: 2, hunk: 0 });
        assert!(view.next_hunk());
        assert_eq!(view.cursor(), h0);
        assert!(view.next_hunk());
        assert_eq!(view.cursor(), h1);
        // The binary file has no hunks, so the next one is in c.txt.
        assert!(view.next_hunk());
        assert_eq!(view.cursor(), c0);
        assert!(!view.next_hunk());
        assert_eq!(view.cursor(), c0);
        // From inside a hunk, `p` goes to its header first.
        view.set_cursor(h1 + 2);
        assert!(view.prev_hunk());
        assert_eq!(view.cursor(), h1);
        assert!(view.prev_hunk());
        assert_eq!(view.cursor(), h0);
        assert!(!view.prev_hunk());
    }

    #[test]
    fn file_navigation() {
        let mut view = DiffView::new(sample());
        assert!(view.next_file());
        assert_eq!(view.cursor(), view.file_start(1));
        assert!(view.next_file());
        assert!(!view.next_file());
        view.move_by(1);
        assert!(view.prev_file());
        assert_eq!(view.cursor(), view.file_start(2));
        assert!(view.prev_file());
        assert_eq!(view.cursor(), view.file_start(1));
        view.top();
        assert!(!view.prev_file());
    }

    #[test]
    fn collapsing_hides_rows_and_hunks() {
        let mut view = DiffView::new(sample());
        let total = view.row_count();
        let a_rows = view.file_start(1);
        view.set_cursor(5);
        view.toggle_collapse(0);
        assert!(view.is_collapsed(0));
        assert_eq!(view.cursor(), 0);
        assert_eq!(view.row_count(), total - a_rows + 1);
        assert_eq!(view.locate(1), RowAddr::FileHeader { file: 1 });
        assert!(matches!(
            view.rows(0..1)[0].content,
            RowContent::FileHeader {
                collapsed: true,
                ..
            }
        ));
        // Hunks of the collapsed file are skipped.
        assert!(view.next_hunk());
        assert_eq!(
            view.locate(view.cursor()),
            RowAddr::HunkHeader { file: 2, hunk: 0 }
        );
        for row in 0..view.row_count() {
            assert_eq!(view.row_of(view.locate(row)), row);
        }
        view.toggle_collapse(0);
        assert_eq!(view.row_count(), total);
    }

    #[test]
    fn collapse_all_toggles() {
        let mut view = DiffView::new(sample());
        view.jump_to_file(2);
        view.toggle_collapse_all();
        assert_eq!(view.row_count(), 3);
        assert_eq!(view.cursor(), 2);
        view.toggle_collapse_all();
        assert!(!view.is_collapsed(1));
        assert_eq!(view.cursor(), view.file_start(2));
    }

    #[test]
    fn split_rows_pair_old_and_new() {
        let old = "a\nb\nc\n";
        let new = "a\nB\nc\nd\n";
        let files = Arc::new(vec![FileDiff::from_blobs(
            "f.txt".into(),
            FileStatus::Modified,
            Some(old.as_bytes()),
            Some(new.as_bytes()),
        )]);
        let mut view = DiffView::new(files);
        // Unified: header, hunk header, a, -b, +B, c, +d.
        assert_eq!(view.row_count(), 7);
        view.set_mode(ViewMode::Split);
        // Split: header, hunk header, a|a, b|B, c|c, _|d.
        assert_eq!(view.row_count(), 6);
        let cells: Vec<(Option<u32>, Option<u32>)> = view
            .rows(2..6)
            .iter()
            .map(|r| match &r.content {
                RowContent::Split { left, right } => (
                    left.as_ref().and_then(|l| l.old_no),
                    right.as_ref().and_then(|l| l.new_no),
                ),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            cells,
            [
                (Some(1), Some(1)),
                (Some(2), Some(2)),
                (Some(3), Some(3)),
                (None, Some(4))
            ]
        );
        for row in 0..view.row_count() {
            assert_eq!(view.row_of(view.locate(row)), row);
        }
    }

    #[test]
    fn mode_switch_keeps_cursor_line() {
        let old = "a\nb\nc\n";
        let new = "a\nB\nc\nd\n";
        let files = Arc::new(vec![FileDiff::from_blobs(
            "f.txt".into(),
            FileStatus::Modified,
            Some(old.as_bytes()),
            Some(new.as_bytes()),
        )]);
        let mut view = DiffView::new(files);
        // Cursor on "+B" (unified row 4).
        view.set_cursor(4);
        view.toggle_mode();
        assert_eq!(view.mode(), ViewMode::Split);
        assert_eq!(view.cursor(), 3);
        // Cursor on "_|d" → unified "+d".
        view.set_cursor(5);
        view.toggle_mode();
        assert_eq!(view.mode(), ViewMode::Unified);
        assert_eq!(view.cursor(), 6);
        match &view.rows(6..7)[0].content {
            RowContent::Line(l) => assert_eq!(l.text, "d"),
            other => panic!("{other:?}"),
        }
        // Headers map to headers.
        view.set_cursor(1);
        view.toggle_mode();
        assert_eq!(
            view.locate(view.cursor()),
            RowAddr::HunkHeader { file: 0, hunk: 0 }
        );
    }

    #[test]
    fn paired_lines_get_word_emphasis() {
        let old = "let total = compute(a);\n";
        let new = "let total = calculate(a);\n";
        let files = Arc::new(vec![FileDiff::from_blobs(
            "f.rs".into(),
            FileStatus::Modified,
            Some(old.as_bytes()),
            Some(new.as_bytes()),
        )]);
        let view = DiffView::new(files);
        let rows = view.rows(2..4);
        let (RowContent::Line(removed), RowContent::Line(added)) =
            (&rows[0].content, &rows[1].content)
        else {
            panic!("{rows:?}");
        };
        assert_eq!(&removed.text[removed.emphasis[0].clone()], "compute");
        assert_eq!(&added.text[added.emphasis[0].clone()], "calculate");
    }

    #[test]
    fn highlights_follow_whole_file_across_hunk_start() {
        use crate::highlight::{HighlightKind, highlight_file};
        // The hunk starts inside a block comment that opens far above it.
        let mut old = String::from("/*\n");
        for i in 0..10 {
            old.push_str(&format!("let x{i} = \"not code\";\n"));
        }
        old.push_str("*/\nfn main() {}\n");
        let new = old.replace("let x8 = \"not code\";", "let x8 = \"still a comment\";");
        let files = Arc::new(vec![FileDiff::from_blobs(
            "lib.rs".into(),
            FileStatus::Modified,
            Some(old.as_bytes()),
            Some(new.as_bytes()),
        )]);
        let mut view = DiffView::new(files.clone());
        // Before highlighting finishes, lines are plain.
        let plain = view.rows(0..view.row_count());
        assert!(plain.iter().all(|r| match &r.content {
            RowContent::Line(l) => l.spans.is_empty(),
            _ => true,
        }));
        view.set_highlights(0, highlight_file(&files[0]));
        let rows = view.rows(0..view.row_count());
        let lines: Vec<&LineView> = rows
            .iter()
            .filter_map(|r| match &r.content {
                RowContent::Line(l) => Some(l),
                _ => None,
            })
            .collect();
        // Hunk starts at "let x5" (3 lines of context), inside the comment.
        assert_eq!(lines[0].text, "let x5 = \"not code\";");
        for line in lines.iter().take_while(|l| l.text.starts_with("let")) {
            assert_eq!(
                line.spans.iter().map(|s| s.kind).collect::<Vec<_>>(),
                [HighlightKind::Comment],
                "{}",
                line.text
            );
        }
        let main = lines.iter().find(|l| l.text.starts_with("fn")).unwrap();
        assert_eq!(main.spans[0].kind, HighlightKind::Keyword);
    }

    #[test]
    fn segments_merge_spans_emphasis_and_expand_tabs() {
        use crate::highlight::HighlightKind;
        let spans = [
            Span {
                range: 1..4,
                kind: HighlightKind::Keyword,
            },
            Span {
                range: 5..8,
                kind: HighlightKind::String,
            },
        ];
        let line = LineView {
            kind: LineKind::Added,
            old_no: None,
            new_no: Some(1),
            text: "\tlet \"a\"",
            spans: &spans,
            emphasis: std::iter::once(6..7).collect(),
        };
        let (display, segments) = line.segments();
        assert_eq!(display, "    let \"a\"");
        let pieces: Vec<(&str, Option<HighlightKind>, bool)> = segments
            .iter()
            .map(|s| (&display[s.range.clone()], s.kind, s.emphasized))
            .collect();
        assert_eq!(
            pieces,
            [
                ("    ", None, false),
                ("let", Some(HighlightKind::Keyword), false),
                (" ", None, false),
                ("\"", Some(HighlightKind::String), false),
                ("a", Some(HighlightKind::String), true),
                ("\"", Some(HighlightKind::String), false),
            ]
        );
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
        // Generous bound for unoptimized CI builds (this includes the
        // intra-line diff of visible changed lines); release builds take
        // well under 0.1 ms.
        assert!(
            per_frame < std::time::Duration::from_millis(8),
            "{per_frame:?}"
        );
    }

    /// Same in split mode with highlights attached, including building the
    /// styled segments the UI renders.
    #[test]
    fn visible_split_rows_with_highlights_are_fast() {
        let files = large_diff(20, 1_000);
        let mut view = DiffView::new(files.clone());
        view.set_mode(ViewMode::Split);
        view.set_highlights(0, crate::highlight::highlight_file(&files[0]));
        assert!(view.row_count() >= 10_000, "{}", view.row_count());
        let visible = 60;
        let step = view.row_count() / 200;
        let start = std::time::Instant::now();
        for i in 0..200 {
            for row in view.rows(i * step..i * step + visible) {
                if let RowContent::Split { left, right } = &row.content {
                    for side in [left, right].into_iter().flatten() {
                        std::hint::black_box(side.segments());
                    }
                }
            }
        }
        let per_frame = start.elapsed() / 200;
        eprintln!(
            "split rows: {}, visible-range build with segments: {per_frame:?} per frame",
            view.row_count()
        );
        assert!(
            per_frame < std::time::Duration::from_millis(16),
            "{per_frame:?}"
        );
    }
}
