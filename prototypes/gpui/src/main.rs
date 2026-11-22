mod review;

use gpui::{prelude::*, *};
use gpui_component::{
    Theme, ThemeMode,
    diff::{
        Diff, DiffEvent, DiffFile, DiffLinePosition, DiffLineRange, DiffMode, DiffSide, DiffState,
    },
    input::{InputEvent, Textarea, TextareaState},
};
use review::Comment;
use std::time::Instant;

actions!(
    review,
    [
        NextChange,
        PrevChange,
        NextFile,
        PrevFile,
        ToggleFile,
        ToggleContext,
        StartComment,
        SubmitComment,
        CancelComment,
        ExtendDown,
        ExtendUp
    ]
);

enum Jump {
    Line(DiffLinePosition),
    File(SharedString),
}

struct Review {
    diff: Entity<DiffState>,
    sidebar: UniformListScrollHandle,
    current_file: usize,
    selection_origin: Option<DiffLinePosition>,
    comments: Vec<Comment>,
    draft: Option<DiffLineRange>,
    input: Entity<TextareaState>,
    _subscriptions: Vec<Subscription>,
    focus: FocusHandle,
    compact_context: bool,
    started: Instant,
    last_frame: Instant,
    first_frame: bool,
    bench: Option<Vec<Jump>>,
    bench_index: usize,
    frame_times: Vec<f64>,
}

fn bench_targets(state: &mut DiffState, cx: &mut Context<DiffState>) -> Vec<Jump> {
    let files = state.files().to_vec();
    let mut targets = Vec::new();
    for file in &files {
        for side in [DiffSide::Original, DiffSide::Modified] {
            state.set_selected_lines(
                Some(DiffLineRange::new(file.path().clone(), side, 1, usize::MAX)),
                cx,
            );
            let Some(bounds) = state.selected_lines() else {
                continue;
            };
            for start in (bounds.start()..=bounds.end()).step_by(20) {
                state.set_selected_lines(
                    Some(DiffLineRange::new(
                        file.path().clone(),
                        side,
                        start,
                        start.saturating_add(19).min(bounds.end()),
                    )),
                    cx,
                );
                if let Some(range) = state.selected_lines() {
                    targets.push(Jump::Line(DiffLinePosition::new(
                        range.path().clone(),
                        side,
                        range.start(),
                    )));
                }
            }
        }
    }
    state.set_selected_lines(None, cx);
    targets.extend(files.iter().map(|file| Jump::File(file.path().clone())));
    targets
}

impl Review {
    fn follow(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(index) = self
            .diff
            .read(cx)
            .files()
            .iter()
            .position(|file| file.path().as_str() == path)
        {
            self.current_file = index;
            self.sidebar.scroll_to_item(index, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    fn file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let files = self.diff.read(cx).files();
        let Some(file) = files.get(index.min(files.len().saturating_sub(1))) else {
            return;
        };
        let path = file.path().clone();
        self.selection_origin = None;
        self.diff.update(cx, |state, cx| {
            state.set_selected_lines(None, cx);
            state.scroll_to_file(&path, cx);
        });
        self.follow(&path, cx);
        self.diff.focus_handle(cx).focus(window, cx);
    }

    fn fold(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.diff.read(cx).files().get(self.current_file) else {
            return;
        };
        let path = file.path().clone();
        let collapsed = self.diff.read(cx).is_file_collapsed(&path);
        self.diff.update(cx, |state, cx| {
            state.set_file_collapsed(&path, !collapsed, cx)
        });
        cx.notify();
    }

    fn context(&mut self, cx: &mut Context<Self>) {
        self.compact_context = !self.compact_context;
        self.diff.update(cx, |state, cx| {
            if self.compact_context {
                state.collapse_unchanged(cx);
            } else {
                state.expand_unchanged(cx);
            }
        });
        cx.notify();
    }

    fn extend(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.diff.focus_handle(cx).focus(window, cx);
        let name = if down {
            "diff::ExtendDown"
        } else {
            "diff::ExtendUp"
        };
        match cx.build_action(name, None) {
            Ok(action) => window.dispatch_action(action, cx),
            Err(error) => eprintln!("Cannot dispatch {name}: {error}"),
        }
    }

    fn start_comment(
        &mut self,
        range: Option<DiffLineRange>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft.is_some() {
            self.input.update(cx, |input, cx| input.focus(window, cx));
            return;
        }
        let Some(range) = range.or_else(|| self.diff.read(cx).selected_lines()) else {
            return;
        };
        let position = DiffLinePosition::new(range.path().clone(), range.end_side(), range.end());
        self.follow(range.path(), cx);
        self.diff
            .update(cx, |state, cx| state.scroll_to_line(position, cx));
        self.draft = Some(range);
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        if let Some(comment) = Comment::submit(&mut self.draft, &text) {
            self.comments.push(comment);
            self.diff.focus_handle(cx).focus(window, cx);
            cx.notify();
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = None;
        self.diff.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn tick(entity: WeakEntity<Self>, window: &mut Window, cx: &mut App) {
        let _ = entity.clone().update(cx, |this, cx| {
            let now = Instant::now();
            if !this.first_frame {
                eprintln!("FIRST_FRAME_PAINTED_MS={:.2}", this.started.elapsed().as_secs_f64() * 1000.);
                this.first_frame = true;
            } else if this.bench.is_some() {
                let elapsed = now.duration_since(this.last_frame).as_secs_f64() * 1000.;
                this.frame_times.push(elapsed);
                eprintln!("BENCH_FRAME={} MS={elapsed:.3}", this.frame_times.len());
            }
            this.last_frame = now;
            if let Some(targets) = &this.bench {
                if let Some(target) = targets.get(this.bench_index) {
                    let path = match target {
                        Jump::Line(position) => {
                            this.diff.update(cx, |state, cx| state.scroll_to_line(position.clone(), cx));
                            position.path().clone()
                        }
                        Jump::File(path) => {
                            this.diff.update(cx, |state, cx| state.scroll_to_file(path, cx));
                            path.clone()
                        }
                    };
                    this.bench_index += 1;
                    this.follow(&path, cx);
                } else {
                    let mut times = this.frame_times.clone();
                    times.sort_by(f64::total_cmp);
                    if !times.is_empty() {
                        let rss = std::fs::read_to_string("/proc/self/status").unwrap_or_default().lines()
                            .find(|line| line.starts_with("VmHWM:")).unwrap_or("VmHWM: unavailable").to_string();
                        eprintln!("BENCH_SUMMARY frames={} avg_ms={:.3} p95_ms={:.3} max_ms={:.3} peak_rss={rss}",
                            times.len(), times.iter().sum::<f64>() / times.len() as f64,
                            times[((times.len() - 1) as f64 * 0.95).ceil() as usize], times[times.len() - 1]);
                    }
                    cx.quit();
                    return;
                }
            }
            window.on_next_frame(move |window, cx| Self::tick(entity, window, cx));
        });
    }
}

impl Render for Review {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.diff.read(cx);
        let files = state
            .files()
            .iter()
            .map(|file| file.path().clone())
            .collect::<Vec<_>>();
        let mut notes = self
            .comments
            .iter()
            .enumerate()
            .map(|(id, comment)| {
                (
                    review::annotation(
                        id,
                        &comment.range,
                        state.is_file_collapsed(comment.range.path()),
                    ),
                    review::label(&comment.range),
                    Some(comment.text.clone()),
                )
            })
            .collect::<Vec<_>>();
        if let Some(range) = &self.draft {
            notes.push((
                review::annotation(usize::MAX, range, state.is_file_collapsed(range.path())),
                review::label(range),
                None,
            ));
        }
        let annotations = notes
            .iter()
            .map(|(annotation, _, _)| annotation.clone())
            .collect::<Vec<_>>();
        let input = self.input.clone();
        let owner = cx.entity().downgrade();
        let add_owner = owner.clone();
        let click_owner = owner.clone();
        let viewer = Diff::new(&self.diff)
            .line_number(true)
            .syntax_highlight(true)
            .change_background(true)
            .annotations(annotations)
            .render_annotation(move |annotation, _, _| {
                let (_, label, text) = notes
                    .iter()
                    .find(|(note, _, _)| note.id() == annotation.id())
                    .unwrap();
                let content = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_size(px(13.))
                    .child(label.clone());
                if let Some(text) = text {
                    content.child(text.clone()).into_any_element()
                } else {
                    let submit = owner.clone();
                    let cancel = owner.clone();
                    content
                        .child("Enter / Ctrl+Enter post · Shift+Enter newline · Esc cancel")
                        .child(Textarea::new(&input).h(px(100.)))
                        .child(
                            div()
                                .flex()
                                .gap_4()
                                .child(
                                    div()
                                        .id("post-comment")
                                        .px_3()
                                        .bg(rgb(0x346a51))
                                        .cursor_pointer()
                                        .child("Post comment")
                                        .on_click(move |_, window, cx| {
                                            let _ = submit
                                                .update(cx, |this, cx| this.submit(window, cx));
                                        }),
                                )
                                .child(
                                    div()
                                        .id("cancel-comment")
                                        .px_3()
                                        .cursor_pointer()
                                        .child("Cancel")
                                        .on_click(move |_, window, cx| {
                                            let _ = cancel
                                                .update(cx, |this, cx| this.cancel(window, cx));
                                        }),
                                ),
                        )
                        .into_any_element()
                }
            })
            .on_add_annotation(move |range, window, cx| {
                let _ = add_owner.update(cx, |this, cx| {
                    this.start_comment(Some(range.clone()), window, cx)
                });
            })
            .on_line_click(move |position, event, window, cx| {
                let _ = click_owner.update(cx, |this, cx| {
                    let range = review::clicked_range(
                        &mut this.selection_origin,
                        position,
                        event.modifiers().shift,
                    );
                    this.diff
                        .update(cx, |state, cx| state.set_selected_lines(Some(range), cx));
                    this.follow(position.path(), cx);
                    this.diff.focus_handle(cx).focus(window, cx);
                });
            });
        div().size_full().flex().flex_col().bg(rgb(0x171e29)).text_color(rgb(0xdce5f1))
            .track_focus(&self.focus).key_context(if self.draft.is_some() { "CommentReview" } else { "Review" })
            .on_action(cx.listener(|this, _: &NextChange, _, cx| this.diff.update(cx, |state, cx| state.next_change(cx))))
            .on_action(cx.listener(|this, _: &PrevChange, _, cx| this.diff.update(cx, |state, cx| state.previous_change(cx))))
            .on_action(cx.listener(|this, _: &NextFile, window, cx| this.file(this.current_file + 1, window, cx)))
            .on_action(cx.listener(|this, _: &PrevFile, window, cx| this.file(this.current_file.saturating_sub(1), window, cx)))
            .on_action(cx.listener(|this, _: &ToggleFile, _, cx| this.fold(cx)))
            .on_action(cx.listener(|this, _: &ToggleContext, _, cx| this.context(cx)))
            .on_action(cx.listener(|this, _: &ExtendDown, window, cx| this.extend(true, window, cx)))
            .on_action(cx.listener(|this, _: &ExtendUp, window, cx| this.extend(false, window, cx)))
            .on_action(cx.listener(|this, _: &StartComment, window, cx| this.start_comment(None, window, cx)))
            .on_action(cx.listener(|this, _: &SubmitComment, window, cx| this.submit(window, cx)))
            .on_action(cx.listener(|this, _: &CancelComment, window, cx| this.cancel(window, cx)))
            .child(div().h(px(56.)).px_4().flex().items_center().justify_between().bg(rgb(0x232d3e))
                .child(format!("Hayami / ripgrep #2610 · {} files · gpui-component Diff", files.len()))
                .child(div().flex().gap_4()
                    .child(div().id("context").cursor_pointer().child(if self.compact_context { "Expand context" } else { "Compact context" })
                        .on_click(cx.listener(|this, _, _, cx| this.context(cx))))
                    .child(div().id("comment").cursor_pointer().child("Comment [c]")
                        .on_click(cx.listener(|this, _, window, cx| this.start_comment(None, window, cx))))))
            .child(div().h(px(30.)).px_4().text_size(px(12.)).child("j/k change group · n/p file · o/Space fold file · click / Shift+click select · Shift+J/K extend · c comment"))
            .child(div().flex().flex_1().min_h_0()
                .child(div().w(px(340.)).h_full().flex_shrink_0().bg(rgb(0x1e2735)).child(
                    uniform_list("files", files.len(), cx.processor(move |this, range: std::ops::Range<usize>, _, cx| range.map(|index| {
                        div().id(index).h(px(30.)).px_2().text_size(px(12.)).flex().items_center().overflow_hidden().cursor_pointer()
                            .bg(rgb(if index == this.current_file { 0x395374 } else { 0x1e2735 }))
                            .child(files[index].clone()).on_click(cx.listener(move |this, _, window, cx| this.file(index, window, cx)))
                    }).collect())).track_scroll(&self.sidebar).h_full()))
                .child(div().flex_1().min_w_0().h_full().child(viewer)))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    env_logger::init();
    let bench = std::env::args_os().any(|arg| arg == "--bench");
    let path = std::env::args_os()
        .skip(1)
        .find(|arg| arg != "--bench")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/ripgrep-2610.diff")
        });
    let files = DiffFile::parse(&std::fs::read_to_string(path)?)?;
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            cx.bind_keys([
                KeyBinding::new("j", NextChange, Some("Review")),
                KeyBinding::new("k", PrevChange, Some("Review")),
                KeyBinding::new("n", NextFile, Some("Review")),
                KeyBinding::new("p", PrevFile, Some("Review")),
                KeyBinding::new("o", ToggleFile, Some("Review")),
                KeyBinding::new("space", ToggleFile, Some("Review")),
                KeyBinding::new("c", StartComment, Some("Review")),
                KeyBinding::new("shift-j", ExtendDown, Some("Review")),
                KeyBinding::new("shift-k", ExtendUp, Some("Review")),
                KeyBinding::new("ctrl-enter", SubmitComment, Some("CommentReview")),
                KeyBinding::new("escape", CancelComment, Some("CommentReview")),
            ]);
            let bounds = Bounds::centered(None, size(px(1600.), px(1000.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Hayami — component-backed review".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let focus = cx.focus_handle();
                        let diff = cx.new(|cx| {
                            let mut state = DiffState::new(files, cx)
                                .with_mode(DiffMode::Unified)
                                .with_context_lines(Some(1));
                            state.expand_unchanged(cx);
                            state
                        });
                        diff.focus_handle(cx).focus(window, cx);
                        let targets = bench.then(|| diff.update(cx, bench_targets));
                        let input = cx.new(|cx| {
                            TextareaState::new(window, cx)
                                .rows(3)
                                .placeholder("Write a review comment…")
                        });
                        let subscriptions = vec![
                            cx.subscribe_in(
                                &input,
                                window,
                                |this: &mut Review, _, event, window, cx| {
                                    if let InputEvent::PressEnter { shift: false, .. } = event {
                                        this.submit(window, cx);
                                    }
                                },
                            ),
                            cx.subscribe(&diff, |this: &mut Review, _, event: &DiffEvent, cx| {
                                match event {
                                    DiffEvent::SelectionStarted(range) => {
                                        this.selection_origin = Some(DiffLinePosition::new(
                                            range.path().clone(),
                                            range.side(),
                                            range.start(),
                                        ));
                                        this.follow(range.path(), cx);
                                    }
                                    DiffEvent::SelectionEnded(range)
                                    | DiffEvent::SelectionChanged(Some(range)) => {
                                        this.follow(range.path(), cx)
                                    }
                                    DiffEvent::FileCollapsed(path)
                                    | DiffEvent::FileExpanded(path) => this.follow(path, cx),
                                    _ => {}
                                }
                                cx.notify();
                            }),
                        ];
                        window.on_next_frame({
                            let owner = cx.entity().downgrade();
                            move |window, cx| Review::tick(owner, window, cx)
                        });
                        Review {
                            diff,
                            sidebar: UniformListScrollHandle::new(),
                            current_file: 0,
                            selection_origin: None,
                            comments: Vec::new(),
                            draft: None,
                            input,
                            _subscriptions: subscriptions,
                            focus,
                            compact_context: false,
                            started,
                            last_frame: Instant::now(),
                            first_frame: false,
                            bench: targets,
                            bench_index: 0,
                            frame_times: Vec::new(),
                        }
                    })
                },
            )
            .expect("Failed to open review window");
        });
    Ok(())
}
