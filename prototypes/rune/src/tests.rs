use super::*;
use rune::{Env, Ime, Mods, Runtime};

const PATCH: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n fn main() {\n-old\n+new\n@@ -100,2 +100,2 @@\n fn second() {\n-old\n+new\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-old\n+new\n";

fn runtime() -> Runtime<Review> {
    let old: String = (1..=90).map(|n| format!("line {n}\n")).collect();
    let new = old
        .replace("line 10\n", "changed 10\n")
        .replace("line 60\n", "changed 60\n");
    let patch: String = ["a.rs", "b.rs"]
        .iter()
        .map(|path| {
            rune_kit::diff::FileDiff::from_texts_with_context(*path, &old, &new, 10).patch()
        })
        .collect();
    let mut rt = Runtime::new(
        load(&patch, 1).unwrap(),
        view,
        (1400.0, 300.0),
        Env::new(true, 1.0),
    );
    rt.on_key = Some(on_key);
    rt.poll = Some(poll);
    rt.frame(0.0);
    rt
}

fn key(rt: &mut Runtime<Review>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn comments_require_a_range_and_nonempty_body_and_allow_multiple() {
    let mut comments = Comments::default();
    assert!(!comments.post("orphan"));
    let range = LineRange::line(0, LineRef::new(Side::New, 1));
    comments.composing = Some(range);
    assert!(!comments.post(" \n "));
    assert_eq!(comments.composing, Some(range));
    assert!(comments.post(" 日本語のコメント "));
    comments.composing = Some(range);
    assert!(comments.post("second"));
    assert_eq!(comments.posted.len(), 2);
    assert_eq!(comments.posted[0].body, "日本語のコメント");
    assert_ne!(comments.posted[0].id, comments.posted[1].id);
}

#[test]
fn parsing_handles_empty_invalid_and_repeated_patches() {
    assert!(load("", 1).is_err());
    assert!(load("not a patch", 1).is_err());
    let review = load(PATCH, 3).unwrap();
    assert_eq!(review.diff.files().len(), 6);
    assert_eq!(review.diff.files()[0].path(), "a.repeat-1.rs");
    assert_eq!(review.diff.files()[4].path(), "a.repeat-3.rs");
}

#[test]
fn repeated_paths_strip_only_the_final_extension() {
    for (path, stem, extension) in [
        ("foo.rs.rs", "foo.rs", ".rs"),
        ("dir.rs/foo", "dir.rs/foo", ""),
        (".gitignore", ".gitignore", ""),
    ] {
        let patch = PATCH.replace("a.rs", path);
        let review = load(&patch, 2).unwrap();
        for copy in 0..2 {
            let expected = format!("{stem}.repeat-{}{extension}", copy + 1);
            let file = &review.diff.files()[copy * 2];
            assert_eq!(file.old_path.as_deref(), Some(expected.as_str()));
            assert_eq!(file.new_path.as_deref(), Some(expected.as_str()));
        }
    }
}

#[test]
fn selection_is_separate_from_composing_and_preserves_an_existing_draft() {
    let mut rt = Runtime::new(
        load(PATCH, 1).unwrap(),
        view,
        (1400.0, 900.0),
        Env::new(true, 1.0),
    );
    rt.on_key = Some(on_key);
    rt.poll = Some(poll);
    rt.frame(0.0);
    let click_number = |rt: &mut Runtime<Review>, index| {
        let r = rt
            .elements()
            .into_iter()
            .filter(|e| e.key.as_deref() == Some("nn"))
            .nth(index)
            .unwrap()
            .rect;
        let (x, y) = (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
    };
    click_number(&mut rt, 0);
    let anchor = rt.state.diff.selection().unwrap();
    assert!(rt.state.comments.composing.is_none());
    assert!(
        !rt.elements()
            .iter()
            .any(|e| e.label.as_deref() == Some("Review comment"))
    );
    key(&mut rt, Key::Text("c".into()), Mods::default());
    assert_eq!(rt.state.comments.composing, Some(anchor));
    rt.ime(Ime::Commit("Keep this draft".into()));
    rt.frame(0.0);
    click_number(&mut rt, 1);
    let selected = rt.state.diff.selection().unwrap();
    assert_ne!(selected, anchor);
    assert_eq!(rt.state.comments.composing, Some(anchor));
    assert_eq!(rt.state.draft.text, "Keep this draft");
    rt.state.open(selected);
    assert_eq!(rt.state.comments.composing, Some(anchor));
    assert_eq!(rt.state.draft.text, "Keep this draft");
    assert!(rt.state.comments.posted.is_empty());
}

#[test]
fn keyboard_composition_posting_and_cancel_use_real_runtime() {
    let mut rt = runtime();
    key(&mut rt, Key::Text("c".into()), Mods::default());
    assert!(rt.state.comments.composing.is_some());
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.label.as_deref() == Some("Review comment"))
    );
    rt.ime(Ime::Preedit("にほんご".into(), None, Vec::new()));
    rt.frame(0.0);
    assert!(rt.state.draft.text.is_empty());
    rt.ime(Ime::Commit("日本語のコメント".into()));
    rt.frame(0.0);
    key(&mut rt, Key::Text("j".into()), Mods::default());
    assert_eq!(rt.state.draft.text, "日本語のコメントj");
    assert_eq!(rt.state.current_hunk, None);
    key(
        &mut rt,
        Key::Enter,
        Mods {
            ctrl: true,
            ..Mods::default()
        },
    );
    assert_eq!(rt.state.comments.posted.len(), 1);
    assert!(rt.state.comments.composing.is_none());
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("日本語のコメントj"))
    );
    key(&mut rt, Key::Text("c".into()), Mods::default());
    key(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.comments.composing.is_none());
    assert_eq!(rt.state.comments.posted.len(), 1);
}

#[test]
fn file_and_hunk_shortcuts_and_empty_submit() {
    let mut rt = runtime();
    key(&mut rt, Key::Text("j".into()), Mods::default());
    assert_eq!(rt.state.first_line().unwrap().start.line, 1);
    key(&mut rt, Key::Text("j".into()), Mods::default());
    assert_eq!(rt.state.first_line().unwrap().start.line, 50);
    key(&mut rt, Key::Text("k".into()), Mods::default());
    key(&mut rt, Key::Text("]".into()), Mods::default());
    assert_eq!(rt.state.diff.visible_file(), 1);
    key(&mut rt, Key::Text("[".into()), Mods::default());
    assert_eq!(rt.state.diff.visible_file(), 0);
    key(&mut rt, Key::Enter, Mods::default());
    key(
        &mut rt,
        Key::Enter,
        Mods {
            cmd: true,
            ..Mods::default()
        },
    );
    assert!(rt.state.comments.posted.is_empty());
    assert!(rt.state.comments.composing.is_some());
}
