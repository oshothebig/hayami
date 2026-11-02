//! GPUI-independent core of hayami.
//!
//! Everything that can be tested without a window lives here: GitHub API
//! access, authentication, git operations, diff computation, the display row
//! model and the keyboard state machine.

pub mod auth;
pub mod github;
pub mod keymap;
pub mod pr_list;
pub mod text_input;

/// Application name used for window titles, cache directories and so on.
pub const APP_NAME: &str = "hayami";
