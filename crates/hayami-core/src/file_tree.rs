//! File tree of the changed files, shown next to the diff.

use crate::diff::FileDiff;

/// One row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    /// Directory heading.
    Dir {
        /// Directory name (one path component).
        name: String,
        /// Nesting depth (0 for top level).
        depth: usize,
    },
    /// A changed file.
    File {
        /// File name (last path component).
        name: String,
        /// Nesting depth.
        depth: usize,
        /// Index into the diff's file list.
        file: usize,
    },
}

/// Tree rows plus the selected file.
#[derive(Debug, Clone, Default)]
pub struct FileTree {
    rows: Vec<TreeRow>,
    /// Row index of each file, indexed by file index.
    file_rows: Vec<usize>,
    selected: usize,
}

impl FileTree {
    /// Builds the tree. `files` must be sorted by path so that every
    /// directory's entries are contiguous.
    pub fn new(files: &[FileDiff]) -> Self {
        let mut rows = Vec::new();
        let mut file_rows = Vec::with_capacity(files.len());
        let mut open_dirs: Vec<&str> = Vec::new();
        for (index, file) in files.iter().enumerate() {
            let mut parts: Vec<&str> = file.path.split('/').collect();
            let name = parts.pop().unwrap_or_default();
            let common = open_dirs
                .iter()
                .zip(&parts)
                .take_while(|(a, b)| a == b)
                .count();
            open_dirs.truncate(common);
            for (depth, dir) in parts.iter().enumerate().skip(common) {
                rows.push(TreeRow::Dir {
                    name: (*dir).to_owned(),
                    depth,
                });
                open_dirs.push(dir);
            }
            file_rows.push(rows.len());
            rows.push(TreeRow::File {
                name: name.to_owned(),
                depth: parts.len(),
                file: index,
            });
        }
        Self {
            rows,
            file_rows,
            selected: 0,
        }
    }

    /// All rows.
    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    /// Selected file index.
    pub fn selected_file(&self) -> usize {
        self.selected
    }

    /// Row index of the selected file.
    pub fn selected_row(&self) -> usize {
        self.file_rows.get(self.selected).copied().unwrap_or(0)
    }

    /// Selects file `index` (clamped).
    pub fn select_file(&mut self, index: usize) {
        self.selected = index.min(self.file_rows.len().saturating_sub(1));
    }

    /// Moves the selection by `delta` files.
    pub fn move_by(&mut self, delta: isize) {
        self.select_file(self.selected.saturating_add_signed(delta));
    }

    /// Selects the last file.
    pub fn select_last(&mut self) {
        self.select_file(usize::MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::FileStatus;

    fn files(paths: &[&str]) -> Vec<FileDiff> {
        paths
            .iter()
            .map(|p| FileDiff::from_blobs((*p).into(), FileStatus::Added, None, Some(b"x\n")))
            .collect()
    }

    #[test]
    fn builds_nested_rows() {
        let tree = FileTree::new(&files(&[
            "README.md",
            "src/a.rs",
            "src/ui/b.rs",
            "src/z.rs",
        ]));
        let names: Vec<String> = tree
            .rows()
            .iter()
            .map(|r| match r {
                TreeRow::Dir { name, depth } => format!("{}{name}/", "  ".repeat(*depth)),
                TreeRow::File { name, depth, .. } => format!("{}{name}", "  ".repeat(*depth)),
            })
            .collect();
        assert_eq!(
            names,
            ["README.md", "src/", "  a.rs", "  ui/", "    b.rs", "  z.rs"]
        );
    }

    #[test]
    fn selection_moves_over_files_only() {
        let mut tree = FileTree::new(&files(&["a/x", "a/y", "b/z"]));
        assert_eq!(tree.selected_row(), 1);
        tree.move_by(2);
        assert_eq!(tree.selected_file(), 2);
        assert_eq!(tree.selected_row(), 4);
        tree.move_by(10);
        assert_eq!(tree.selected_file(), 2);
        tree.move_by(-10);
        assert_eq!(tree.selected_file(), 0);
        tree.select_last();
        assert_eq!(tree.selected_file(), 2);
    }
}
