//! Review screen: file tree and virtualized diff.

use std::sync::{Arc, Mutex};

use gpui::{
    Context, Div, HighlightStyle, ListHorizontalSizingBehavior, ScrollStrategy, SharedString,
    Stateful, StyledText, UniformListScrollHandle, div, prelude::*, px, uniform_list,
};
use hayami_core::diff::{FileDiff, LineKind};
use hayami_core::file_tree::{FileTree, TreeRow};
use hayami_core::github::PrDetail;
use hayami_core::view::{DiffView, DisplayRow, LineView, RowContent, ViewMode};

use crate::root::{RootView, Status};
use crate::theme;

pub const ROW_HEIGHT: f32 = 20.0;
const TREE_WIDTH: f32 = 300.0;

/// Which pane of the review screen has the keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewFocus {
    Tree,
    Diff,
}

pub struct ReviewScreen {
    pub focus: ReviewFocus,
    pub title: String,
    pub detail: Option<PrDetail>,
    pub status: Status,
    /// Progress message written by the background loader.
    pub progress: Arc<Mutex<String>>,
    pub view: Option<DiffView>,
    pub tree: FileTree,
    pub diff_scroll: UniformListScrollHandle,
    pub tree_scroll: UniformListScrollHandle,
}

impl ReviewScreen {
    pub fn new(title: String) -> Self {
        Self {
            focus: ReviewFocus::Diff,
            title,
            detail: None,
            status: Status::Loading("Loading pull request…".into()),
            progress: Arc::new(Mutex::new(String::new())),
            view: None,
            tree: FileTree::default(),
            diff_scroll: UniformListScrollHandle::new(),
            tree_scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn set_files(&mut self, files: Vec<FileDiff>) {
        self.tree = FileTree::new(&files);
        self.view = Some(DiffView::new(Arc::new(files)));
    }

    /// Keeps the cursor row visible and the tree selection on the cursor's file.
    pub fn sync_after_move(&mut self) {
        let Some(view) = &self.view else {
            return;
        };
        self.diff_scroll
            .scroll_to_item(view.cursor(), ScrollStrategy::Top);
        if let Some(file) = view.current_file() {
            self.tree.select_file(file);
            self.tree_scroll
                .scroll_to_item(self.tree.selected_row(), ScrollStrategy::Top);
        }
    }

    /// Moves the tree selection and shows that file in the diff.
    pub fn move_tree_selection(&mut self, apply: impl FnOnce(&mut FileTree)) {
        apply(&mut self.tree);
        let file = self.tree.selected_file();
        if let Some(view) = &mut self.view {
            view.jump_to_file(file);
        }
        self.sync_after_move();
    }

    /// Current loading message, preferring live progress from the loader.
    pub fn status_message(&self) -> Status {
        match &self.status {
            Status::Loading(fallback) => {
                let live = self.progress.lock().map(|p| p.clone()).unwrap_or_default();
                Status::Loading(if live.is_empty() {
                    fallback.clone()
                } else {
                    live
                })
            }
            other => other.clone(),
        }
    }
}

pub fn render(screen: &ReviewScreen, cx: &mut Context<RootView>) -> Div {
    let mode = match screen.view.as_ref().map(DiffView::mode) {
        Some(ViewMode::Split) => "  ·  split",
        Some(ViewMode::Unified) => "  ·  unified",
        None => "",
    };
    let header = match &screen.detail {
        Some(d) => format!(
            "{}  ·  {} ← {}  ·  @{}  ·  review: {}",
            screen.title,
            d.base_ref_name,
            d.head_ref_name,
            d.author,
            d.review_decision.as_deref().unwrap_or("none")
        ),
        None => screen.title.clone(),
    } + mode;
    let body = div().flex().flex_row().flex_grow().min_h_0();
    let body = match &screen.view {
        Some(view) => body
            .child(render_tree(screen, cx))
            .child(render_diff(view, screen, cx)),
        None => body,
    };
    div()
        .flex()
        .flex_col()
        .size_full()
        .child(crate::root::title_bar(&header))
        .children(crate::root::render_status(&screen.status_message()))
        .child(body)
        .child(crate::root::hint_bar(match screen.focus {
            ReviewFocus::Diff => {
                "j/k move · n/p hunk · ]/[ file · o collapse · ⌘⇧D split · ⌘⇧E tree · ⌘⇧P commands · ⌘[ back"
            }
            ReviewFocus::Tree => {
                "j/k select file · ↩/esc focus diff · o collapse · ⌘⇧P commands · ⌘[ back"
            }
        }))
}

fn render_tree(screen: &ReviewScreen, cx: &mut Context<RootView>) -> Stateful<Div> {
    let count = screen.tree.rows().len();
    let list = uniform_list(
        "file-tree",
        count,
        cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
            let Some(screen) = this.review() else {
                return Vec::new();
            };
            let Some(view) = &screen.view else {
                return Vec::new();
            };
            let files = view.files().clone();
            let selected = screen.tree.selected_row();
            let rows = screen.tree.rows();
            range
                .filter_map(|ix| {
                    rows.get(ix)
                        .map(|row| render_tree_row(ix, row, ix == selected, &files, cx))
                })
                .collect::<Vec<_>>()
        }),
    )
    .track_scroll(screen.tree_scroll.clone())
    .size_full();
    div()
        .id("tree-panel")
        .w(px(TREE_WIDTH))
        .h_full()
        .flex_shrink_0()
        .bg(theme::panel_bg())
        .border_r_1()
        .border_color(if screen.focus == ReviewFocus::Tree {
            theme::accent()
        } else {
            theme::border()
        })
        .child(list)
}

fn render_tree_row(
    ix: usize,
    row: &TreeRow,
    selected: bool,
    files: &[FileDiff],
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    let base = div()
        .id(ix)
        .h(px(ROW_HEIGHT))
        .px_2()
        .flex()
        .items_center()
        .whitespace_nowrap()
        .overflow_hidden();
    match row {
        TreeRow::Dir { name, depth } => base
            .pl(px(8.0 + 12.0 * *depth as f32))
            .text_color(theme::muted())
            .child(format!("{name}/")),
        TreeRow::File { name, depth, file } => {
            let file_index = *file;
            let diff = &files[file_index];
            base.pl(px(8.0 + 12.0 * *depth as f32))
                .gap_2()
                .when(selected, |d| d.bg(theme::selection()))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    this.jump_to_file(file_index);
                    cx.notify();
                }))
                .child(
                    div()
                        .text_color(status_color(diff))
                        .child(diff.status.marker().to_string()),
                )
                .child(div().flex_grow().child(name.clone()))
                .child(
                    div()
                        .text_color(theme::muted())
                        .text_sm()
                        .child(format!("+{} -{}", diff.additions, diff.deletions)),
                )
        }
    }
}

fn status_color(file: &FileDiff) -> gpui::Rgba {
    match file.status.marker() {
        'A' => theme::added_fg(),
        'D' => theme::removed_fg(),
        'R' => theme::accent(),
        _ => theme::text(),
    }
}

fn render_diff(view: &DiffView, screen: &ReviewScreen, cx: &mut Context<RootView>) -> Div {
    let list = uniform_list(
        "diff",
        view.row_count(),
        cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| {
            let Some(view) = this.review().and_then(|s| s.view.as_ref()) else {
                return Vec::new();
            };
            let cursor = view.cursor();
            view.rows(range)
                .iter()
                .map(|row| render_diff_row(row, row.index == cursor))
                .collect::<Vec<_>>()
        }),
    )
    // Unified rows scroll horizontally; split halves clip to half the width.
    .with_horizontal_sizing_behavior(match view.mode() {
        ViewMode::Unified => ListHorizontalSizingBehavior::Unconstrained,
        ViewMode::Split => ListHorizontalSizingBehavior::FitList,
    })
    .track_scroll(screen.diff_scroll.clone())
    .size_full();
    div().flex_grow().min_w_0().h_full().child(list)
}

fn line_number(n: Option<u32>) -> Div {
    div()
        .w(px(44.0))
        .flex_shrink_0()
        .text_color(theme::muted())
        .text_right()
        .pr_2()
        .child(n.map(|n| n.to_string()).unwrap_or_default())
}

fn render_diff_row(row: &DisplayRow<'_>, is_cursor: bool) -> Stateful<Div> {
    let base = div()
        .id(row.index)
        .h(px(ROW_HEIGHT))
        .flex()
        .flex_row()
        .items_center()
        .whitespace_nowrap()
        .border_l_2()
        .border_color(if is_cursor {
            theme::accent()
        } else {
            gpui::transparent_black().into()
        });
    match &row.content {
        RowContent::FileHeader { file, collapsed } => {
            let name = match &file.status {
                hayami_core::diff::FileStatus::Renamed { from } => {
                    format!("{from} → {}", file.path)
                }
                _ => file.path.clone(),
            };
            base.bg(theme::header_bg())
                .px_2()
                .gap_2()
                .child(if *collapsed { "▸" } else { "▾" })
                .child(
                    div()
                        .text_color(status_color(file))
                        .child(file.status.marker().to_string()),
                )
                .child(name)
                .child(
                    div()
                        .text_color(theme::muted())
                        .child(format!("+{} -{}", file.additions, file.deletions)),
                )
        }
        RowContent::Notice(text) => base.pl(px(100.0)).text_color(theme::muted()).child(*text),
        RowContent::HunkHeader(text) => base
            .bg(theme::hunk_bg())
            .text_color(theme::muted())
            .pl(px(100.0))
            .child(text.clone()),
        RowContent::Line(line) => {
            let (bg, sign) = line_style(line.kind);
            base.when_some(bg, |d, bg| d.bg(bg))
                .child(line_number(line.old_no))
                .child(line_number(line.new_no))
                .child(div().w(px(16.0)).flex_shrink_0().child(sign))
                .child(styled_text(line))
        }
        RowContent::Split { left, right } => base
            .child(split_half(left.as_ref(), true))
            .child(
                div()
                    .w(px(1.0))
                    .h_full()
                    .flex_shrink_0()
                    .bg(theme::border()),
            )
            .child(split_half(right.as_ref(), false)),
    }
}

fn line_style(kind: LineKind) -> (Option<gpui::Rgba>, &'static str) {
    match kind {
        LineKind::Added => (Some(theme::added_bg()), "+"),
        LineKind::Removed => (Some(theme::removed_bg()), "-"),
        LineKind::Context => (None, " "),
    }
}

/// One side of a split row; an empty side is drawn as filler.
fn split_half(line: Option<&LineView<'_>>, old_side: bool) -> Div {
    let half = div()
        .w_1_2()
        .h_full()
        .flex()
        .flex_row()
        .items_center()
        .overflow_hidden();
    let Some(line) = line else {
        return half.bg(theme::filler_bg());
    };
    let (bg, sign) = line_style(line.kind);
    half.when_some(bg, |d, bg| d.bg(bg))
        .child(line_number(if old_side {
            line.old_no
        } else {
            line.new_no
        }))
        .child(div().w(px(16.0)).flex_shrink_0().child(sign))
        .child(styled_text(line))
}

/// Line text with syntax colors and intra-line emphasis.
fn styled_text(line: &LineView<'_>) -> StyledText {
    let (text, segments) = line.segments();
    let emphasis = match line.kind {
        LineKind::Added => Some(theme::added_emphasis_bg()),
        LineKind::Removed => Some(theme::removed_emphasis_bg()),
        LineKind::Context => None,
    };
    let highlights: Vec<_> = segments
        .into_iter()
        .filter(|s| s.kind.is_some() || s.emphasized)
        .map(|s| {
            let style = HighlightStyle {
                color: s.kind.map(|k| theme::syntax(k).into()),
                background_color: emphasis.filter(|_| s.emphasized).map(Into::into),
                ..Default::default()
            };
            (s.range, style)
        })
        .collect();
    StyledText::new(SharedString::from(text)).with_highlights(highlights)
}
