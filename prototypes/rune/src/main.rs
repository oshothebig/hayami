use std::{path::PathBuf, sync::OnceLock};

use rune::{App, AppSpec, Cx, El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, row, text};
use rune_kit::diff::{Annotation, DiffState, DiffView, LineRange, LineRef, Side, parse};
use rune_kit::{Button, Kit, TextArea};

mod model;
#[cfg(test)]
mod tests;
use model::Comments;

struct Config {
    patch: String,
    repeat: usize,
}
static CONFIG: OnceLock<Config> = OnceLock::new();

struct Review {
    diff: DiffState,
    comments: Comments,
    draft: TextEdit,
    current_hunk: Option<usize>,
    nav_file: usize,
    pending_file: Option<usize>,
    pending_line: Option<LineRange>,
    pending_sidebar: Option<usize>,
}

fn load(patch: &str, repeat: usize) -> Result<Review, String> {
    let original = parse(patch);
    if original.is_empty() {
        return Err("The patch contains no unified file diffs".into());
    }
    let mut files = Vec::new();
    for copy in 0..repeat {
        for source in &original {
            let mut file = source.clone();
            if repeat > 1 {
                for path in [&mut file.old_path, &mut file.new_path]
                    .into_iter()
                    .flatten()
                {
                    let extension = std::path::Path::new(path)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_owned();
                    let suffix = if extension.is_empty() {
                        String::new()
                    } else {
                        format!(".{extension}")
                    };
                    *path = format!(
                        "{}.repeat-{}{suffix}",
                        path.trim_end_matches(&suffix),
                        copy + 1
                    );
                }
            }
            files.push(file);
        }
    }
    Ok(Review {
        diff: DiffState::new(files),
        comments: Comments::default(),
        draft: TextEdit::new(""),
        current_hunk: None,
        nav_file: 0,
        pending_file: None,
        pending_line: None,
        pending_sidebar: None,
    })
}

fn init() -> Review {
    let config = CONFIG.get().expect("CLI configuration initialized");
    load(&config.patch, config.repeat).expect("patch validated before launch")
}

impl Review {
    fn first_line(&self) -> Option<LineRange> {
        let file = self.diff.visible_file();
        let hunks = &self.diff.files().get(file)?.hunks;
        let hunk = hunks.get(
            self.current_hunk
                .unwrap_or(0)
                .min(hunks.len().saturating_sub(1)),
        )?;
        let line = hunk.lines.first()?;
        let at = line
            .new
            .map(|n| LineRef::new(Side::New, n))
            .or_else(|| line.old.map(|n| LineRef::new(Side::Old, n)))?;
        Some(LineRange::line(file, at))
    }

    fn sync_annotations(&mut self) {
        let mut notes: Vec<_> = self
            .comments
            .posted
            .iter()
            .map(|c| Annotation::new(c.range.file, c.range.end, c.id.to_string()))
            .collect();
        if let Some(range) = self.comments.composing {
            notes.push(Annotation::new(range.file, range.end, "composer"));
        }
        self.diff.set_annotations(notes);
    }

    fn open(&mut self, range: LineRange) {
        if self.comments.composing.is_some() {
            return;
        }
        self.comments.composing = Some(range);
        self.draft = TextEdit::new("");
        self.draft.focused = true;
        self.diff.set_selection(Some(range));
        self.sync_annotations();
        self.pending_line = Some(range);
    }

    fn cancel(&mut self, tx: &mut Tx) {
        self.comments.composing = None;
        self.draft = TextEdit::new("");
        self.sync_annotations();
        tx.blur();
    }

    fn submit(&mut self, tx: &mut Tx) {
        if self.comments.post(&self.draft.text) {
            self.cancel(tx);
        }
    }
}

fn annotation(s: &Review, a: &Annotation, k: &Kit) -> El {
    let mut card = col()
        .w_full()
        .padding(14.0)
        .gap(8.0)
        .radius(8.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_weak);
    if a.id == "composer" {
        let range = s
            .comments
            .composing
            .expect("composer annotation has a range");
        card = card
            .child(
                text(format!(
                    "New comment · lines {}–{}",
                    range.start.line, range.end.line
                ))
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
            )
            .child(
                TextArea::new(&s.draft)
                    .label("Review comment")
                    .placeholder("Leave a review comment…")
                    .width(None)
                    .rows(3)
                    .on_edit(|s: &mut Review| &mut s.draft)
                    .on_submit(|s: &mut Review, tx| s.submit(tx))
                    .on_key(|s: &mut Review, ev, tx| {
                        if ev.key == Key::Escape {
                            s.cancel(tx);
                            true
                        } else if ev.key == Key::Enter && (ev.mods.ctrl || ev.mods.cmd) {
                            s.submit(tx);
                            true
                        } else {
                            false
                        }
                    })
                    .build(k),
            )
            .child(
                row()
                    .gap(8.0)
                    .child(
                        Button::new("Post · Ctrl/⌘ Enter")
                            .on_press_tx(|s: &mut Review, tx| s.submit(tx))
                            .build(k),
                    )
                    .child(
                        Button::new("Cancel · Esc")
                            .on_press_tx(|s: &mut Review, tx| s.cancel(tx))
                            .build(k),
                    ),
            );
    } else if let Some(comment) = s.comments.posted.iter().find(|c| c.id.to_string() == a.id) {
        card = card
            .child(
                text(format!(
                    "You · lines {}–{}",
                    comment.range.start.line, comment.range.end.line
                ))
                .font(k.ty.ui_medium)
                .color(k.c.text_muted),
            )
            .child(
                text(&comment.body)
                    .font(k.ty.body)
                    .color(k.c.text_base)
                    .wrap(),
            );
    }
    col().w_full().padding(12.0).child(card)
}

fn view(s: &Review, cx: &mut Cx<Review>) -> El {
    let k = Kit::neutral(cx.system_dark());
    let files = s.diff.files();
    let selected = s.diff.visible_file();
    let list = col()
        .gap(3.0)
        .padding(8.0)
        .children(files.iter().enumerate().map(|(i, f)| {
            col()
                .key(format!("file-{i}"))
                .w_full()
                .padding(10.0)
                .gap(5.0)
                .radius(6.0)
                .fill(if i == selected {
                    k.c.layer_02
                } else {
                    k.c.bg_base
                })
                .child(text(f.path()).font(k.ty.ui).color(k.c.text_base).wrap())
                .child(
                    text(format!("+{}  −{}", f.additions(), f.deletions()))
                        .font(k.ty.ui)
                        .color(k.c.text_muted),
                )
                .role("button")
                .label(format!("Open {}", f.path()))
                .on_press_tx(move |s: &mut Review, tx| {
                    s.current_hunk = None;
                    s.nav_file = i;
                    s.diff.scroll_to_file(i, tx);
                })
        }));
    let sidebar = col()
        .w(290.0)
        .h_full()
        .min_h(0.0)
        .fill(k.c.bg_base)
        .child(
            text(format!("CHANGED FILES · {}", files.len()))
                .font(k.ty.ui_medium)
                .color(k.c.text_muted)
                .padding(18.0),
        )
        .child(
            col()
                .key("files")
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list),
        );
    let ka = k.clone();
    let diff = DiffView::new(|s: &Review| &s.diff, |s: &mut Review| &mut s.diff)
        .annotation(move |s, a| annotation(s, a, &ka))
        .on_comment(|s, range, _| s.open(range))
        .on_visible_file(|s, file| {
            s.pending_sidebar = Some(file);
            if file != s.nav_file {
                s.nav_file = file;
                s.current_hunk = None;
            }
        })
        .build(s, &k);
    let body = row()
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(sidebar)
        .child(col().grow(1.0).min_w(0.0).h_full().child(diff));
    col().w_full().h_full().fill(k.c.bg_base)
        .child(row().h(62.0).px(20.0).gap(16.0).items_center().fill(k.c.layer_01)
            .child(text("hayami").font(k.ty.ui_medium).color(k.c.text_base))
            .child(text("ripgrep #2610 · add hyperlink support").font(k.ty.ui).color(k.c.text_muted))
            .child(row().grow(1.0))
            .child(text("LOCAL REVIEW / RUNE").font(k.ty.ui).color(k.c.text_faint)))
        .child(body)
        .child(text("j / k  hunks     ] / [  files     c / Enter  comment     Ctrl/⌘ Enter  post     Esc  cancel   ·   In-memory only")
            .font(k.ty.ui).color(k.c.text_muted).padding(12.0))
}

fn on_key(s: &mut Review, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.comments.composing.is_some() || tx.focused().is_some() {
        return false;
    }
    if (ev.key == Key::Enter
        || matches!(&ev.key, Key::Text(t) if t == "c" && !ev.mods.ctrl && !ev.mods.cmd && !ev.mods.alt))
        && let Some(range) = s.diff.selection().or_else(|| s.first_line())
    {
        s.open(range);
        return true;
    }
    let before = s.diff.visible_file();
    let taken = s.diff.key(ev, tx);
    if taken && let Key::Text(t) = &ev.key {
        let file = s.diff.visible_file();
        match t.as_str() {
            "j" => {
                s.current_hunk = Some(if file != before {
                    0
                } else {
                    s.current_hunk.map_or(0, |h| h + 1)
                })
            }
            "k" => {
                s.current_hunk = Some(if file != before {
                    s.diff.files()[file].hunks.len().saturating_sub(1)
                } else {
                    s.current_hunk.unwrap_or(0).saturating_sub(1)
                })
            }
            "]" | "[" => s.current_hunk = None,
            _ => {}
        }
        s.nav_file = file;
    }
    taken
}

fn poll(s: &mut Review, _: f64, tx: &mut Tx) -> bool {
    if let Some(file) = s.pending_file.take() {
        s.nav_file = file;
        s.current_hunk = None;
        s.diff.scroll_to_file(file, tx);
        return true;
    }
    if let Some(range) = s.pending_line.take() {
        s.diff.scroll_to_line(range.file, range.start, tx);
        return true;
    }
    if let Some(file) = s.pending_sidebar.take() {
        tx.scroll_into_view(format!("file-{file}"));
        return true;
    }
    false
}

fn composer_scene(s: &mut Review) {
    if let Some(mut range) = s.first_line() {
        range.end.line += 2;
        s.diff.set_selection(Some(range));
        s.open(range);
        s.draft = TextEdit::new("日本語のコメント");
        s.draft.focused = true;
    }
}

fn spec() -> AppSpec<Review> {
    AppSpec {
        scenes: &[
            ("default", |_| {}),
            ("file-selected", |s| {
                s.pending_file = Some(4.min(s.diff.files().len() - 1))
            }),
            ("composer", composer_scene),
            ("commented", |s| {
                composer_scene(s);
                s.comments.post(&s.draft.text);
                s.sync_annotations();
            }),
            ("scrolled", |s| {
                s.pending_file = Some(s.diff.files().len() / 2)
            }),
            ("folded", |s| {
                s.diff.set_collapsed(0, true);
                s.diff.set_collapsed(1, true);
            }),
        ],
        on_key: Some(on_key),
        poll: Some(poll),
        families: ("Inter", "ui-monospace, Inter"),
        ..AppSpec::new("Hayami · Rune review", (1400.0, 900.0), init, view)
    }
}

fn config() -> Result<Config, String> {
    let mut args = std::env::args().skip(1).peekable();
    let mut path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ripgrep-2610.diff");
    let mut repeat = 1;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repeat" => {
                repeat = args
                    .next()
                    .ok_or("--repeat requires a positive integer")?
                    .parse::<usize>()
                    .map_err(|_| "Invalid --repeat")?;
                if repeat == 0 {
                    return Err("--repeat must be greater than zero".into());
                }
            }
            "--snap" | "--tree" | "--size" | "--theme" | "--scene" | "--at" | "--scale"
            | "--frames" | "--script" | "--out" | "--trace" | "--every" | "--start" | "--fps"
            | "--duration" | "--video" | "--strip" | "--cols" | "--crop" => {
                args.next();
            }
            arg if arg.starts_with('-') => {}
            _ => path = PathBuf::from(arg),
        }
    }
    let patch = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    load(&patch, repeat)?;
    Ok(Config { patch, repeat })
}

fn main() {
    match config() {
        Ok(config) => {
            let _ = CONFIG.set(config);
            App::new(spec()).modifier_keys().run();
        }
        Err(error) => {
            eprintln!("hayami-rune: {error}");
            std::process::exit(2);
        }
    }
}
