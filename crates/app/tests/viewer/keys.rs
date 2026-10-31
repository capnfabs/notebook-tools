//! Keyboard shortcuts and commands.

use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::i18n::ocr_error;
use notebook_app::{Command, NotebookApp, PickFiles, SavePath, Transcriber, t};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::*;

#[test]
fn command_numbers_switch_the_view() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    press_command(&mut harness, egui::Key::Num2);
    harness.get_by_label(&t!("current-page-cleaned", name = "a.png"));
    press_command(&mut harness, egui::Key::Num1);
    harness.get_by_label(&t!("current-page", name = "a.png"));
    press_command(&mut harness, egui::Key::Num3);
    step_until(&mut harness, "# Thurs 23 Oct");

    press_command(&mut harness, egui::Key::Num1);
    press_command(&mut harness, egui::Key::Num3);
    harness.get_by_label_contains("# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn command_backspace_deletes_the_page() {
    let mut harness = harness_with(transcriber());
    drop_files(
        &mut harness,
        &[("a.png", page_photo()), ("b.png", png()), ("c.png", png())],
    );
    press_command(&mut harness, egui::Key::Backspace);
    assert_eq!(thumbnails(&harness), ["b.png", "c.png"]);

    // A transcribed page asks first, and nothing else happens until it's answered.
    show_transcript(&mut harness);
    press_command(&mut harness, egui::Key::Backspace);
    harness.get_by_label(&t!("confirm-delete", name = "b.png"));
    press(&mut harness, egui::Modifiers::NONE, egui::Key::ArrowRight);
    press_command(&mut harness, egui::Key::Num1);
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["b.png", "c.png"]);
    harness.get_by_label(&t!("current-page-cleaned", name = "b.png"));
}

#[test]
fn command_t_transcribes_or_says_how_to_retranscribe() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    press_command(&mut harness, egui::Key::T);
    step_until(&mut harness, "# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    press_command(&mut harness, egui::Key::T);
    let hint = before_mark(t!("already-transcribed", name = "a.png", shortcut = MARK));
    harness.get_by_label_contains(&hint);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // It goes after a few seconds.
    harness.run_steps(20);
    assert!(harness.query_by_label_contains(&hint).is_none());
}

#[test]
fn command_t_retries_a_failed_page() {
    let answers = vec![Err(overloaded()), Ok(recorded())];
    let (fetch, calls) = answering(answers);
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    press_command(&mut harness, egui::Key::T);
    let failed = t!(
        "transcribe-failed",
        name = "a.png",
        error = ocr_error(&overloaded())
    );
    step_until(&mut harness, &failed);
    press_command(&mut harness, egui::Key::T);
    step_until(&mut harness, "# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn command_shift_t_forces_retranscription() {
    let answers = vec![Ok(recorded()), Ok(recorded_fixture("fallback"))];
    let (fetch, calls) = answering(answers);
    let dir = test_dir("command_shift_t");
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    press(&mut harness, COMMAND_SHIFT, egui::Key::T);
    step_until(&mut harness, &by("claude-opus-4-8"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    // A corrected page asks first.
    correct_plumber(&mut harness);
    press(&mut harness, COMMAND_SHIFT, egui::Key::T);
    harness.get_by_label(&t!("confirm-force", name = "a.png"));
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn command_e_exports_then_reexports() {
    let dir = test_dir("command_e");
    let path = dir.path().join("a.md");
    let asked = Arc::new(AtomicUsize::new(0));
    let save_path: SavePath = {
        let (asked, path) = (asked.clone(), path.clone());
        Arc::new(move |_| {
            asked.fetch_add(1, Ordering::SeqCst);
            Some(path.clone())
        })
    };
    let mut harness = harness_for(
        NotebookApp::with_transcription(Some(transcriber()), None).with_save_path(save_path),
    );
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    // Nothing to repeat yet, so it asks.
    press_command(&mut harness, egui::Key::E);
    for option in [t!("export-current-page"), t!("export-markdown-file")] {
        harness.get_by_label(&option).click();
        settle(&mut harness);
    }
    harness
        .get_by_role_and_label(Role::Button, &t!("export-go"))
        .click();
    step_until(&mut harness, &exported(1, "a.png"));

    correct_plumber(&mut harness);
    press_command(&mut harness, egui::Key::E);
    assert!(
        harness
            .query_by_role_and_label(Role::Button, &t!("export-go"))
            .is_none()
    );
    step_until(&mut harness, &exported(1, "a.png"));
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("ring the plumbers re:"), "{written}");
    assert_eq!(asked.load(Ordering::SeqCst), 1);

    // Export… always asks.
    press(&mut harness, COMMAND_SHIFT, egui::Key::E);
    harness.get_by_role_and_label(Role::Button, &t!("export-go"));
}

#[test]
fn add_pages_adds_the_picked_files() {
    let dir = test_dir("add_pages");
    let paths = vec![dir.path().join("b.png"), dir.path().join("a.png")];
    for path in &paths {
        std::fs::write(path, png()).unwrap();
    }
    let pick_files: PickFiles = Arc::new(move || paths.clone());
    let mut harness = harness_for(NotebookApp::default().with_file_picker(pick_files));

    press(&mut harness, COMMAND_SHIFT, egui::Key::I);
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);
    assert_eq!(harness.state_mut().frame_time_for_logs(Some(1.0)), None);
}

#[test]
fn editing_keys_stay_with_the_text_field() {
    let mut harness = quick_harness();
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);
    harness.run_steps(30);
    let line = harness.get_by_label("# Thurs 23 Oct");
    line.click();
    line.click();
    harness.run_ok();
    harness.get_by_role(Role::TextInput);

    press(&mut harness, egui::Modifiers::NONE, egui::Key::ArrowRight);
    press_command(&mut harness, egui::Key::Backspace);
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);
    harness.get_by_label(&t!("current-page-cleaned", name = "a.png"));
    harness.get_by_role(Role::TextInput);
}

#[test]
fn enter_picks_the_affirmative_button() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    let names = ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"];
    let mut files: Vec<_> = names.iter().map(|n| (*n, png())).collect();
    files[0].1 = page_photo();
    drop_files(&mut harness, &files);
    let enter = |harness: &mut Harness<'_, NotebookApp>| {
        harness.key_press(egui::Key::Enter);
        settle(harness);
    };

    click_command(&harness, Command::TranscribeAll);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-transcribe-all", count = 6));
    enter(&mut harness);
    step_until_transcribed(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 6);

    // Dangerous ones too.
    show_transcript(&mut harness);
    correct_plumber(&mut harness);
    click_command(&harness, Command::ForceRetranscribe);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-force", name = "a.png"));
    enter(&mut harness);
    step_until_transcribed(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 7);

    click_command(&harness, Command::ExportAs);
    settle(&mut harness);
    enter(&mut harness);
    step_until(&mut harness, &t!("export-copied", count = 6, name = ""));

    click_command(&harness, Command::Delete);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-delete", name = "a.png"));
    enter(&mut harness);
    assert_eq!(thumbnails(&harness), &names[1..]);
}
