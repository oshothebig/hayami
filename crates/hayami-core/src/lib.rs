//! GPUI-independent core of hayami.
//!
//! Everything that can be tested without a window lives here: GitHub API
//! access, authentication, git operations, diff computation, the display row
//! model and the keyboard state machine.

/// Application name used for window titles, cache directories and so on.
pub const APP_NAME: &str = "hayami";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_name_is_lowercase() {
        assert_eq!(APP_NAME, APP_NAME.to_lowercase());
    }
}
