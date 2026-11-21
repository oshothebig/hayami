//! Maps the lines of a [`nsh_base::DiffView`] patch back to files and line numbers.
//!
//! A patch's files are joined into one old and one new text: file `i` starts at
//! the sum of the excerpt lines (hunk lines) of the files before it. The view
//! reports selections and takes annotations in those joined lines, while a
//! reviewer reads paths and the numbers of the original files.

use nsh_base::DiffSide;

/// One file of a unified diff, with the original line number of each excerpt line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchFile {
    pub path: String,
    pub old_numbers: Vec<u32>,
    pub new_numbers: Vec<u32>,
    /// The first line of each hunk in this file's new excerpt, and the same in the old one.
    pub hunk_starts: Vec<(u32, u32)>,
}

/// The line numbers of one side of a file.
type Numbers = fn(&PatchFile) -> &[u32];

/// A unified diff's files and where each starts in the view's joined texts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchIndex {
    pub files: Vec<PatchFile>,
    old_at: Vec<u32>,
    new_at: Vec<u32>,
}

impl PatchIndex {
    pub fn parse(patch: &str) -> Self {
        let mut files: Vec<PatchFile> = Vec::new();
        let mut in_hunk = false;
        let (mut old, mut new) = (0u32, 0u32);
        for line in patch.lines() {
            if let Some(rest) = line.strip_prefix("diff --git ") {
                in_hunk = false;
                let path = rest.rsplit_once(" b/").map(|(_, b)| b).unwrap_or(rest);
                files.push(PatchFile {
                    path: path.to_owned(),
                    ..Default::default()
                });
                continue;
            }
            let Some(file) = files.last_mut() else {
                continue;
            };
            if let Some(header) = line.strip_prefix("@@ ") {
                if let Some((o, n)) = parse_hunk_header(header) {
                    in_hunk = true;
                    old = o;
                    new = n;
                    file.hunk_starts
                        .push((file.new_numbers.len() as u32, file.old_numbers.len() as u32));
                }
                continue;
            }
            if !in_hunk {
                continue;
            }
            match line.as_bytes().first() {
                Some(b'+') => {
                    file.new_numbers.push(new);
                    new += 1;
                }
                Some(b'-') => {
                    file.old_numbers.push(old);
                    old += 1;
                }
                Some(b'\\') => {}
                Some(b' ') | None => {
                    file.old_numbers.push(old);
                    file.new_numbers.push(new);
                    old += 1;
                    new += 1;
                }
                Some(_) => in_hunk = false,
            }
        }
        let mut old_at = Vec::with_capacity(files.len());
        let mut new_at = Vec::with_capacity(files.len());
        let (mut o, mut n) = (0u32, 0u32);
        for file in &files {
            old_at.push(o);
            new_at.push(n);
            o += file.old_numbers.len() as u32;
            n += file.new_numbers.len() as u32;
        }
        Self {
            files,
            old_at,
            new_at,
        }
    }

    /// Lines in the patch text, as `wc -l` counts them.
    pub fn line_count(patch: &str) -> usize {
        patch.lines().count()
    }

    /// The file of a joined line and that line's number in the file (from 1).
    pub fn locate(&self, side: DiffSide, line: u32) -> Option<(usize, u32)> {
        let (at, numbers): (&[u32], Numbers) = match side {
            DiffSide::Old => (&self.old_at, |f| &f.old_numbers),
            _ => (&self.new_at, |f| &f.new_numbers),
        };
        let file = at.partition_point(|&start| start <= line).checked_sub(1)?;
        let mut file = file;
        // Files without lines (binary, renames) share their start with the next file.
        while numbers(&self.files[file]).is_empty() {
            file = file.checked_sub(1)?;
        }
        let number = *numbers(&self.files[file]).get((line - at[file]) as usize)?;
        Some((file, number))
    }

    /// The joined line of a file's excerpt line.
    pub fn joined(&self, side: DiffSide, file: usize, line: u32) -> u32 {
        match side {
            DiffSide::Old => self.old_at[file] + line,
            _ => self.new_at[file] + line,
        }
    }

    /// `path:L12-L14`, or `path:L12` for one line.
    pub fn describe(&self, side: DiffSide, start: u32, end: u32) -> String {
        let prefix = if side == DiffSide::Old { "old " } else { "" };
        match (self.locate(side, start), self.locate(side, end)) {
            (Some((file, a)), Some((_, b))) if a != b => {
                format!("{prefix}{}:L{a}-L{b}", self.files[file].path)
            }
            (Some((file, a)), _) => format!("{prefix}{}:L{a}", self.files[file].path),
            _ => format!("{prefix}line {}", start + 1),
        }
    }
}

fn parse_hunk_header(header: &str) -> Option<(u32, u32)> {
    let mut parts = header.split(' ');
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let start = |s: &str| s.split(',').next()?.parse::<u32>().ok();
    Some((start(old)?, start(new)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -10,3 +10,4 @@ fn a()
 one
-two
+deux
+trois
 four
diff --git a/logo.png b/logo.png
Binary files a/logo.png and b/logo.png differ
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1,2 +1,1 @@
-gone
 kept
";

    #[test]
    fn joins_files_and_keeps_original_numbers() {
        let index = PatchIndex::parse(PATCH);
        assert_eq!(index.files.len(), 3);
        assert_eq!(index.files[0].new_numbers, vec![10, 11, 12, 13]);
        assert_eq!(index.files[0].old_numbers, vec![10, 11, 12]);
        assert_eq!(index.locate(DiffSide::New, 4), Some((2, 1)));
        assert_eq!(index.locate(DiffSide::Old, 4), Some((2, 2)));
        assert_eq!(index.joined(DiffSide::New, 2, 0), 4);
        assert_eq!(index.describe(DiffSide::New, 1, 2), "a.rs:L11-L12");
        assert_eq!(index.describe(DiffSide::Old, 3, 3), "old b.rs:L1");
    }

    #[test]
    fn reads_the_shared_fixture() {
        let index = PatchIndex::parse(crate::FIXTURE);
        assert_eq!(index.files.len(), 32);
        assert!(PatchIndex::line_count(crate::FIXTURE) > 4000);
    }
}
