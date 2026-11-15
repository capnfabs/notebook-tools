//! Correcting uncertain words, deleting struck text and editing lines.

use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::{Command, Event, Fetch, NotebookApp, Transcriber, t};
use std::sync::Arc;

use crate::common::*;

#[test]
fn transcribed_view_shows_the_page_with_uncertain_words() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.get_by_label(&t!("current-page-cleaned", name = "a.png"));
    for word in ["plumber", "w/"] {
        harness.get_by_label(word);
        harness.get_by_label(&t!("uncertain-box", word = word));
    }
}

#[test]
fn corrections_are_logged() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    correct_plumber(&mut harness);
    harness.get_by_label("w/").click();
    settle(&mut harness);
    harness.get_by_label(&t!("keep-word", word = "w/")).click();
    settle(&mut harness);
    // A word corrected again doesn't finish the page again.
    harness
        .get_by_label(&t!("uncertain-box", word = "plumbers"))
        .click();
    settle(&mut harness);
    let field = harness.get_by_role(Role::TextInput);
    field.focus();
    field.type_text("Plumbing Co");
    harness.run_ok();
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);

    assert_eq!(
        events(&harness),
        [
            "transcribed from_cache=false",
            "corrected 0 plumber->plumbers Alternative(0)",
            "corrected 1 w/->w/ Keep",
            "all resolved 2 unfound=0",
            "corrected 0 plumbers->Plumbing Co Custom",
        ]
    );
    let Event::Corrected { page, model, .. } = &harness.state().eval_log().events()[1] else {
        panic!("not a correction");
    };
    assert_eq!(
        (page.name.as_str(), model.as_str()),
        ("a.png", "claude-opus-5-5")
    );
}

#[test]
fn picking_an_alternative_corrects_the_word() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    correct_plumber(&mut harness);
    harness.get_by_label("plumbers");
    harness.get_by_label(&t!("uncertain-box", word = "plumbers"));
    assert!(harness.query_by_label("plumber").is_none());

    // The replaced reading is still on offer.
    harness.get_by_label("plumbers").click();
    settle(&mut harness);
    harness.get_by_label(&t!("keep-word", word = "plumbers"));
    harness.get_by_label("plumber").click();
    settle(&mut harness);
    harness.get_by_label(&t!("uncertain-box", word = "plumber"));
}

#[test]
fn boxes_take_a_reading_of_your_own() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness
        .get_by_label(&t!("uncertain-box", word = "plumber"))
        .click();
    settle(&mut harness);
    let field = harness.get_by_role(Role::TextInput);
    field.focus();
    field.type_text("Plumbing Co");
    harness.run_ok();
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);

    harness.get_by_label("Plumbing Co");
    assert!(
        harness.query_by_label_contains("Keep").is_none(),
        "popup closed"
    );
}

#[test]
fn keeping_a_reading_closes_the_popup() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.get_by_label("w/").click();
    settle(&mut harness);
    harness.get_by_label(&t!("keep-word", word = "w/")).click();
    settle(&mut harness);

    harness.get_by_label("w/");
    assert!(harness.query_by_label_contains("Keep").is_none());
}

#[test]
fn double_clicking_a_line_edits_it() {
    let mut harness = quick_harness();
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    // Far enough after the click on Transcribed that the double-click isn't a triple.
    harness.run_steps(30);
    let line = harness.get_by_label("# Thurs 23 Oct");
    line.click();
    line.click();
    harness.run_ok();
    let field = harness.get_by_role(Role::TextInput);
    assert_eq!(field.value().as_deref(), Some("# Thurs 23 Oct"));
    harness.key_press(egui::Key::End);
    field.type_text(" 2026");
    harness.run_ok();
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);

    harness.get_by_label("# Thurs 23 Oct 2026");
    assert!(harness.query_by_role(Role::TextInput).is_none());
    assert_eq!(
        events(&harness)[1..],
        ["line 0 # Thurs 23 Oct-># Thurs 23 Oct 2026"]
    );

    // Escape drops the edit.
    harness.run_steps(30);
    let line = harness.get_by_label("# Thurs 23 Oct 2026");
    line.click();
    line.click();
    harness.run_ok();
    harness.get_by_role(Role::TextInput).type_text("!!");
    harness.run_ok();
    harness.key_press(egui::Key::Escape);
    settle(&mut harness);
    harness.get_by_label("# Thurs 23 Oct 2026");
    assert_eq!(events(&harness).len(), 2, "escaped edit not logged");
}

#[test]
fn clicking_another_correction_keeps_its_popup_open() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    // The lower word first: its popup opens below it, clear of the other.
    harness.get_by_label("w/").click();
    settle(&mut harness);
    harness.get_by_label(&t!("keep-word", word = "w/"));
    harness.get_by_label("plumber").click();
    settle(&mut harness);
    harness.get_by_label(&t!("keep-word", word = "plumber"));
    assert!(
        harness
            .query_by_label(&t!("keep-word", word = "w/"))
            .is_none()
    );

    // And from the page.
    harness
        .get_by_label(&t!("uncertain-box", word = "w/"))
        .click();
    settle(&mut harness);
    harness.get_by_label(&t!("keep-word", word = "w/"));
    assert!(
        harness
            .query_by_label(&t!("keep-word", word = "plumber"))
            .is_none()
    );
}

#[test]
fn clicking_a_box_scrolls_the_transcript_to_its_word() {
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
    let (heading, word) = (top(&harness, "# Thurs 23 Oct"), top(&harness, "w/"));
    assert!(word > HEIGHT, "w/ starts out of sight at {word}");

    harness
        .get_by_label(&t!("uncertain-box", word = "w/"))
        .click();
    settle(&mut harness);
    let scrolled = heading - top(&harness, "# Thurs 23 Oct");
    assert!(scrolled > 0.0, "scrolled {scrolled}");
    assert!(top(&harness, "w/") < HEIGHT);
    harness.get_by_label(&t!("keep-word", word = "w/"));
}

#[test]
fn hovering_a_line_highlights_its_box() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    assert!(!line_box_outlined(&harness));

    harness.get_by_label(" shower").hover();
    harness.run_ok();
    assert!(line_box_outlined(&harness));
    harness.get_by_label("# Thurs 23 Oct").hover();
    harness.run_ok();
    assert!(!line_box_outlined(&harness), "only the hovered line");
}

#[test]
fn a_single_click_hints_at_double_click() {
    let hint = t!("edit-hint");
    let mut harness = quick_harness();
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.run_steps(30);
    harness.get_by_label("# Thurs 23 Oct").click();
    harness.run_steps(5);
    assert!(
        harness.query_by_label(&hint).is_none(),
        "not while it could be a double-click"
    );
    harness.run_steps(30);
    harness.get_by_label(&hint);
    // Gone once the pointer moves away.
    harness
        .get_by_label(&t!("uncertain-box", word = "plumber"))
        .hover();
    harness.run_steps(2);
    assert!(harness.query_by_label(&hint).is_none());

    edit_line(&mut harness, "# Thurs 23 Oct");
    harness.run_steps(30);
    harness.get_by_role(Role::TextInput);
    assert!(
        harness.query_by_label(&hint).is_none(),
        "not for a double-click"
    );
}

#[test]
fn struck_text_can_be_deleted() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.get_by_label("~~tap~~").click();
    settle(&mut harness);
    harness.get_by_label(&t!("delete-struck")).click();
    settle(&mut harness);

    harness.get_by_label(" re: shower");
    assert!(harness.query_by_label("~~tap~~").is_none());
    assert_eq!(
        events(&harness)[1..],
        ["line 1 - ring the plumber re: ~~tap~~ shower->- ring the plumber re: shower"]
    );
}

#[test]
fn struck_uncertain_words_can_be_deleted_and_put_back() {
    // Claude unsure of "tap" in "~~tap~~", rather than of "plumber".
    let fetch: Fetch = Arc::new(|_, _, _, _| {
        let mut fetched = recorded();
        let body = String::from_utf8(fetched.body)
            .unwrap()
            .replace(r#"\"plumber\", \"bbox\""#, r#"\"tap\", \"bbox\""#)
            .replace(r#"[\"plumbers\", \"plunber\"]"#, r#"[\"top\"]"#);
        fetched.body = body.into_bytes();
        Ok(fetched)
    });
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.get_by_label("~~tap~~").click();
    settle(&mut harness);
    harness.get_by_label("~~top~~");
    harness.get_by_label(&t!("keep-word", word = "~~tap~~"));
    harness.get_by_label(&t!("delete-struck")).click();
    settle(&mut harness);
    harness.get_by_label("- ring the plumber re: shower");

    harness
        .get_by_label(&t!("uncertain-box", word = "~~tap~~"))
        .click();
    settle(&mut harness);
    assert!(
        harness.query_by_label_contains("Keep").is_none(),
        "nothing to keep"
    );
    harness.get_by_label("~~top~~").click();
    settle(&mut harness);
    harness.get_by_label("- ring the plumber re: ");
    harness.get_by_label("~~top~~");
    harness.get_by_label(" shower");
    assert_eq!(
        events(&harness)[1..],
        [
            "corrected 0 ~~tap~~-> DeleteStruck",
            "corrected 0 ->~~top~~ Alternative(1)",
        ]
    );
}

#[test]
fn unfound_boxes_say_why() {
    let mut harness = quick_harness();
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    edit_line(&mut harness, " shower");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_role(Role::TextInput).type_text("- call Sam");
    harness.run_ok();
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);
    harness.get_by_label("- call Sam");

    harness
        .get_by_label(&t!("uncertain-box", word = "plumber"))
        .hover();
    harness.run_steps(60);
    harness.get_by_label(&t!("unfound-edited-out", word = "plumber"));
}

#[test]
fn reset_asks_then_restores_the_transcript() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    assert!(disabled(&harness, &Command::ResetTranscript.label()));

    correct_plumber(&mut harness);
    click_command(&harness, Command::ResetTranscript);
    settle(&mut harness);
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);
    harness.get_by_label("plumbers");

    click_command(&harness, Command::ResetTranscript);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-reset-button")).click();
    settle(&mut harness);
    harness.get_by_label("plumber");
    assert!(harness.query_by_label("plumbers").is_none());
    assert!(disabled(&harness, &Command::ResetTranscript.label()));
    assert_eq!(events(&harness).last().map(String::as_str), Some("reset"));
}
