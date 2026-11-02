//! State of the PR list screen: two groups with a single selection cursor.

use crate::github::{PrList, PrSummary};

/// Which group a row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrGroup {
    /// PRs requesting the viewer's review.
    ReviewRequested,
    /// PRs authored by the viewer.
    Authored,
}

impl PrGroup {
    /// Heading shown above the group.
    pub fn title(self) -> &'static str {
        match self {
            PrGroup::ReviewRequested => "Review requested",
            PrGroup::Authored => "Your pull requests",
        }
    }
}

/// One rendered row of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrListRow<'a> {
    /// Group heading with the number of PRs in it.
    Header(PrGroup, usize),
    /// A PR; `selected` is true for the cursor row.
    Pr {
        /// The PR.
        pr: &'a PrSummary,
        /// Whether the cursor is on it.
        selected: bool,
        /// Index among selectable PRs.
        index: usize,
    },
    /// Placeholder for an empty group.
    Empty(PrGroup),
}

/// PR list with selection.
#[derive(Debug, Clone, Default)]
pub struct PrListState {
    list: PrList,
    selected: usize,
}

impl PrListState {
    /// Creates the state, keeping the selection on the same PR when possible.
    pub fn new(list: PrList) -> Self {
        Self { list, selected: 0 }
    }

    /// Replaces the list (after reload), keeping the selected PR if present.
    pub fn replace(&mut self, list: PrList) {
        let previous = self.selected_pr().map(|p| p.pr.clone());
        self.list = list;
        self.selected = previous
            .and_then(|pr| self.prs().position(|p| p.pr == pr))
            .unwrap_or(0);
        self.clamp();
    }

    fn prs(&self) -> impl Iterator<Item = &PrSummary> {
        self.list
            .review_requested
            .iter()
            .chain(self.list.authored.iter())
    }

    /// Number of selectable PRs.
    pub fn len(&self) -> usize {
        self.list.review_requested.len() + self.list.authored.len()
    }

    /// Whether both groups are empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Index of the selected PR.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// The selected PR.
    pub fn selected_pr(&self) -> Option<&PrSummary> {
        self.prs().nth(self.selected)
    }

    fn clamp(&mut self) {
        self.selected = self.selected.min(self.len().saturating_sub(1));
    }

    /// Moves the selection by `delta`, clamped to the list.
    pub fn move_by(&mut self, delta: isize) {
        self.selected = self.selected.saturating_add_signed(delta);
        self.clamp();
    }

    /// Selects the first PR.
    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    /// Selects the last PR.
    pub fn select_last(&mut self) {
        self.selected = self.len().saturating_sub(1);
    }

    /// Selects the PR at `index` (clamped).
    pub fn select(&mut self, index: usize) {
        self.selected = index;
        self.clamp();
    }

    /// Rows to render, including group headers.
    pub fn rows(&self) -> Vec<PrListRow<'_>> {
        let mut rows = Vec::with_capacity(self.len() + 4);
        let mut index = 0;
        for (group, prs) in [
            (PrGroup::ReviewRequested, &self.list.review_requested),
            (PrGroup::Authored, &self.list.authored),
        ] {
            rows.push(PrListRow::Header(group, prs.len()));
            if prs.is_empty() {
                rows.push(PrListRow::Empty(group));
            }
            for pr in prs {
                rows.push(PrListRow::Pr {
                    pr,
                    selected: index == self.selected,
                    index,
                });
                index += 1;
            }
        }
        rows
    }

    /// Row index (in [`Self::rows`]) of the selected PR, for scrolling.
    pub fn selected_row(&self) -> usize {
        self.rows()
            .iter()
            .position(|r| matches!(r, PrListRow::Pr { selected: true, .. }))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::PrRef;

    fn pr(owner: &str, number: u64) -> PrSummary {
        PrSummary {
            pr: PrRef {
                owner: owner.into(),
                repo: "r".into(),
                number,
            },
            title: format!("PR {number}"),
            author: "a".into(),
            url: String::new(),
            is_draft: false,
            updated_at: String::new(),
            additions: 0,
            deletions: 0,
            review_decision: None,
        }
    }

    fn state() -> PrListState {
        PrListState::new(PrList {
            review_requested: vec![pr("x", 1), pr("x", 2)],
            authored: vec![pr("me", 3)],
        })
    }

    #[test]
    fn moves_across_groups_and_clamps() {
        let mut s = state();
        assert_eq!(s.selected_pr().unwrap().pr.number, 1);
        s.move_by(2);
        assert_eq!(s.selected_pr().unwrap().pr.number, 3);
        s.move_by(5);
        assert_eq!(s.selected_pr().unwrap().pr.number, 3);
        s.move_by(-10);
        assert_eq!(s.selected_index(), 0);
        s.select_last();
        assert_eq!(s.selected_index(), 2);
        s.select_first();
        assert_eq!(s.selected_index(), 0);
    }

    #[test]
    fn rows_have_headers_and_placeholders() {
        let s = PrListState::new(PrList {
            review_requested: vec![],
            authored: vec![pr("me", 3)],
        });
        let rows = s.rows();
        assert_eq!(rows[0], PrListRow::Header(PrGroup::ReviewRequested, 0));
        assert_eq!(rows[1], PrListRow::Empty(PrGroup::ReviewRequested));
        assert_eq!(rows[2], PrListRow::Header(PrGroup::Authored, 1));
        assert!(matches!(rows[3], PrListRow::Pr { selected: true, .. }));
        assert_eq!(s.selected_row(), 3);
    }

    #[test]
    fn replace_keeps_selected_pr() {
        let mut s = state();
        s.move_by(1);
        s.replace(PrList {
            review_requested: vec![pr("x", 0), pr("x", 1), pr("x", 2)],
            authored: vec![],
        });
        assert_eq!(s.selected_pr().unwrap().pr.number, 2);
        s.replace(PrList::default());
        assert!(s.selected_pr().is_none());
        assert!(s.is_empty());
    }
}
