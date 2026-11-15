//! Keyboard focus: Tab and the arrows move it through the UI, except where something needs the
//! arrows itself, and ⌘←/→ change pages whatever has it. See
//! `decisions/2026-10-31-keyboard-focus.md`.

use eframe::egui;
use eframe::egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::{NotebookApp, t};

use crate::common::*;

#[test]
fn thumbnails_never_take_focus() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);

    let mut reached = Vec::new();
    for _ in 0..30 {
        press(&mut harness, Modifiers::NONE, Key::Tab);
        let thumbnail = harness
            .query_all_by_role(Role::Image)
            .find(|node| node.is_focused());
        assert!(thumbnail.is_none(), "after {reached:?}");
        reached.extend(focused(&harness));
    }
    assert!(!reached.is_empty(), "Tab reached something");
}

#[test]
fn arrows_move_the_focus_and_command_arrows_change_pages() {
    let mut harness = harness_with(transcriber());
    drop_files(
        &mut harness,
        &[("a.png", page_photo()), ("b.png", another_page_photo())],
    );
    show_transcript(&mut harness);
    tab_to(&mut harness, "- ring the ");

    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    assert_eq!(focused(&harness).as_deref(), Some("- [x] buy eggs"));
    press(&mut harness, Modifiers::NONE, Key::ArrowUp);
    press(&mut harness, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(focused(&harness).as_deref(), Some("plumber"));
    assert_eq!(selected_thumbnails(&harness), ["a.png"]);

    press(&mut harness, Modifiers::COMMAND, Key::ArrowRight);
    assert_eq!(selected_thumbnails(&harness), ["b.png"]);
}

#[test]
fn arrows_stay_in_a_text_field() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    tab_to(&mut harness, "- [x] buy eggs");
    press(&mut harness, Modifiers::NONE, Key::Enter);

    for key in [Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft] {
        press(&mut harness, Modifiers::NONE, key);
        assert!(harness.get_by_role(Role::TextInput).is_focused(), "{key:?}");
    }
    assert!(events(&harness)[1..].is_empty(), "nothing edited");
}

#[test]
fn arrows_stay_in_the_popup() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    tab_to(&mut harness, "plumber");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    let keep = t!("keep-word", word = "plumber");

    let mut down = Vec::new();
    for _ in 0..4 {
        press(&mut harness, Modifiers::NONE, Key::ArrowDown);
        down.extend(focused(&harness));
    }
    // The field for a reading of your own, then nowhere further: OK is off while it's empty.
    assert_eq!(down[..2], ["plunber".to_owned(), keep.clone()]);
    assert!(harness.get_by_role(Role::TextInput).is_focused());
    assert_eq!(down[2], down[3]);

    press(&mut harness, Modifiers::NONE, Key::ArrowUp);
    assert_eq!(focused(&harness), Some(keep));
    for _ in 0..3 {
        press(&mut harness, Modifiers::NONE, Key::ArrowLeft);
    }
    assert_eq!(focused(&harness).as_deref(), Some("plumbers"));
}

#[test]
fn tabbing_scrolls_the_transcript() {
    // Too short for the whole transcript.
    const HEIGHT: f32 = 110.0;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(800.0, HEIGHT))
        .build_ui_state(
            |ui, app: &mut NotebookApp| app.ui(ui),
            NotebookApp::with_transcription(Some(transcriber()), None),
        );
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    let top =
        |harness: &Harness<'_, NotebookApp>, label: &str| harness.get_by_label(label).rect().top();
    let start = top(&harness, "# Thurs 23 Oct");
    assert!(top(&harness, "w/") > HEIGHT, "w/ starts out of sight");

    tab_to(&mut harness, "w/");
    assert!(top(&harness, "w/") < HEIGHT);
    assert!(
        top(&harness, "# Thurs 23 Oct") < start,
        "heading scrolled away"
    );
    // Round to the top again.
    tab_to(&mut harness, "# Thurs 23 Oct");
    assert!(top(&harness, "# Thurs 23 Oct") >= start - 1.0);
}

#[test]
fn word_boxes_never_take_focus() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    let boxes = [
        t!("uncertain-box", word = "plumber"),
        t!("uncertain-box", word = "w/"),
    ];

    let mut reached = Vec::new();
    for _ in 0..40 {
        press(&mut harness, Modifiers::NONE, Key::Tab);
        reached.extend(focused(&harness));
    }
    assert!(reached.iter().any(|r| r == "plumber"), "{reached:?}");
    assert!(!reached.iter().any(|r| boxes.contains(r)), "{reached:?}");
}

#[test]
fn changing_page_puts_the_focus_at_the_top_of_its_transcript() {
    // Too short for the whole transcript.
    const HEIGHT: f32 = 110.0;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(800.0, HEIGHT))
        .build_ui_state(
            |ui, app: &mut NotebookApp| app.ui(ui),
            NotebookApp::with_transcription(Some(transcriber()), None),
        );
    drop_files(
        &mut harness,
        &[("a.png", page_photo()), ("b.png", another_page_photo())],
    );
    show_transcript(&mut harness);
    let heading = harness.get_by_label("# Thurs 23 Oct").rect().top();
    press_command(&mut harness, Key::ArrowRight);
    press_command(&mut harness, Key::T);
    step_until(&mut harness, "# Thurs 23 Oct");
    press_command(&mut harness, Key::ArrowLeft);
    assert_eq!(selected_thumbnails(&harness), ["a.png"]);
    tab_to(&mut harness, "w/");

    press_command(&mut harness, Key::ArrowRight);
    assert_eq!(selected_thumbnails(&harness), ["b.png"]);
    assert_eq!(focused(&harness).as_deref(), Some("# Thurs 23 Oct"));
    let top = harness.get_by_label("# Thurs 23 Oct").rect().top();
    assert!(
        top >= heading - 1.0,
        "scrolled to the top: {top} vs {heading}"
    );

    // Not when the focus was somewhere else.
    tab_to(&mut harness, &t!("command-view-original"));
    press_command(&mut harness, Key::ArrowLeft);
    assert_eq!(focused(&harness), Some(t!("command-view-original")));
}

#[test]
fn focus_highlights_the_word_and_its_line() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    let plain = text_format(&harness, "plumber").background;
    assert!(!line_box_outlined(&harness));

    tab_to(&mut harness, "plumber");
    assert_ne!(text_format(&harness, "plumber").background, plain);
    assert!(line_box_outlined(&harness));

    tab_to(&mut harness, "- [x] buy eggs");
    assert_eq!(text_format(&harness, "plumber").background, plain);
    assert!(!line_box_outlined(&harness), "only the focused line");
}

#[test]
fn enter_on_a_word_puts_the_focus_in_its_popup() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    tab_to(&mut harness, "plumber");

    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(focused(&harness).as_deref(), Some("plumbers"));
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert!(harness.query_by_label_contains("Keep").is_none(), "closed");
    assert_eq!(focused(&harness).as_deref(), Some("plumber"));

    // Picking a reading puts the focus back on the word, corrected.
    press(&mut harness, Modifiers::NONE, Key::Enter);
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(harness.query_by_label_contains("Keep").is_none(), "closed");
    assert_eq!(focused(&harness).as_deref(), Some("plumbers"));
    assert_eq!(
        events(&harness)[1..],
        ["corrected 0 plumber->plumbers Alternative(0)"]
    );
}

#[test]
fn deleting_struck_text_moves_the_focus_on() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    tab_to(&mut harness, "~~tap~~");

    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(focused(&harness), Some(t!("delete-struck")));
    press(&mut harness, Modifiers::NONE, Key::Enter);
    // What followed it, now joined to what came before.
    assert_eq!(focused(&harness).as_deref(), Some(" re: shower"));
    press(&mut harness, Modifiers::NONE, Key::Tab);
    assert_eq!(focused(&harness).as_deref(), Some("- [x] buy eggs"));
}

#[test]
fn tab_goes_round_the_popup() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    tab_to(&mut harness, "plumber");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    let keep = t!("keep-word", word = "plumber");

    let mut reached = Vec::new();
    for _ in 0..4 {
        press(&mut harness, Modifiers::NONE, Key::Tab);
        reached.extend(focused(&harness));
    }
    // The field for a reading of your own is unlabelled; OK is off while it's empty.
    assert_eq!(reached[..2], ["plunber".to_owned(), keep.clone()]);
    assert_eq!(reached[3], "plumbers", "round to the top: {reached:?}");

    press(&mut harness, Modifiers::SHIFT, Key::Tab);
    assert!(
        harness.get_by_role(Role::TextInput).is_focused(),
        "round to the bottom"
    );
    harness.get_by_label(&keep);
}

#[test]
fn editing_a_line_puts_the_focus_back_on_the_piece_it_came_from() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    // After the uncertain word and the struck text in "- ring the plumber re: ~~tap~~ shower".
    tab_to(&mut harness, " shower");

    press(&mut harness, Modifiers::NONE, Key::Enter);
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(focused(&harness).as_deref(), Some(" shower"));

    // Still there after an edit before it moves it along.
    press(&mut harness, Modifiers::NONE, Key::Enter);
    harness.key_press(Key::Home);
    harness.get_by_role(Role::TextInput).type_text("xx");
    harness.run_ok();
    press(&mut harness, Modifiers::NONE, Key::Enter);
    harness.get_by_label("xx- ring the ");
    assert_eq!(focused(&harness).as_deref(), Some(" shower"));

    // Changed, it's found by where it was.
    press(&mut harness, Modifiers::NONE, Key::Enter);
    harness.key_press(Key::End);
    harness.get_by_role(Role::TextInput).type_text("s");
    harness.run_ok();
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(focused(&harness).as_deref(), Some(" showers"));
}

#[test]
fn clicking_a_word_leaves_the_focus_alone() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.get_by_label("plumber").click();
    settle(&mut harness);
    harness.get_by_label(&t!("keep-word", word = "plumber"));
    assert_ne!(focused(&harness).as_deref(), Some("plumbers"));
}

#[test]
fn enter_on_a_line_edits_it() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    tab_to(&mut harness, "# Thurs 23 Oct");

    press(&mut harness, Modifiers::NONE, Key::Enter);
    let field = harness.get_by_role(Role::TextInput);
    assert!(field.is_focused());
    assert_eq!(field.value().as_deref(), Some("# Thurs 23 Oct"));
    harness.key_press(Key::End);
    field.type_text(" 2026");
    harness.run_ok();
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(harness.query_by_role(Role::TextInput).is_none());
    assert_eq!(focused(&harness).as_deref(), Some("# Thurs 23 Oct 2026"));

    // Escape drops an edit, and puts the focus back too.
    press(&mut harness, Modifiers::NONE, Key::Enter);
    harness.get_by_role(Role::TextInput).type_text("!!");
    harness.run_ok();
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert_eq!(focused(&harness).as_deref(), Some("# Thurs 23 Oct 2026"));
}
