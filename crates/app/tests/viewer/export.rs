//! Exporting and re-exporting to files and the clipboard.

use eframe::egui::{accesskit::Role, accesskit::Toggled};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::{Command, NotebookApp, SavePath, t};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use crate::common::*;

#[test]
fn export_waits_for_every_page_in_scope() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    click_command(&harness, Command::ExportAs);
    settle(&mut harness);
    harness.get_by_label(&t!("export-everything")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("export-not-transcribed", names = "b.png"));
    assert!(
        harness
            .get_by_role_and_label(Role::Button, &t!("export-go"))
            .accesskit_node()
            .is_disabled()
    );

    harness.get_by_label(&t!("export-current-page")).click();
    settle(&mut harness);
    let blocked = before_mark(t!("export-not-transcribed", names = MARK));
    assert!(harness.query_by_label_contains(&blocked).is_none());
    assert!(
        !harness
            .get_by_role_and_label(Role::Button, &t!("export-go"))
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn export_waits_for_pages_to_load() {
    let gate = Arc::new(RwLock::new(()));
    let held = gate.write().unwrap();
    let app = NotebookApp::with_transcription(Some(transcriber()), None)
        .with_loader(gated_loader(gate.clone()));
    let mut harness = harness_for(app);
    start_dropping(&mut harness, &[("a.png", page_photo())]);
    // Transcription works from the file, so it doesn't wait.
    show_transcript(&mut harness);

    click_command(&harness, Command::ExportAs);
    harness.step();
    harness.step();
    harness.get_by_label(&t!("export-still-loading", names = "a.png"));
    assert!(
        harness
            .get_by_role_and_label(Role::Button, &t!("export-go"))
            .accesskit_node()
            .is_disabled()
    );

    drop(held);
    settle(&mut harness);
    let blocked = before_mark(t!("export-still-loading", names = MARK));
    assert!(harness.query_by_label_contains(&blocked).is_none());
    assert!(
        !harness
            .get_by_role_and_label(Role::Button, &t!("export-go"))
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn exports_markdown_to_the_clipboard() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    correct_plumber(&mut harness);

    let copied = export_to_clipboard(&mut harness);
    assert!(
        copied.starts_with("<!-- a.png -->\n# Thurs 23 Oct\n- ring the plumbers re:"),
        "{copied}"
    );
    harness.get_by_label(&t!("export-copied", count = 1, name = "a.png"));
    settle(&mut harness);
    assert!(!disabled(&harness, &t!("command-reexport")));
}

#[test]
fn reexport_overwrites_the_same_file() {
    let dir = test_dir("reexport");
    let path = dir.path().join("a.md");
    let asked = Arc::new(AtomicUsize::new(0));
    let save_path: SavePath = {
        let (asked, path) = (asked.clone(), path.clone());
        Arc::new(move |suggested| {
            assert_eq!(suggested, "a.md");
            asked.fetch_add(1, Ordering::SeqCst);
            Some(path.clone())
        })
    };
    let mut harness = harness_for(
        NotebookApp::with_transcription(Some(transcriber()), None).with_save_path(save_path),
    );
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    assert!(disabled(&harness, &t!("command-reexport")));

    export(
        &mut harness,
        &[&t!("export-current-page"), &t!("export-markdown-file")],
    );
    step_until(&mut harness, &exported(1, "a.png"));
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("ring the plumber re:"), "{written}");

    correct_plumber(&mut harness);
    harness.get_by_label(&t!("command-reexport")).click();
    // The frame that starts it still shows the first export's status.
    harness.step();
    step_until(&mut harness, &exported(1, "a.png"));
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("ring the plumbers re:"), "{written}");
    assert_eq!(asked.load(Ordering::SeqCst), 1, "asked where to save once");
}

#[test]
fn time_in_the_save_dialog_isnt_a_slow_frame() {
    // Cancelled.
    let save_path: SavePath = Arc::new(|_| None);
    let mut harness = harness_for(
        NotebookApp::with_transcription(Some(transcriber()), None).with_save_path(save_path),
    );
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    assert_eq!(
        harness.state_mut().frame_time_for_logs(Some(0.2)),
        Some(0.2)
    );
    assert_eq!(
        harness.state_mut().frame_time_for_logs(Some(0.01)),
        None,
        "fast"
    );

    export(&mut harness, &[&t!("export-markdown-clipboard")]);
    step_until(
        &mut harness,
        &t!("export-copied", count = 1, name = "a.png"),
    );
    assert_eq!(
        harness.state_mut().frame_time_for_logs(Some(0.2)),
        Some(0.2),
        "no dialog"
    );

    export(&mut harness, &[&t!("export-markdown-file")]);
    settle(&mut harness);
    assert_eq!(harness.state_mut().frame_time_for_logs(Some(5.0)), None);
    assert_eq!(
        harness.state_mut().frame_time_for_logs(Some(0.2)),
        Some(0.2),
        "only the frame that opened it"
    );
}

#[test]
fn export_defaults_to_everything_as_markdown_on_the_clipboard() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    click_command(&harness, Command::ExportAs);
    settle(&mut harness);
    let options: Vec<String> = harness
        .get_all_by_role(Role::RadioButton)
        .map(|node| {
            let toggled = node.accesskit_node().toggled() == Some(Toggled::True);
            let label = node.accesskit_node().label().unwrap_or_default();
            format!("{}{label}", if toggled { "* " } else { "" })
        })
        .collect();
    assert_eq!(
        options,
        [
            format!("* {}", t!("export-everything")),
            t!("export-current-page"),
            format!("* {}", t!("export-markdown-clipboard")),
            t!("export-markdown-file"),
            t!("export-pdf"),
        ]
    );
}

#[test]
fn exports_a_pdf() {
    let dir = test_dir("pdf");
    let path = dir.path().join("notebook.pdf");
    let save_path: SavePath = {
        let path = path.clone();
        Arc::new(move |suggested| {
            assert_eq!(suggested, format!("{}.pdf", t!("export-default-name")));
            Some(path.clone())
        })
    };
    let mut harness = harness_for(
        NotebookApp::with_transcription(Some(transcriber()), None).with_save_path(save_path),
    );
    drop_files(
        &mut harness,
        &[("a.png", page_photo()), ("b.png", page_photo())],
    );
    click_command(&harness, Command::TranscribeAll);
    step_until_transcribed(&mut harness);

    export(&mut harness, &[&t!("export-everything"), &t!("export-pdf")]);
    step_until(&mut harness, &exported(2, ""));
    let written = std::fs::read(&path).unwrap();
    assert!(written.starts_with(b"%PDF"));
    assert!(!disabled(&harness, &t!("command-reexport")));
}
