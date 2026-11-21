//! The review screen's flows, driven headless with the steps a reviewer takes.

use std::time::Duration;

use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

const GONE: Duration = Duration::from_secs(5);

fn start() -> Headless {
    let mut config = EngineConfig::new(Root::of::<hayami_nsh::App>());
    config.viewport = size(px(1280.0), px(800.0));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("text:/^Loaded \\d+ patch lines/").unwrap();
    app
}

#[test]
fn lists_every_file_of_the_fixture() {
    let app = start();
    let screen = app.screen();
    assert!(
        screen.shows("crates/printer/src/hyperlink.rs"),
        "{:?}",
        screen.texts()
    );
    assert!(screen.shows("tests/regression.rs"));
    let summary = screen
        .texts()
        .into_iter()
        .find(|t| t.contains(" files · +"))
        .unwrap();
    assert!(summary.starts_with("32 files"), "{summary}");
}

#[test]
fn c_opens_a_draft_enter_posts_it_with_its_place() {
    let mut app = start();
    app.press_key("j").unwrap();
    app.press_key("c").unwrap();
    app.wait_for("id:comment-input").unwrap();
    app.type_text("id:comment-input", "Needs a test").unwrap();
    app.submit("id:comment-input").unwrap();
    app.wait_for("Needs a test").unwrap();
    app.wait_for_gone("id:comment-input", GONE).unwrap();
    let screen = app.screen();
    let place = screen
        .texts()
        .into_iter()
        .find(|t| t.starts_with("You · "))
        .unwrap();
    assert!(place.contains(":L"), "{place}");
    assert!(screen.matches("text:/1 comments$/").unwrap());
}

#[test]
fn escape_cancels_the_draft() {
    let mut app = start();
    app.press_key("c").unwrap();
    app.wait_for("id:comment-input").unwrap();
    app.type_text("id:comment-input", "never mind").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:comment-input", GONE).unwrap();
    assert!(app.screen().shows("Comment canceled"));
    assert!(!app.screen().shows("never mind"));
}

#[test]
fn j_typed_in_the_draft_is_text_not_navigation() {
    let mut app = start();
    app.press_key("c").unwrap();
    app.wait_for("id:comment-input").unwrap();
    app.type_text("id:comment-input", "jk ok").unwrap();
    assert_eq!(
        app.screen().input_text("id:comment-input").unwrap(),
        Some("jk ok")
    );
}

#[test]
fn shift_j_moves_to_the_next_file() {
    let mut app = start();
    app.press_key("shift+j").unwrap();
    app.press_key("c").unwrap();
    app.wait_for("id:comment-input").unwrap();
    let status = app
        .screen()
        .texts()
        .into_iter()
        .find(|t| t.starts_with("Commenting on "))
        .unwrap();
    assert!(status.contains(".gitignore:L"), "{status}");
}
