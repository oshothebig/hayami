use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct FileDiff {
    pub old_path: String,
    pub new_path: String,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<Line>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Add,
    Delete,
    Context,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    pub text: String,
    pub old_number: Option<usize>,
    pub new_number: Option<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

fn range(value: &str, prefix: char) -> Result<(usize, usize), ParseError> {
    let value = value
        .strip_prefix(prefix)
        .ok_or_else(|| ParseError("Invalid hunk range".into()))?;
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    Ok((
        start
            .parse()
            .map_err(|_| ParseError("Invalid hunk start".into()))?,
        count
            .parse()
            .map_err(|_| ParseError("Invalid hunk count".into()))?,
    ))
}

fn validate(hunk: &Hunk) -> Result<(), ParseError> {
    let old = hunk.lines.iter().filter(|l| l.old_number.is_some()).count();
    let new = hunk.lines.iter().filter(|l| l.new_number.is_some()).count();
    if (old, new) != (hunk.old_count, hunk.new_count) {
        return Err(ParseError(format!(
            "Hunk line count mismatch: {}",
            hunk.header
        )));
    }
    Ok(())
}

pub fn parse(input: &str) -> Result<Vec<FileDiff>, ParseError> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old, mut new) = (0, 0);
    for text in input.lines() {
        if let Some(paths) = text.strip_prefix("diff --git ") {
            let (a, b) = paths
                .split_once(" b/")
                .ok_or_else(|| ParseError("Unsupported git file header".into()))?;
            files.push(FileDiff {
                old_path: a.strip_prefix("a/").unwrap_or(a).into(),
                new_path: b.into(),
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            if text.trim().is_empty() {
                continue;
            }
            return Err(ParseError("Expected a git diff file header".into()));
        };
        if text.starts_with("@@ ") {
            let mut parts = text.split_whitespace();
            parts.next();
            let (old_start, old_count) = range(parts.next().unwrap_or(""), '-')?;
            let (new_start, new_count) = range(parts.next().unwrap_or(""), '+')?;
            if parts.next() != Some("@@") {
                return Err(ParseError("Invalid hunk header".into()));
            }
            old = old_start;
            new = new_start;
            file.hunks.push(Hunk {
                header: text.into(),
                old_start,
                old_count,
                new_start,
                new_count,
                lines: Vec::new(),
            });
        } else if let Some(hunk) = file.hunks.last_mut() {
            if text == "\\ No newline at end of file" {
                continue;
            }
            let kind = match text.as_bytes().first() {
                Some(b'+') => LineKind::Add,
                Some(b'-') => LineKind::Delete,
                Some(b' ') => LineKind::Context,
                _ => return Err(ParseError("Invalid hunk line".into())),
            };
            let old_number = (kind != LineKind::Add).then_some(old);
            let new_number = (kind != LineKind::Delete).then_some(new);
            old += usize::from(old_number.is_some());
            new += usize::from(new_number.is_some());
            hunk.lines.push(Line {
                kind,
                text: text[1..].into(),
                old_number,
                new_number,
            });
        } else if let Some(path) = text.strip_prefix("--- ") {
            file.old_path = path.strip_prefix("a/").unwrap_or(path).into();
        } else if let Some(path) = text.strip_prefix("+++ ") {
            file.new_path = path.strip_prefix("b/").unwrap_or(path).into();
        }
    }
    for file in &files {
        for hunk in &file.hunks {
            validate(hunk)?;
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_and_kinds() {
        let files = parse("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -2,2 +2,3 @@ function\n same\n-old\n+new\n+extra\n\\ No newline at end of file\n").unwrap();
        let lines = &files[0].hunks[0].lines;
        assert_eq!(
            (lines[0].old_number, lines[0].new_number),
            (Some(2), Some(2))
        );
        assert_eq!(lines[1].kind, LineKind::Delete);
        assert_eq!((lines[1].old_number, lines[1].new_number), (Some(3), None));
        assert_eq!((lines[3].old_number, lines[3].new_number), (None, Some(4)));
    }

    #[test]
    fn handles_empty_added_deleted_and_metadata_only_files() {
        assert!(parse("").unwrap().is_empty());
        let files = parse("diff --git a/a b/a\n--- /dev/null\n+++ b/a\n@@ -0,0 +1 @@\n+added\ndiff --git a/b b/b\n--- a/b\n+++ /dev/null\n@@ -1 +0,0 @@\n-deleted\ndiff --git a/c b/d\nsimilarity index 100%\nrename from c\nrename to d\n").unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].old_path, "/dev/null");
        assert_eq!(files[1].new_path, "/dev/null");
        assert!(files[2].hunks.is_empty());
        assert_eq!(files[2].new_path, "d");
    }

    #[test]
    fn rejects_malformed_and_truncated_input() {
        for input in [
            "not a diff",
            "diff --git a/a b/a\n@@ -x +1 @@\n+a",
            "diff --git a/a b/a\n@@ -1 +1 @@\n+a",
            "diff --git a/a b/a\n@@ -0,0 +1 @@\nbad",
        ] {
            assert!(parse(input).is_err(), "{input}");
        }
    }

    #[test]
    fn parses_shared_fixture() {
        let files = parse(include_str!("../../../fixtures/ripgrep-2610.diff")).unwrap();
        assert_eq!(files.len(), 32);
        assert!(files.iter().map(|f| f.hunks.len()).sum::<usize>() > 32);
    }
}
