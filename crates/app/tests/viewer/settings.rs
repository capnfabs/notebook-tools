//! The Settings dialog, and other dialogs.

use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::{Command, Fetch, NotebookApp, t};
use notebook_core::claude_transcribe::Fetched;
use notebook_core::llm_model::Model;
use notebook_core::settings::Settings;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::*;

/// macOS order: the affirmative button rightmost, after Cancel.
#[test]
fn dialogs_put_the_affirmative_button_last() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    let left = |harness: &Harness<'_, NotebookApp>, label: &str| {
        let node = harness.get_by_role_and_label(Role::Button, label);
        node.accesskit_node().bounding_box().unwrap().x0
    };

    click_command(&harness, Command::ExportAs);
    settle(&mut harness);
    let cancel = t!("dialog-cancel");
    assert!(left(&harness, &cancel) < left(&harness, &t!("export-go")));
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);

    click_command(&harness, Command::Delete);
    settle(&mut harness);
    assert!(left(&harness, &cancel) < left(&harness, &t!("confirm-delete-button")));
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);

    press_command(&mut harness, egui::Key::Comma);
    assert!(left(&harness, &cancel) < left(&harness, &t!("settings-save")));
}

#[test]
fn settings_open_without_pages_and_cancelling_keeps_them() {
    let mut harness = harness();
    press_command(&mut harness, egui::Key::Comma);
    assert!(checked(&harness, &t!("model-opus")));

    harness.get_by_label(&t!("model-sonnet")).click();
    press(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    assert!(
        harness.query_by_label(&t!("settings-save")).is_none(),
        "closed"
    );
    press_command(&mut harness, egui::Key::Comma);
    assert!(checked(&harness, &t!("model-opus")), "Escape kept Opus");

    harness.get_by_label(&t!("model-sonnet")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);
    press_command(&mut harness, egui::Key::Comma);
    assert!(checked(&harness, &t!("model-opus")), "Cancel kept Opus");
}

#[test]
fn saved_settings_are_kept_for_next_time() {
    let dir = test_dir("settings_file");
    let path = dir.path().join("settings.json");
    let app = || harness_for(NotebookApp::default().with_settings_file(path.clone()));

    let mut harness = app();
    press_command(&mut harness, egui::Key::Comma);
    harness.get_by_label(&t!("model-sonnet")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("settings-save")).click();
    settle(&mut harness);
    assert!(
        harness.query_by_label(&t!("settings-save")).is_none(),
        "closed"
    );
    assert_eq!(Settings::load(&path).model, Model::Sonnet);

    let mut harness = app();
    press_command(&mut harness, egui::Key::Comma);
    assert!(checked(&harness, &t!("model-sonnet")));
}

/// A new markdown style reaches transcripts the user hasn't corrected, read again from the cache,
/// and corrected ones once they're reset; none is sent again.
#[test]
fn a_new_markdown_style_restyles_unedited_transcripts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let fetch: Fetch = {
        let calls = calls.clone();
        Arc::new(move |_, _, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            let recorded = recorded();
            let body = String::from_utf8(recorded.body)
                .unwrap()
                .replace("buy eggs", "buy <u>eggs</u>");
            Ok(Fetched {
                body: body.into_bytes(),
                ..recorded
            })
        })
    };
    let dir = test_dir("restyle");
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(
        &mut harness,
        &[("a.png", page_photo()), ("b.png", page_photo())],
    );
    show_transcript(&mut harness);
    harness.get_by_label("b.png").click();
    settle(&mut harness);
    // From the cache, which a.png's transcription filled.
    click_command(&harness, Command::Transcribe);
    step_until(&mut harness, "buy _eggs_");
    correct_plumber(&mut harness);

    press_command(&mut harness, egui::Key::Comma);
    harness.get_by_label(&t!("settings-underlines")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("underline-asterisks")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("settings-save")).click();
    settle(&mut harness);

    harness.get_by_label("a.png").click();
    step_until(&mut harness, "buy *eggs*");
    harness.get_by_label("b.png").click();
    settle(&mut harness);
    harness.get_by_label_contains("buy _eggs_");
    harness.get_by_label("plumbers");

    click_command(&harness, Command::ResetTranscript);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-reset-button")).click();
    step_until(&mut harness, "buy *eggs*");
    harness.get_by_label("plumber");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let transcribed = events(&harness)
        .into_iter()
        .filter(|e| e.starts_with("transcribed"))
        .collect::<Vec<_>>();
    assert_eq!(
        transcribed,
        [
            "transcribed from_cache=false",
            "transcribed from_cache=true"
        ]
    );
}

#[test]
fn enter_does_nothing_while_the_affirmative_button_is_disabled() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    click_command(&harness, Command::ExportAs);
    settle(&mut harness);
    harness.get_by_label(&t!("export-not-transcribed", names = "b.png"));
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, &t!("export-go"));
    let copied = before_mark(t!("export-copied", count = MARK, name = MARK));
    assert!(harness.query_by_label_contains(&copied).is_none());
}

#[test]
fn enter_is_the_focused_widgets() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    click_command(&harness, Command::Delete);
    settle(&mut harness);
    harness.get_by_label(&t!("dialog-cancel")).focus();
    settle(&mut harness);
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);
    let ask = t!("confirm-delete", name = "a.png");
    assert!(harness.query_by_label(&ask).is_none());
    assert_eq!(thumbnails(&harness), ["a.png"]);
}
