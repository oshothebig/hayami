//! A one-screen pull request review prototype on Native Shell (issue #22).
//!
//! The screen reads a local unified diff (no GitHub API): a file list on the
//! left, the patch in a unified `DiffView` on the right, and inline comments
//! drafted and shown under the lines they are about.

mod index;

use std::rc::Rc;
use std::time::{Duration, Instant};

pub use index::{PatchFile, PatchIndex};
use nsh_base::{
    DiffAnnotation, DiffFile, DiffLayout, DiffLineRange, DiffSide, DiffStats, DiffView,
    DiffViewApi, button, diff_view, input, theme,
};
use nsh_core::{keymap::When, prelude::*};

/// The shared fixture of issues #20, #21 and #22: BurntSushi/ripgrep PR #2610.
pub const FIXTURE: &str = include_str!("../../../fixtures/ripgrep-2610.diff");

const DRAFT: &str = "draft";

/// The app's UI set (`nsh.json` `"rust"`).
#[native_module("hayami-nsh/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// The patch to review: the file `HAYAMI_DIFF` names, else the shared fixture.
fn load_patch() -> String {
    std::env::var("HAYAMI_DIFF")
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_else(|| FIXTURE.to_owned())
}

#[derive(Debug, Clone, PartialEq)]
struct Comment {
    id: SharedString,
    range: DiffLineRange,
    place: String,
    body: String,
}

/// The app's root: the review screen fills the platform's main window.
#[component]
pub fn App() -> impl IntoElement {
    screen()
}

fn screen() -> impl IntoElement {
    view()
        .flex_1()
        .bg(move || theme().background)
        .child(review(load_patch()))
}

fn review(patch: String) -> impl IntoElement {
    let started = Instant::now();
    let index = Rc::new(PatchIndex::parse(&patch));
    let patch_lines = PatchIndex::line_count(&patch);
    let patch = SharedString::from(patch);

    let dv = component_ref::<DiffView>();
    let files = signal(Vec::<DiffFile>::new());
    let current_file = signal(0usize);
    let stats = signal(DiffStats::default());
    let comments = signal(Vec::<Comment>::new());
    let draft = signal(None::<DiffLineRange>);
    let draft_text = signal(String::new());
    let composing = signal(String::new());
    let next_id = signal(1u32);
    let status = signal(String::from("Loading…"));

    let api = move |f: &dyn Fn(&DiffViewApi)| {
        if let Some(api) = dv.api::<DiffViewApi>() {
            f(&api);
        }
    };
    let follow_hunk = move || {
        api(&|api| {
            if let Some(i) = api.current_hunk()
                && let Some(h) = api.hunks().get(i as usize)
            {
                current_file.set(h.file as usize);
            }
        })
    };
    let go_to_file = move |i: usize| {
        let Some(path) = files.with_untracked(|f| f.get(i).map(|f| f.path.to_string())) else {
            return;
        };
        current_file.set(i);
        api(&|api| api.scroll_to_file(path.clone()));
    };
    let start_comment = {
        let index = index.clone();
        move |range: Option<DiffLineRange>| {
            let range = range.or_else(|| {
                let api = dv.api::<DiffViewApi>()?;
                api.selected_lines().or_else(|| {
                    let h = api.hunks().get(api.current_hunk()? as usize).copied()?;
                    let file = h.file as usize;
                    let (side, line) = if h.new_lines > 0 {
                        (
                            DiffSide::New,
                            index.joined(DiffSide::New, file, h.new_start),
                        )
                    } else {
                        (
                            DiffSide::Old,
                            index.joined(DiffSide::Old, file, h.old_start),
                        )
                    };
                    let mut r = DiffLineRange::default();
                    r.side = side;
                    r.start = line;
                    r.end = line;
                    Some(r)
                })
            });
            let Some(range) = range else {
                status.set("Select lines on the line numbers first".to_owned());
                return;
            };
            // GTK reveals the newly focused comment box by scrolling the
            // diff's virtualized rows to the top; keep the view where it was.
            if let Some(api) = dv.api::<DiffViewApi>() {
                let offset = api.scroll_offset();
                set_timeout(Duration::from_millis(400), move || {
                    if let Some(api) = dv.api::<DiffViewApi>() {
                        api.set_scroll_offset(offset);
                    }
                });
            }
            draft.set(Some(range));
            draft_text.set(String::new());
            status.set(format!(
                "Commenting on {}",
                index.describe(range.side, range.start, range.end)
            ));
        }
    };
    let cancel_comment = move || {
        if draft.get_untracked().is_some() {
            draft.set(None);
            draft_text.set(String::new());
            status.set("Comment canceled".to_owned());
        }
    };
    let submit_comment = {
        let index = index.clone();
        move || {
            let Some(range) = draft.get_untracked() else {
                return;
            };
            let body = draft_text.get_untracked();
            if body.trim().is_empty() {
                return;
            }
            let id = next_id.get_untracked();
            next_id.set(id + 1);
            let place = index.describe(range.side, range.start, range.end);
            comments.update(|c| {
                c.push(Comment {
                    id: SharedString::from(format!("c{id}")),
                    range,
                    place: place.clone(),
                    body: body.trim().to_owned(),
                })
            });
            draft.set(None);
            draft_text.set(String::new());
            status.set(format!("Comment posted on {place}"));
        }
    };

    on_key("j", move |_| {
        api(&|api| {
            api.next_hunk();
        });
        follow_hunk();
    });
    on_key("k", move |_| {
        api(&|api| {
            api.previous_hunk();
        });
        follow_hunk();
    });
    on_key("shift+j", move |_| {
        let n = files.with_untracked(Vec::len);
        go_to_file((current_file.get_untracked() + 1).min(n.saturating_sub(1)));
    });
    on_key("shift+k", move |_| {
        go_to_file(current_file.get_untracked().saturating_sub(1))
    });
    on_key("o", move |_| {
        if let Some(path) = files.with_untracked(|f| {
            f.get(current_file.get_untracked())
                .map(|f| f.path.to_string())
        }) {
            api(&|api| api.toggle_file(path.clone()));
        }
    });
    on_key("shift+o", move |_| {
        let all_collapsed = files.with_untracked(|f| f.iter().all(|f| f.collapsed));
        api(&|api| {
            for f in api.files() {
                let path = f.path.to_string();
                if all_collapsed {
                    api.expand_file(path);
                } else {
                    api.collapse_file(path);
                }
            }
        });
    });
    {
        let start_comment = start_comment.clone();
        on_key("c", move |_| start_comment(None));
    }
    let always = KeyOptions {
        when: When::Always,
        ..Default::default()
    };
    on_key_with("escape", always, move |_| cancel_comment());
    {
        let submit_comment = submit_comment.clone();
        on_key_with("mod+enter", always, move |_| submit_comment());
    }

    let annotations = move || {
        let mut list: Vec<DiffAnnotation> = comments.with(|c| {
            c.iter()
                .map(|c| {
                    let mut a = DiffAnnotation::default();
                    a.id = c.id.clone();
                    a.side = c.range.side;
                    a.line = c.range.end;
                    a
                })
                .collect()
        });
        if let Some(range) = draft.get() {
            let mut a = DiffAnnotation::default();
            a.id = SharedString::from(DRAFT);
            a.side = range.side;
            a.line = range.end;
            list.push(a);
        }
        list
    };

    let render_annotation = {
        let submit_comment = submit_comment.clone();
        move |a: &DiffAnnotation| -> AnyElement {
            if a.id == DRAFT {
                draft_box(
                    draft_text,
                    composing,
                    submit_comment.clone(),
                    cancel_comment,
                )
                .into_element()
            } else {
                comment_box(comments, a.id.clone()).into_element()
            }
        }
    };

    let start_from_gutter = start_comment.clone();
    let diff = diff_view()
        .patch(patch)
        .layout(DiffLayout::Unified)
        .annotations(annotations)
        .render_annotation(render_annotation)
        .render_add_annotation(move |range: &DiffLineRange| {
            let range = *range;
            let start = start_from_gutter.clone();
            button()
                .test_id("add-comment")
                .accessibility_label("Add comment")
                .on_press(move || start(Some(range)))
                .w(px(16.0))
                .h(px(16.0))
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .bg(move || theme().primary)
                .child(
                    text("+")
                        .text_xs()
                        .text_color(move || theme().primary_foreground),
                )
        })
        .on_stats_change(move |s: &DiffStats| {
            stats.set(*s);
            if let Some(api) = dv.api::<DiffViewApi>() {
                files.set(api.files());
            }
            status.set(format!(
                "Loaded {patch_lines} patch lines in {} ms",
                started.elapsed().as_millis()
            ));
            log::info!("diff ready: {patch_lines} lines in {:?}", started.elapsed());
        })
        .on_file_toggle(move |f: &DiffFile| {
            files.update(|list| {
                if let Some(entry) = list.get_mut(f.index as usize) {
                    entry.collapsed = f.collapsed;
                }
            })
        })
        .component_ref(&dv)
        .flex_1();

    view()
        .flex_1()
        .flex_col()
        .child(header(stats, files, comments))
        .child(
            view()
                .flex_1()
                .flex_row()
                .child(file_list(files, current_file, go_to_file))
                .child(view().flex_1().flex_col().child(diff)),
        )
        .child(footer(status))
}

fn header(
    stats: Signal<DiffStats>,
    files: Signal<Vec<DiffFile>>,
    comments: Signal<Vec<Comment>>,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(move || theme().border)
        .child(
            text("BurntSushi/ripgrep#2610 · add hyperlink support")
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || theme().foreground),
        )
        .child(
            text(move || {
                let s = stats.get();
                format!(
                    "{} files · +{} −{} · {} comments",
                    files.with(Vec::len),
                    s.added,
                    s.deleted,
                    comments.with(Vec::len)
                )
            })
            .test_id("summary")
            .text_sm()
            .text_color(move || theme().muted_foreground),
        )
}

fn file_list(
    files: Signal<Vec<DiffFile>>,
    current_file: Signal<usize>,
    go_to_file: impl Fn(usize) + Copy + 'static,
) -> impl IntoElement {
    scroll()
        .w(px(300.0))
        .border_r_1()
        .border_color(move || theme().border)
        .child(view().flex_col().py_1().child(each(
            move || files.get(),
            |f: &DiffFile| f.index,
            move |file, _| {
                let index = move || file.with(|f| f.index as usize);
                let selected = move || current_file.get() == index();
                button()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .bg(move || {
                        if selected() {
                            theme().accent
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    })
                    .hover(|s| s.bg(move || theme().muted))
                    .on_press(move || go_to_file(index()))
                    .child(
                        text(move || file.with(|f| f.path.to_string()))
                            .flex_1()
                            .text_sm()
                            .truncate()
                            .text_color(move || theme().foreground),
                    )
                    .child(
                        text(move || file.with(|f| format!("+{}", f.added)))
                            .text_xs()
                            .text_color(move || hsla(0.36, 0.6, 0.4, 1.0)),
                    )
                    .child(
                        text(move || file.with(|f| format!("−{}", f.deleted)))
                            .text_xs()
                            .text_color(move || hsla(0.0, 0.65, 0.5, 1.0)),
                    )
            },
        )))
}

fn draft_box(
    draft_text: Signal<String>,
    composing: Signal<String>,
    submit: impl Fn() + Clone + 'static,
    cancel: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let submit_button = submit.clone();
    view()
        .test_id("draft")
        .m_2()
        .p_2()
        .gap_2()
        .flex_col()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().ring)
        .bg(move || theme().card)
        .child(
            input()
                .test_id("comment-input")
                .auto_focus(true)
                .placeholder("Leave a comment (Enter to post, Esc to cancel)")
                .value(move || draft_text.get())
                .on_change(move |v: &str| draft_text.set(v.to_owned()))
                .on_composition_change(move |c: &Composition| {
                    composing.set(if c.composing {
                        c.text.clone()
                    } else {
                        String::new()
                    })
                })
                .on_submit(move |_: &str| submit())
                .px_2()
                .py_1()
                .border_1()
                .border_color(move || theme().input)
                .rounded(move || theme().radius)
                .text_color(move || theme().foreground),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    text(move || {
                        let c = composing.get();
                        if c.is_empty() {
                            String::new()
                        } else {
                            format!("Composing: {c}")
                        }
                    })
                    .flex_1()
                    .text_xs()
                    .text_color(move || theme().muted_foreground),
                )
                .child(
                    button()
                        .accessibility_label("Cancel")
                        .on_press(cancel)
                        .px_3()
                        .py_1()
                        .rounded(move || theme().radius)
                        .border_1()
                        .border_color(move || theme().border)
                        .child(
                            text("Cancel")
                                .text_sm()
                                .text_color(move || theme().foreground),
                        ),
                )
                .child(
                    button()
                        .accessibility_label("Comment")
                        .disabled(move || draft_text.with(|t| t.trim().is_empty()))
                        .disabled_style(|s| s.opacity(0.4))
                        .on_press(submit_button)
                        .px_3()
                        .py_1()
                        .rounded(move || theme().radius)
                        .bg(move || theme().primary)
                        .child(
                            text("Comment")
                                .text_sm()
                                .text_color(move || theme().primary_foreground),
                        ),
                ),
        )
}

fn comment_box(comments: Signal<Vec<Comment>>, id: SharedString) -> impl IntoElement {
    let comment = computed(move || comments.with(|c| c.iter().find(|c| c.id == id).cloned()));
    view()
        .m_2()
        .p_2()
        .gap_1()
        .flex_col()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .child(
            text(move || {
                comment
                    .get()
                    .map(|c| format!("You · {}", c.place))
                    .unwrap_or_default()
            })
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(move || theme().muted_foreground),
        )
        .child(
            text(move || comment.get().map(|c| c.body).unwrap_or_default())
                .text_sm()
                .text_color(move || theme().card_foreground),
        )
}

fn footer(status: Signal<String>) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_4()
        .px_4()
        .py_1()
        .border_t_1()
        .border_color(move || theme().border)
        .child(
            text(move || status.get())
                .test_id("status")
                .flex_1()
                .text_xs()
                .text_color(move || theme().muted_foreground),
        )
        .child(
            text("j/k hunk · J/K file · o fold file · O fold all · c comment · Enter post · Esc cancel")
                .text_xs()
                .text_color(move || theme().muted_foreground),
        )
}
