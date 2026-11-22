use gpui_component::diff::{DiffAnnotation, DiffLinePosition, DiffLineRange};

pub struct Comment {
    pub range: DiffLineRange,
    pub text: String,
}

pub fn clicked_range(
    origin: &mut Option<DiffLinePosition>,
    position: &DiffLinePosition,
    extend: bool,
) -> DiffLineRange {
    let anchor = origin
        .as_ref()
        .filter(|anchor| extend && anchor.path() == position.path())
        .cloned()
        .unwrap_or_else(|| position.clone());
    *origin = Some(anchor.clone());
    DiffLineRange::new(
        position.path().clone(),
        anchor.side(),
        anchor.line(),
        position.line(),
    )
    .with_end_side(position.side())
}

impl Comment {
    pub fn submit(draft: &mut Option<DiffLineRange>, text: &str) -> Option<Self> {
        if text.trim().is_empty() {
            return None;
        }
        Some(Self {
            range: draft.take()?,
            text: text.trim().to_owned(),
        })
    }
}

pub fn label(range: &DiffLineRange) -> String {
    format!(
        "{} · {:?} L{}–{:?} L{}",
        range.path(),
        range.side(),
        range.start(),
        range.end_side(),
        range.end()
    )
}

pub fn annotation(id: usize, range: &DiffLineRange, collapsed: bool) -> DiffAnnotation {
    if collapsed {
        DiffAnnotation::file(("review-note", id), range.path().clone())
    } else {
        DiffAnnotation::line(
            ("review-note", id),
            DiffLinePosition::new(range.path().clone(), range.end_side(), range.end()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::diff::{DiffFile, DiffSide};

    fn range() -> DiffLineRange {
        DiffLineRange::new("a.rs", DiffSide::Modified, 12, 15)
    }

    #[test]
    fn code_clicks_keep_origin_across_forward_reverse_and_cross_side_ranges() {
        let point = |side, line| DiffLinePosition::new("a.rs", side, line);
        let mut origin = None;
        assert_eq!(
            clicked_range(&mut origin, &point(DiffSide::Modified, 12), false).start(),
            12
        );
        assert_eq!(
            clicked_range(&mut origin, &point(DiffSide::Modified, 15), true),
            range()
        );
        let reversed = clicked_range(&mut origin, &point(DiffSide::Modified, 10), true);
        assert_eq!((reversed.start(), reversed.end()), (12, 10));
        let cross_side = clicked_range(&mut origin, &point(DiffSide::Original, 9), true);
        assert_eq!(
            (cross_side.side(), cross_side.end_side()),
            (DiffSide::Modified, DiffSide::Original)
        );
        let other = DiffLinePosition::new("other.rs", DiffSide::Modified, 1);
        assert_eq!(
            clicked_range(&mut origin, &other, true),
            DiffLineRange::new("other.rs", DiffSide::Modified, 1, 1)
        );
        assert_eq!(clicked_range(&mut None, &other, true).start(), 1);
    }

    #[test]
    fn submits_range_and_keeps_anchor_when_folded() {
        let mut draft = Some(range());
        let comment = Comment::submit(&mut draft, " review\ntext ").unwrap();
        assert!(draft.is_none());
        assert_eq!(comment.text, "review\ntext");
        let expanded = annotation(0, &comment.range, false);
        let folded = annotation(0, &comment.range, true);
        assert_eq!(expanded.id(), folded.id());
        assert_eq!(expanded.position().unwrap().line(), 15);
        assert!(folded.position().is_none());
        assert_eq!(expanded.path(), folded.path());
        assert_eq!(comment.range, range());
    }

    #[test]
    fn rejects_empty_text_and_missing_draft() {
        let mut draft = Some(range());
        assert!(Comment::submit(&mut draft, " \n ").is_none());
        assert_eq!(draft, Some(range()));
        assert!(Comment::submit(&mut None, "text").is_none());
    }

    #[test]
    fn a_draft_is_consumed_once_across_repeated_submit_dispatches() {
        for submissions in [
            vec!["日本語", "日本語"],
            vec!["日本語", "日本語", "日本語"],
            vec!["", " \n ", "日本語", "日本語"],
        ] {
            let anchor = DiffLineRange::new("tests/regression.rs", DiffSide::Modified, 383, 383);
            let mut draft = Some(anchor.clone());
            let comments = submissions
                .into_iter()
                .filter_map(|text| Comment::submit(&mut draft, text))
                .collect::<Vec<_>>();
            assert_eq!(comments.len(), 1);
            assert_eq!(comments[0].range, anchor);
            assert_eq!(comments[0].text, "日本語");
            assert!(draft.is_none());
            assert!(Comment::submit(&mut draft, "日本語").is_none());

            draft = Some(anchor);
            assert!(Comment::submit(&mut draft, "日本語").is_some());
            assert!(Comment::submit(&mut draft, "日本語").is_none());
        }
    }

    #[test]
    fn component_parser_reads_shared_fixture_and_rejects_malformed_patch() {
        let files = DiffFile::parse(include_str!("../../../fixtures/ripgrep-2610.diff")).unwrap();
        assert_eq!(files.len(), 32);
        assert_eq!(files[0].path().as_str(), ".github/workflows/ci.yml");
        assert!(files.iter().any(|file| file.language().as_str() == "rust"));
        assert!(DiffFile::parse("").unwrap().is_empty());
        assert!(DiffFile::parse("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ broken @@\n").is_err());
    }

    #[test]
    fn component_delete_and_rename_share_path_and_annotation_target() {
        let patch = "diff --git a/a.rs b/a.rs\ndeleted file mode 100644\n--- a/a.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\ndiff --git a/b.rs b/a.rs\nsimilarity index 50%\nrename from b.rs\nrename to a.rs\n--- a/b.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-before\n+after\n";
        let files = DiffFile::parse(patch).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path(), files[1].path());
        assert_ne!(files[0].original_path(), files[1].original_path());
        let deletion = DiffLineRange::new(files[0].path().clone(), DiffSide::Original, 1, 1);
        let rename = DiffLineRange::new(files[1].path().clone(), DiffSide::Original, 1, 1);
        assert_eq!(
            annotation(0, &deletion, false).position(),
            annotation(1, &rename, false).position()
        );
    }
}
