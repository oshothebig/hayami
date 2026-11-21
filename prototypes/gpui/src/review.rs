use crate::diff::{FileDiff, LineKind};
use std::collections::BTreeSet;

pub fn file_key(file: &FileDiff) -> &str {
    if file.new_path == "/dev/null" {
        &file.old_path
    } else {
        &file.new_path
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Old,
    New,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub file: usize,
    pub side: Side,
    pub start: usize,
    pub end: usize,
}

impl Anchor {
    pub fn label(&self) -> String {
        format!(
            "{} L{}–L{}",
            if self.side == Side::Old { "old" } else { "new" },
            self.start,
            self.end
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub file: usize,
    pub hunk: usize,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub anchor: Anchor,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    File(usize),
    Hunk(usize, usize),
    Line(Position),
    Comment(usize),
    Draft,
}

pub struct ReviewModel {
    pub files: Vec<FileDiff>,
    pub collapsed: BTreeSet<(usize, usize)>,
    pub cursor: Option<Position>,
    pub origin: Option<Position>,
    pub comments: Vec<Comment>,
    pub draft: Option<Anchor>,
}

impl ReviewModel {
    pub fn new(files: Vec<FileDiff>) -> Self {
        Self {
            files,
            collapsed: BTreeSet::new(),
            cursor: None,
            origin: None,
            comments: Vec::new(),
            draft: None,
        }
    }

    pub fn point(&self, p: Position) -> Option<Anchor> {
        let line = self
            .files
            .get(p.file)?
            .hunks
            .get(p.hunk)?
            .lines
            .get(p.line)?;
        let (side, number) = if line.kind == LineKind::Delete {
            (Side::Old, line.old_number?)
        } else {
            (Side::New, line.new_number?)
        };
        Some(Anchor {
            file: p.file,
            side,
            start: number,
            end: number,
        })
    }

    pub fn select(&mut self, p: Position, extend: bool) {
        let Some(point) = self.point(p) else {
            return;
        };
        if extend {
            if let Some(start) = self.origin.and_then(|p| self.point(p)) {
                if start.file != point.file || start.side != point.side {
                    return;
                }
            } else {
                self.origin = Some(p);
            }
        } else {
            self.origin = Some(p);
        }
        self.cursor = Some(p);
    }

    pub fn selection(&self) -> Option<Anchor> {
        let a = self.point(self.origin?)?;
        let b = self.point(self.cursor?)?;
        Some(Anchor {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
            ..a
        })
    }

    pub fn extend(&mut self, direction: isize) {
        let Some(p) = self.cursor else {
            return;
        };
        let candidates: Vec<_> = self.files[p.file]
            .hunks
            .iter()
            .enumerate()
            .flat_map(|(h, hunk)| {
                (0..hunk.lines.len()).map(move |line| Position {
                    file: p.file,
                    hunk: h,
                    line,
                })
            })
            .collect();
        let Some(index) = candidates.iter().position(|item| *item == p) else {
            return;
        };
        let side = self.point(p).unwrap().side;
        let mut next = index as isize + direction;
        while next >= 0 && (next as usize) < candidates.len() {
            let candidate = candidates[next as usize];
            if self.point(candidate).is_some_and(|a| a.side == side) {
                self.select(candidate, true);
                return;
            }
            next += direction;
        }
    }

    pub fn navigate_hunk(
        &self,
        current: (usize, usize),
        direction: isize,
    ) -> Option<(usize, usize)> {
        let hunks: Vec<_> = self
            .files
            .iter()
            .enumerate()
            .flat_map(|(f, file)| (0..file.hunks.len()).map(move |h| (f, h)))
            .collect();
        if hunks.is_empty() {
            return None;
        }
        let index = hunks.iter().position(|h| *h == current).unwrap_or(0);
        Some(hunks[index.saturating_add_signed(direction).min(hunks.len() - 1)])
    }

    pub fn toggle(&mut self, file: usize, hunk: usize) {
        if self
            .files
            .get(file)
            .and_then(|f| f.hunks.get(hunk))
            .is_none()
        {
            return;
        }
        if !self.collapsed.remove(&(file, hunk)) {
            self.collapsed.insert((file, hunk));
        }
    }

    pub fn submit(&mut self, text: &str) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let Some(anchor) = self.draft.take() else {
            return false;
        };
        self.comments.push(Comment {
            anchor,
            text: text.trim().into(),
        });
        true
    }

    fn anchor_hunk(&self, anchor: &Anchor, f: usize, h: usize) -> bool {
        f == anchor.file
            && self.files[f].hunks[h].lines.iter().any(|line| {
                let number = if anchor.side == Side::Old {
                    line.old_number
                } else {
                    line.new_number
                };
                number == Some(anchor.end)
            })
    }

    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (f, file) in self.files.iter().enumerate() {
            rows.push(Row::File(f));
            for (h, hunk) in file.hunks.iter().enumerate() {
                rows.push(Row::Hunk(f, h));
                let folded = self.collapsed.contains(&(f, h));
                if !folded {
                    for l in 0..hunk.lines.len() {
                        let p = Position {
                            file: f,
                            hunk: h,
                            line: l,
                        };
                        rows.push(Row::Line(p));
                        for (i, comment) in self.comments.iter().enumerate() {
                            let number = if comment.anchor.side == Side::Old {
                                hunk.lines[l].old_number
                            } else {
                                hunk.lines[l].new_number
                            };
                            if comment.anchor.file == f && number == Some(comment.anchor.end) {
                                rows.push(Row::Comment(i));
                            }
                        }
                        if let Some(anchor) = &self.draft {
                            let number = if anchor.side == Side::Old {
                                hunk.lines[l].old_number
                            } else {
                                hunk.lines[l].new_number
                            };
                            if anchor.file == f && number == Some(anchor.end) {
                                rows.push(Row::Draft);
                            }
                        }
                    }
                } else {
                    for (i, comment) in self.comments.iter().enumerate() {
                        if self.anchor_hunk(&comment.anchor, f, h) {
                            rows.push(Row::Comment(i));
                        }
                    }
                    if self
                        .draft
                        .as_ref()
                        .is_some_and(|a| self.anchor_hunk(a, f, h))
                    {
                        rows.push(Row::Draft);
                    }
                }
            }
        }
        rows
    }

    pub fn row_file(&self, row: Row) -> Option<usize> {
        match row {
            Row::File(f) | Row::Hunk(f, _) => Some(f),
            Row::Line(p) => Some(p.file),
            Row::Comment(i) => Some(self.comments[i].anchor.file),
            Row::Draft => self.draft.as_ref().map(|a| a.file),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> ReviewModel {
        ReviewModel::new(crate::diff::parse("diff --git a/a.rs b/a.rs\n@@ -1,2 +1,3 @@\n same\n-old\n+new\n+more\ndiff --git a/b.rs b/b.rs\n@@ -0,0 +1 @@\n+b\n").unwrap())
    }
    fn p(line: usize) -> Position {
        Position {
            file: 0,
            hunk: 0,
            line,
        }
    }
    #[test]
    fn selects_reverse_range_and_skips_other_side() {
        let mut m = model();
        m.select(p(3), false);
        m.extend(-1);
        assert_eq!(
            m.selection().unwrap(),
            Anchor {
                file: 0,
                side: Side::New,
                start: 2,
                end: 3
            }
        );
        m.select(p(1), true);
        assert_eq!(m.cursor, Some(p(2)));
        m.select(Position { file: 9, ..p(0) }, false);
        assert_eq!(m.cursor, Some(p(2)));
    }
    #[test]
    fn comments_survive_folding_and_row_changes() {
        let mut m = model();
        m.select(p(2), false);
        m.select(p(3), true);
        m.draft = m.selection();
        assert!(!m.submit("  "));
        assert!(m.draft.is_some());
        assert!(m.submit("Looks good"));
        let anchor = m.comments[0].anchor.clone();
        m.toggle(0, 0);
        assert!(m.rows().contains(&Row::Comment(0)));
        assert!(!m.rows().contains(&Row::Line(p(2))));
        m.toggle(0, 0);
        assert_eq!(m.comments[0].anchor, anchor);
        assert!(m.rows().contains(&Row::Comment(0)));
    }
    #[test]
    fn deleted_files_have_distinct_comment_anchors() {
        let mut m = ReviewModel::new(crate::diff::parse("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-a\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-b\n").unwrap());
        m.select(p(0), false);
        m.draft = m.selection();
        assert!(m.submit("Removed intentionally"));
        assert_eq!(m.comments[0].anchor.file, 0);
        assert_eq!(m.comments[0].anchor.side, Side::Old);
        assert_eq!(
            m.rows().iter().filter(|r| **r == Row::Comment(0)).count(),
            1
        );
        assert_eq!(m.row_file(Row::Comment(0)), Some(0));
    }
    #[test]
    fn deletion_and_rename_to_same_path_have_distinct_anchors() {
        let mut m = ReviewModel::new(crate::diff::parse("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-removed\ndiff --git a/b.rs b/a.rs\nrename from b.rs\nrename to a.rs\n--- a/b.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-before\n+after\n").unwrap());
        let renamed = Position { file: 1, ..p(0) };
        assert_ne!(m.point(p(0)).unwrap().file, m.point(renamed).unwrap().file);
        m.select(p(0), false);
        m.select(renamed, true);
        assert_eq!(m.cursor, Some(p(0)));
        m.select(renamed, false);
        m.draft = m.selection();
        assert_eq!(m.rows().iter().filter(|r| **r == Row::Draft).count(), 1);
        assert_eq!(m.row_file(Row::Draft), Some(1));
        assert!(m.submit("Rename comment"));
        assert_eq!(
            m.rows().iter().filter(|r| **r == Row::Comment(0)).count(),
            1
        );
        assert_eq!(m.row_file(Row::Comment(0)), Some(1));
        m.toggle(0, 0);
        m.toggle(1, 0);
        assert_eq!(
            m.rows(),
            vec![
                Row::File(0),
                Row::Hunk(0, 0),
                Row::File(1),
                Row::Hunk(1, 0),
                Row::Comment(0)
            ]
        );
    }
    #[test]
    fn navigation_clamps_and_handles_empty() {
        let m = model();
        assert_eq!(m.navigate_hunk((0, 0), 1), Some((1, 0)));
        assert_eq!(m.navigate_hunk((1, 0), 1), Some((1, 0)));
        assert_eq!(m.navigate_hunk((0, 0), -1), Some((0, 0)));
        let mut m = ReviewModel::new(Vec::new());
        assert!(m.rows().is_empty());
        assert!(m.selection().is_none());
        assert_eq!(m.navigate_hunk((0, 0), 1), None);
        assert!(!m.submit("x"));
        m.toggle(0, 0);
    }
}
