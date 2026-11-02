//! Root view: owns the screens and routes key events through the core keymap.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Context, Div, FocusHandle, KeyDownEvent, ScrollStrategy, SharedString, UniformListScrollHandle,
    Window, div, prelude::*, px, uniform_list,
};
use hayami_core::auth::{
    AuthError, AuthMethod, CredentialStore, GhCli, StoredToken, Token, TokenProvider, TokenSource,
    choose_auth_method, login_shell_path,
};
use hayami_core::diff::FileDiff;
use hayami_core::git::load_pr_diff;
use hayami_core::github::{GithubClient, GithubError, PrDetail, PrList, ReqwestTransport};
use hayami_core::keymap::{Command, KeyContext, KeyInterpreter, KeyOutcome};
use hayami_core::paths::repos_root;
use hayami_core::pr_list::{PrListRow, PrListState};
use hayami_core::text_input::TextInput;

use crate::keychain::Keychain;
use crate::keys::to_press;
use crate::review::{self, ROW_HEIGHT, ReviewScreen};
use crate::theme;

/// Loading state of something fetched in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Idle,
    Loading(String),
    Failed(String),
}

enum Screen {
    /// Resolving how to authenticate.
    Starting,
    /// `gh` is unavailable: ask for a personal access token.
    TokenEntry {
        input: TextInput,
        error: Option<String>,
    },
    /// PR list.
    PrList,
    /// Reviewing one PR.
    Review(Box<ReviewScreen>),
}

/// Failure while opening a PR.
enum OpenError {
    Github(GithubError),
    Other(String),
}

pub struct RootView {
    focus: FocusHandle,
    keys: KeyInterpreter,
    client: Option<Arc<GithubClient>>,
    screen: Screen,
    pr_list: PrListState,
    list_status: Status,
    list_scroll: UniformListScrollHandle,
    /// Incremented whenever a PR is (re)opened or closed; stale results are dropped.
    generation: u64,
}

impl RootView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut view = Self {
            focus,
            keys: KeyInterpreter::new(),
            client: None,
            screen: Screen::Starting,
            pr_list: PrListState::default(),
            list_status: Status::Loading("Looking for gh…".into()),
            list_scroll: UniformListScrollHandle::new(),
            generation: 0,
        };
        view.start_auth(cx);
        view
    }

    pub fn review(&self) -> Option<&ReviewScreen> {
        match &self.screen {
            Screen::Review(screen) => Some(screen),
            _ => None,
        }
    }

    fn review_mut(&mut self) -> Option<&mut ReviewScreen> {
        match &mut self.screen {
            Screen::Review(screen) => Some(screen),
            _ => None,
        }
    }

    /// Runs `job` on a background thread and applies `done` on the UI thread.
    fn background<T: Send + 'static>(
        cx: &mut Context<Self>,
        job: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let task = cx.background_executor().spawn(async move { job() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                done(this, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_auth(&mut self, cx: &mut Context<Self>) {
        Self::background(
            cx,
            || {
                let path = login_shell_path();
                let method = choose_auth_method(path.as_deref());
                let stored = match method {
                    AuthMethod::Gh(_) => Ok(None),
                    AuthMethod::Manual => Keychain.load(),
                };
                (method, path, stored)
            },
            |this, (method, path, stored), cx| match method {
                AuthMethod::Gh(gh) => {
                    this.set_token_source(Box::new(GhCli::new(gh, path)));
                    this.reload_list(cx);
                }
                AuthMethod::Manual => match stored {
                    Ok(Some(_)) => {
                        this.set_token_source(Box::new(StoredToken::new(Keychain)));
                        this.reload_list(cx);
                    }
                    Ok(None) => this.show_token_entry(None),
                    Err(e) => this.show_token_entry(Some(e.to_string())),
                },
            },
        );
    }

    fn set_token_source(&mut self, source: Box<dyn TokenSource>) {
        let tokens = Arc::new(TokenProvider::new(source));
        match ReqwestTransport::new() {
            Ok(transport) => {
                self.client = Some(Arc::new(GithubClient::new(Arc::new(transport), tokens)));
            }
            Err(e) => self.list_status = Status::Failed(e),
        }
        self.screen = Screen::PrList;
    }

    fn show_token_entry(&mut self, error: Option<String>) {
        self.client = None;
        self.screen = Screen::TokenEntry {
            input: TextInput::new(),
            error,
        };
    }

    /// Converts a GitHub error into a status, switching to token entry when
    /// the manual token is missing or was rejected.
    fn handle_github_error(&mut self, error: GithubError) -> Status {
        match error {
            GithubError::Auth(AuthError::ManualTokenRejected | AuthError::NoStoredToken) => {
                self.show_token_entry(Some(error.to_string()));
                Status::Idle
            }
            other => Status::Failed(other.to_string()),
        }
    }

    fn reload_list(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.list_status = Status::Loading("Loading pull requests…".into());
        Self::background(
            cx,
            move || client.list_pull_requests(),
            |this, result: Result<PrList, GithubError>, _cx| match result {
                Ok(list) => {
                    this.pr_list.replace(list);
                    this.list_status = Status::Idle;
                    this.scroll_list_to_selection();
                }
                Err(e) => this.list_status = this.handle_github_error(e),
            },
        );
    }

    fn open_selected_pr(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(summary)) = (self.client.clone(), self.pr_list.selected_pr())
        else {
            return;
        };
        let pr = summary.pr.clone();
        let screen = ReviewScreen::new(format!("{} {}", pr, summary.title));
        let progress = screen.progress.clone();
        self.screen = Screen::Review(Box::new(screen));
        self.generation += 1;
        let generation = self.generation;

        let Some(root) = repos_root() else {
            if let Some(screen) = self.review_mut() {
                screen.status = Status::Failed("Cannot determine the cache directory".into());
            }
            return;
        };
        Self::background(
            cx,
            move || -> Result<(PrDetail, Vec<FileDiff>), OpenError> {
                let report = |message: &str| {
                    if let Ok(mut p) = progress.lock() {
                        *p = message.to_owned();
                    }
                };
                report("Loading pull request metadata…");
                let detail = client.pull_request(&pr).map_err(OpenError::Github)?;
                let diff = load_pr_diff(&detail, &root, Some(client.tokens()), &report)
                    .map_err(|e| OpenError::Other(e.to_string()))?;
                Ok((detail, diff.files))
            },
            move |this, result, _cx| {
                if this.generation != generation {
                    return;
                }
                let status = match result {
                    Ok((detail, files)) => {
                        if let Some(screen) = this.review_mut() {
                            screen.detail = Some(detail);
                            screen.set_files(files);
                            screen.sync_after_move();
                        }
                        Status::Idle
                    }
                    Err(OpenError::Github(e)) => this.handle_github_error(e),
                    Err(OpenError::Other(message)) => Status::Failed(message),
                };
                if let Some(screen) = this.review_mut() {
                    screen.status = status;
                }
            },
        );
        Self::poll_progress(generation, cx);
    }

    /// Re-renders periodically while a PR is loading so progress is visible.
    fn poll_progress(generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let loading = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.generation == generation
                            && matches!(this.review().map(|s| &s.status), Some(Status::Loading(_)))
                    })
                    .unwrap_or(false);
                if !loading {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn jump_to_file(&mut self, file: usize) {
        if let Some(screen) = self.review_mut()
            && let Some(view) = &mut screen.view
        {
            view.jump_to_file(file);
            screen.sync_after_move();
        }
    }

    fn submit_token(&mut self, cx: &mut Context<Self>) {
        let Screen::TokenEntry { input, error } = &mut self.screen else {
            return;
        };
        let Some(token) = Token::new(input.take()) else {
            *error = Some("Token is empty".into());
            return;
        };
        if let Err(e) = Keychain.save(&token) {
            *error = Some(e.to_string());
            return;
        }
        self.set_token_source(Box::new(StoredToken::new(Keychain)));
        self.reload_list(cx);
    }

    fn key_context(&self) -> KeyContext {
        match self.screen {
            Screen::TokenEntry { .. } => KeyContext::TextInput,
            Screen::Starting | Screen::PrList => KeyContext::PrList,
            Screen::Review(_) => KeyContext::Diff,
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let press = to_press(&event.keystroke);
        let context = self.key_context();
        match self.keys.handle(press.clone(), context) {
            KeyOutcome::Command(command) => self.run_command(command, window, cx),
            KeyOutcome::Pending => {}
            KeyOutcome::Unhandled => {
                if let Screen::TokenEntry { input, .. } = &mut self.screen {
                    if press.key == "backspace" {
                        input.backspace();
                    } else if press.is_typing()
                        && let Some(typed) = &event.keystroke.key_char
                    {
                        input.insert(typed);
                    }
                }
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn scroll_list_to_selection(&self) {
        self.list_scroll
            .scroll_to_item(self.pr_list.selected_row(), ScrollStrategy::Center);
    }

    fn half_page(window: &Window) -> isize {
        let rows = (window.viewport_size().height / px(ROW_HEIGHT)).floor() as isize;
        (rows / 2).max(1)
    }

    fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        let half = Self::half_page(window);
        match command {
            Command::Quit => cx.quit(),
            Command::Reload => match &self.screen {
                Screen::Review(_) => self.open_selected_pr(cx),
                _ => self.reload_list(cx),
            },
            Command::Back => {
                if matches!(self.screen, Screen::Review(_)) {
                    self.generation += 1;
                    self.screen = Screen::PrList;
                    self.scroll_list_to_selection();
                }
            }
            Command::Confirm => self.submit_token(cx),
            Command::Cancel => {}
            Command::Paste => {
                if let Screen::TokenEntry { input, .. } = &mut self.screen
                    && let Some(text) = cx.read_from_clipboard().and_then(|c| c.text())
                {
                    input.insert(text.trim());
                }
            }
            Command::Open => {
                if matches!(self.screen, Screen::PrList) {
                    self.open_selected_pr(cx);
                }
            }
            Command::MoveDown
            | Command::MoveUp
            | Command::HalfPageDown
            | Command::HalfPageUp
            | Command::Top
            | Command::Bottom => match &mut self.screen {
                Screen::PrList => {
                    let list = &mut self.pr_list;
                    match command {
                        Command::MoveDown => list.move_by(1),
                        Command::MoveUp => list.move_by(-1),
                        Command::HalfPageDown => list.move_by(half),
                        Command::HalfPageUp => list.move_by(-half),
                        Command::Top => list.select_first(),
                        _ => list.select_last(),
                    }
                    self.scroll_list_to_selection();
                }
                Screen::Review(screen) => {
                    if let Some(view) = &mut screen.view {
                        match command {
                            Command::MoveDown => view.move_by(1),
                            Command::MoveUp => view.move_by(-1),
                            Command::HalfPageDown => view.move_by(half),
                            Command::HalfPageUp => view.move_by(-half),
                            Command::Top => view.top(),
                            _ => view.bottom(),
                        }
                        screen.sync_after_move();
                    }
                }
                _ => {}
            },
        }
    }

    fn render_pr_list(&self, cx: &mut Context<Self>) -> Div {
        let rows = self.pr_list.rows().len();
        let list = uniform_list(
            "pr-list",
            rows,
            cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| {
                let rows = this.pr_list.rows();
                range
                    .filter_map(|ix| rows.get(ix).map(|row| render_pr_row(ix, row)))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.list_scroll.clone())
        .flex_grow();
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(title_bar("Pull requests"))
            .children(render_status(&self.list_status))
            .child(list)
            .child(hint_bar(
                "j/k move · ↩ open · g g / G top/bottom · ⌘R reload · ⌘Q quit",
            ))
    }

    fn render_token_entry(input: &TextInput, error: &Option<String>) -> Div {
        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child("`gh` was not found. Enter a GitHub personal access token.")
            .child(
                div()
                    .text_color(theme::muted())
                    .child("It is stored in the macOS keychain. ↩ to save, ⌘V to paste."),
            )
            .child(
                div()
                    .w(px(480.0))
                    .h(px(28.0))
                    .px_2()
                    .border_1()
                    .border_color(theme::accent())
                    .bg(theme::panel_bg())
                    .child(SharedString::from(format!("{}▏", input.masked()))),
            )
            .children(
                error
                    .clone()
                    .map(|e| div().text_color(theme::error()).child(e)),
            )
    }
}

pub fn render_status(status: &Status) -> Option<Div> {
    let (color, message) = match status {
        Status::Idle => return None,
        Status::Loading(message) => (theme::muted(), message),
        Status::Failed(message) => (theme::error(), message),
    };
    Some(div().px_3().py_1().text_color(color).child(message.clone()))
}

pub fn title_bar(title: &str) -> Div {
    div()
        .px_3()
        .py_2()
        .bg(theme::header_bg())
        .border_b_1()
        .border_color(theme::border())
        .whitespace_nowrap()
        .overflow_hidden()
        .child(title.to_owned())
}

pub fn hint_bar(text: impl Into<SharedString>) -> Div {
    div()
        .px_3()
        .py_1()
        .border_t_1()
        .border_color(theme::border())
        .text_color(theme::muted())
        .text_sm()
        .child(text.into())
}

fn render_pr_row(ix: usize, row: &PrListRow<'_>) -> gpui::Stateful<Div> {
    let base = div()
        .id(ix)
        .h(px(24.0))
        .px_3()
        .flex()
        .items_center()
        .whitespace_nowrap()
        .overflow_hidden();
    match row {
        PrListRow::Header(group, count) => base
            .bg(theme::header_bg())
            .text_color(theme::accent())
            .child(format!("{} ({count})", group.title())),
        PrListRow::Empty(_) => base.text_color(theme::muted()).child("  (none)"),
        PrListRow::Pr { pr, selected, .. } => {
            let draft = if pr.is_draft { " [draft]" } else { "" };
            base.when(*selected, |d| d.bg(theme::selection()))
                .gap_2()
                .child(div().text_color(theme::muted()).child(pr.pr.to_string()))
                .child(format!("{}{draft}", pr.title))
                .child(div().text_color(theme::muted()).child(format!(
                    "@{} +{} -{}",
                    pr.author, pr.additions, pr.deletions
                )))
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.screen {
            Screen::Starting => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::muted())
                .child("Starting…"),
            Screen::TokenEntry { input, error } => Self::render_token_entry(input, error),
            Screen::PrList => self.render_pr_list(cx),
            Screen::Review(screen) => review::render(screen, cx),
        };
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family("Menlo")
            .text_size(px(13.0))
            .child(content)
    }
}
