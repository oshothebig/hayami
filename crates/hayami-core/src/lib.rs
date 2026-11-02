//! GPUI-independent core of hayami.
//!
//! Everything that can be tested without a window lives here: GitHub API
//! access, authentication, git operations, diff computation, the display row
//! model and the keyboard state machine.

pub mod auth;
pub mod diff;
pub mod file_tree;
pub mod git;
pub mod github;
pub mod keymap;
pub mod palette;
pub mod paths;
pub mod pr_list;
pub mod text_input;
pub mod view;

/// Application name used for window titles, cache directories and so on.
pub const APP_NAME: &str = "hayami";
