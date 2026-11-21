mod diff;
mod review;

use diff::LineKind;
use gpui::{prelude::*, *};
use gpui_component::{
    Theme, ThemeMode,
    highlighter::{HighlightTheme, SyntaxHighlighter},
    input::{InputEvent, Rope, Textarea, TextareaState},
};
use review::{Position, ReviewModel, Row, Side, file_key};
use std::{collections::HashMap, time::Instant};

actions!(
    review,
    [
        NextHunk,
        PrevHunk,
        NextFile,
        PrevFile,
        ToggleHunk,
        StartComment,
        SubmitComment,
        CancelComment,
        ExtendDown,
        ExtendUp
    ]
);
type Styles = HashMap<(usize, usize, usize), Vec<(std::ops::Range<usize>, HighlightStyle)>>;

struct Review {
    model: ReviewModel,
    rows: Vec<Row>,
    styles: Styles,
    list: ListState,
    sidebar: UniformListScrollHandle,
    current_file: usize,
    current_hunk: (usize, usize),
    focus: FocusHandle,
    input: Entity<TextareaState>,
    _input_subscription: Subscription,
    first_frame: bool,
    started: Instant,
    last_frame: Instant,
    bench: Option<Vec<usize>>,
    bench_index: usize,
    frame_times: Vec<f64>,
}

fn highlights(model: &ReviewModel) -> Styles {
    let theme = HighlightTheme::default_dark();
    let mut styles = HashMap::new();
    for (f, file) in model.files.iter().enumerate() {
        let language = match file_key(file).rsplit('.').next().unwrap_or("") {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "toml" | "lock" => "toml",
            "yaml" | "yml" => "yaml",
            "json" => "json",
            "sh" => "bash",
            _ => "text",
        };
        for (h, hunk) in file.hunks.iter().enumerate() {
            for side in [Side::Old, Side::New] {
                let mut code = String::new();
                let mut ranges = Vec::new();
                for (l, line) in hunk.lines.iter().enumerate() {
                    if (side == Side::Old && line.kind == LineKind::Add)
                        || (side == Side::New && line.kind == LineKind::Delete)
                    {
                        continue;
                    }
                    let start = code.len();
                    code.push_str(&line.text);
                    code.push('\n');
                    if (side == Side::Old) == (line.kind == LineKind::Delete) {
                        ranges.push((l, start..code.len() - 1));
                    }
                }
                let mut highlighter = SyntaxHighlighter::new(language);
                highlighter.update(None, &Rope::from_str(&code), None);
                for (l, range) in ranges {
                    let spans = highlighter
                        .styles(&range, &*theme)
                        .into_iter()
                        .map(|(span, style)| {
                            ((span.start - range.start)..(span.end - range.start), style)
                        })
                        .collect();
                    styles.insert((f, h, l), spans);
                }
            }
        }
    }
    styles
}

impl Review {
    fn rebuild(&mut self) {
        let top = self.list.logical_scroll_top();
        self.rows = self.model.rows();
        self.list.reset(self.rows.len());
        self.list.scroll_to(ListOffset {
            item_ix: top.item_ix.min(self.rows.len().saturating_sub(1)),
            offset_in_item: top.offset_in_item,
        });
    }
    fn jump(&mut self, row: Row) {
        if let Some(index) = self.rows.iter().position(|r| *r == row) {
            self.list.scroll_to(ListOffset {
                item_ix: index,
                offset_in_item: px(0.),
            });
        }
    }
    fn file(&mut self, f: usize) {
        if self.model.files.is_empty() {
            return;
        }
        self.current_file = f.min(self.model.files.len() - 1);
        self.current_hunk = (self.current_file, 0);
        self.model.cursor = None;
        self.model.origin = None;
        self.sidebar
            .scroll_to_item(self.current_file, ScrollStrategy::Nearest);
        self.jump(Row::File(self.current_file));
    }
    fn hunk(&mut self, direction: isize) {
        let current = self.model.active_hunk(self.current_hunk);
        if let Some((f, h)) = self.model.navigate_hunk(current, direction) {
            self.current_hunk = (f, h);
            self.current_file = f;
            self.model.select(
                Position {
                    file: f,
                    hunk: h,
                    line: 0,
                },
                false,
            );
            self.jump(Row::Hunk(f, h));
        }
    }
    fn toggle(&mut self) {
        let (f, h) = self.model.active_hunk(self.current_hunk);
        self.model.toggle(f, h);
        self.rebuild();
        self.jump(Row::Hunk(f, h));
    }
    fn start_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.selection().is_none() {
            let (f, h) = self.current_hunk;
            self.model.select(
                Position {
                    file: f,
                    hunk: h,
                    line: 0,
                },
                false,
            );
        }
        self.model.draft = self.model.selection();
        if self.model.draft.is_none() {
            return;
        }
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.rebuild();
        if let Some(index) = self.rows.iter().position(|r| *r == Row::Draft) {
            self.list.scroll_to_reveal_item(index);
        }
        cx.notify();
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value();
        if self.model.submit(&text) {
            self.focus.focus(window, cx);
            self.rebuild();
            cx.notify();
        }
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.draft = None;
        self.focus.focus(window, cx);
        self.rebuild();
        cx.notify();
    }
    fn row(&mut self, index: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let base = div()
            .id(index)
            .w_full()
            .min_h(px(28.))
            .text_size(px(14.))
            .line_height(px(28.));
        match self.rows[index] {
            Row::File(f) => base
                .h(px(38.))
                .bg(rgb(0x303f59))
                .px_3()
                .font_weight(FontWeight::BOLD)
                .child(format!(
                    "{}   ·   {} hunks",
                    file_key(&self.model.files[f]),
                    self.model.files[f].hunks.len()
                ))
                .into_any_element(),
            Row::Hunk(f, h) => {
                let folded = self.model.collapsed.contains(&(f, h));
                let current = self.current_hunk == (f, h);
                base.h(px(30.))
                    .px_3()
                    .bg(rgb(if current { 0x344c6a } else { 0x26364b }))
                    .text_color(rgb(0x9bcaff))
                    .cursor_pointer()
                    .child(format!(
                        "{} {}{}",
                        if folded { "▶" } else { "▼" },
                        self.model.files[f].hunks[h].header,
                        if folded { "  · collapsed" } else { "" }
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.focus.focus(window, cx);
                        this.model.cursor = None;
                        this.model.origin = None;
                        this.current_hunk = (f, h);
                        this.toggle();
                        cx.notify();
                    }))
                    .into_any_element()
            }
            Row::Line(p) => {
                let line = &self.model.files[p.file].hunks[p.hunk].lines[p.line];
                let selected = self.model.selection().is_some_and(|a| {
                    self.model.point(p).is_some_and(|b| {
                        a.file == b.file && a.side == b.side && (a.start..=a.end).contains(&b.start)
                    })
                });
                let (prefix, color) = match line.kind {
                    LineKind::Add => ("+", 0x173c2b),
                    LineKind::Delete => ("−", 0x48262e),
                    LineKind::Context => (" ", 0x171e29),
                };
                let spans = self
                    .styles
                    .get(&(p.file, p.hunk, p.line))
                    .cloned()
                    .unwrap_or_default();
                base.h(px(28.))
                    .flex()
                    .bg(rgb(if selected { 0x365d87 } else { color }))
                    .cursor_pointer()
                    .child(
                        div()
                            .w(px(62.))
                            .flex_shrink_0()
                            .text_right()
                            .pr_2()
                            .text_color(rgb(0x91a0b5))
                            .child(line.old_number.map(|n| n.to_string()).unwrap_or_default()),
                    )
                    .child(
                        div()
                            .w(px(62.))
                            .flex_shrink_0()
                            .text_right()
                            .pr_2()
                            .text_color(rgb(0x91a0b5))
                            .child(line.new_number.map(|n| n.to_string()).unwrap_or_default()),
                    )
                    .child(div().w(px(24.)).flex_shrink_0().child(prefix))
                    .child(StyledText::new(line.text.clone()).with_highlights(spans))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.focus.focus(window, cx);
                        this.current_hunk = (p.file, p.hunk);
                        this.current_file = p.file;
                        this.model.select(p, event.modifiers().shift);
                        cx.notify();
                    }))
                    .into_any_element()
            }
            Row::Comment(i) => {
                let comment = &self.model.comments[i];
                base.px_4()
                    .py_2()
                    .bg(rgb(0x34304b))
                    .border_l_4()
                    .border_color(rgb(0xb69aff))
                    .whitespace_normal()
                    .child(format!(
                        "Review comment · {}\n{}",
                        comment.anchor.label(),
                        comment.text
                    ))
                    .into_any_element()
            }
            Row::Draft => base
                .p_3()
                .bg(rgb(0x28364e))
                .flex()
                .flex_col()
                .gap_2()
                .child(format!(
                    "Comment on {} · Enter / Ctrl+Enter submit · Shift+Enter newline · Esc cancel",
                    self.model.draft.as_ref().unwrap().label()
                ))
                .child(Textarea::new(&self.input).h(px(84.)))
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .child(
                            div()
                                .id("submit")
                                .px_3()
                                .bg(rgb(0x346a51))
                                .cursor_pointer()
                                .child("Post comment")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.submit(window, cx)),
                                ),
                        )
                        .child(
                            div()
                                .id("cancel")
                                .px_3()
                                .cursor_pointer()
                                .child("Cancel")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.cancel(window, cx)),
                                ),
                        ),
                )
                .into_any_element(),
        }
    }
    fn tick(entity: WeakEntity<Self>, window: &mut Window, cx: &mut App) {
        let _ = entity.clone().update(cx, |this,cx| {
            let now=Instant::now();
            if !this.first_frame { eprintln!("FIRST_FRAME_PAINTED_MS={:.2}",this.started.elapsed().as_secs_f64()*1000.); this.first_frame=true; }
            else if this.bench.is_some() { let elapsed=now.duration_since(this.last_frame).as_secs_f64()*1000.; this.frame_times.push(elapsed); eprintln!("BENCH_FRAME={} MS={elapsed:.3}",this.frame_times.len()); }
            this.last_frame=now;
            let top=this.list.logical_scroll_top().item_ix.min(this.rows.len().saturating_sub(1));
            if let Some(row)=this.rows.get(top).copied() {
                if let Some(f)=this.model.row_file(row) { if f!=this.current_file { this.current_file=f; this.sidebar.scroll_to_item(f,ScrollStrategy::Nearest); cx.notify(); } }
                let hunk=match row { Row::File(f)=>Some((f,0)),Row::Hunk(f,h)=>Some((f,h)),Row::Line(p)=>Some((p.file,p.hunk)),_=>None }; if let Some(hunk)=hunk { this.current_hunk=this.model.active_hunk(hunk); }
            }
            if let Some(targets)=&this.bench {
                if let Some(target)=targets.get(this.bench_index) { this.list.scroll_to(ListOffset {item_ix:*target,offset_in_item:px(0.)}); this.bench_index+=1; cx.notify(); }
                else {
                    let mut times=this.frame_times.clone(); times.sort_by(f64::total_cmp);
                    if !times.is_empty() {
                        let rss=std::fs::read_to_string("/proc/self/status").unwrap_or_default().lines().find(|l|l.starts_with("VmHWM:")).unwrap_or("VmHWM: unavailable").to_string();
                        eprintln!("BENCH_SUMMARY frames={} avg_ms={:.3} p95_ms={:.3} max_ms={:.3} peak_rss={rss}",times.len(),times.iter().sum::<f64>()/times.len() as f64,times[((times.len()-1) as f64*0.95).ceil() as usize],times[times.len()-1]);
                    }
                    cx.quit(); return;
                }
            }
            window.on_next_frame(move |window,cx| Self::tick(entity,window,cx));
        });
    }
}

impl Render for Review {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.model.draft.is_some();
        div().size_full().flex().flex_col().bg(rgb(0x171e29)).text_color(rgb(0xdce5f1)).font_family("DejaVu Sans Mono")
            .track_focus(&self.focus).key_context(if editing {"CommentReview"} else {"Review"})
            .on_action(cx.listener(|this,_:&NextHunk,_,cx|{this.hunk(1);cx.notify();}))
            .on_action(cx.listener(|this,_:&PrevHunk,_,cx|{this.hunk(-1);cx.notify();}))
            .on_action(cx.listener(|this,_:&NextFile,_,cx|{this.file(this.current_file+1);cx.notify();}))
            .on_action(cx.listener(|this,_:&PrevFile,_,cx|{this.file(this.current_file.saturating_sub(1));cx.notify();}))
            .on_action(cx.listener(|this,_:&ToggleHunk,_,cx|{this.toggle();cx.notify();}))
            .on_action(cx.listener(|this,_:&ExtendDown,_,cx|{this.model.extend(1);this.rebuild();if let Some(p)=this.model.cursor {this.jump(Row::Line(p));}cx.notify();}))
            .on_action(cx.listener(|this,_:&ExtendUp,_,cx|{this.model.extend(-1);this.rebuild();if let Some(p)=this.model.cursor {this.jump(Row::Line(p));}cx.notify();}))
            .on_action(cx.listener(|this,_:&StartComment,window,cx|this.start_comment(window,cx)))
            .on_action(cx.listener(|this,_:&SubmitComment,window,cx|this.submit(window,cx)))
            .on_action(cx.listener(|this,_:&CancelComment,window,cx|this.cancel(window,cx)))
            .child(div().h(px(56.)).px_4().flex().items_center().justify_between().bg(rgb(0x232d3e))
                .child(format!("Hayami / ripgrep #2610 · {} files · {} rows",self.model.files.len(),self.rows.len()))
                .child(div().id("comment").px_3().cursor_pointer().child("Comment [c]").on_click(cx.listener(|this,_,window,cx|this.start_comment(window,cx)))))
            .child(div().h(px(30.)).px_4().bg(rgb(0x202a39)).text_size(px(12.)).child("j/k hunk · n/p file · o fold · click / Shift+click select · Shift+J/K extend · c comment"))
            .child(div().flex().flex_1().min_h_0()
                .child(div().w(px(340.)).h_full().flex_shrink_0().bg(rgb(0x1e2735)).child(
                    uniform_list("files",self.model.files.len(),cx.processor(|this,range:std::ops::Range<usize>,_,cx|range.map(|f| {
                        div().id(f).h(px(30.)).px_2().text_size(px(12.)).flex().items_center().overflow_hidden().cursor_pointer().bg(rgb(if f==this.current_file {0x395374}else{0x1e2735}))
                            .child(file_key(&this.model.files[f]).to_string()).on_click(cx.listener(move |this,_,window,cx|{this.focus.focus(window,cx);this.file(f);cx.notify();}))
                    }).collect())).track_scroll(&self.sidebar).h_full()))
                .child(list(self.list.clone(),cx.processor(|this,index,window,cx|this.row(index,window,cx))).flex_1().h_full().min_w_0()))
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
    let model = ReviewModel::new(diff::parse(&std::fs::read_to_string(path)?)?);
    let styles = highlights(&model);
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        cx.bind_keys([
            KeyBinding::new("j", NextHunk, Some("Review")),
            KeyBinding::new("k", PrevHunk, Some("Review")),
            KeyBinding::new("n", NextFile, Some("Review")),
            KeyBinding::new("p", PrevFile, Some("Review")),
            KeyBinding::new("o", ToggleHunk, Some("Review")),
            KeyBinding::new("space", ToggleHunk, Some("Review")),
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
                    title: Some("Hayami — GPUI review".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    focus.focus(window, cx);
                    let input = cx.new(|cx| {
                        TextareaState::new(window, cx)
                            .rows(3)
                            .placeholder("Write a review comment…")
                    });
                    let subscription = cx.subscribe_in(
                        &input,
                        window,
                        |this: &mut Review, _, event, window, cx| {
                            if let InputEvent::PressEnter { shift: false, .. } = event {
                                this.submit(window, cx);
                            }
                        },
                    );
                    let rows = model.rows();
                    let targets = bench.then(|| {
                        (0..rows.len())
                            .step_by(20)
                            .chain(
                                rows.iter()
                                    .enumerate()
                                    .filter_map(|(i, r)| matches!(r, Row::File(_)).then_some(i)),
                            )
                            .collect()
                    });
                    window.on_next_frame({
                        let entity = cx.entity().downgrade();
                        move |window, cx| Review::tick(entity, window, cx)
                    });
                    Review {
                        list: ListState::new(rows.len(), ListAlignment::Top, px(300.)),
                        rows,
                        model,
                        styles,
                        sidebar: UniformListScrollHandle::new(),
                        current_file: 0,
                        current_hunk: (0, 0),
                        focus,
                        input,
                        _input_subscription: subscription,
                        first_frame: false,
                        started,
                        last_frame: Instant::now(),
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
