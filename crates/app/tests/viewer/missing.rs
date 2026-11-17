//! Pages whose photos are missing when their project opens: shown in their place, their
//! transcripts readable and correctable, but not transcribed or cropped again. See
//! `decisions/2026-11-15-missing-pages-shown.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::{Command, NotebookApp, SavePath, Transcriber, t};
use notebook_core::project;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::*;

/// A project, `notes.notebook`, of a.png, b.png (transcribed) and c.png, three different photos,
/// saved and then opened again once b.png and c.png have gone.
struct Opened {
    dir: TempDir,
    path: PathBuf,
    /// The project file as it was saved, before the photos went.
    before: Value,
    harness: Harness<'static, NotebookApp>,
    /// How many pages the reopened app has sent to Claude.
    sent: Arc<AtomicUsize>,
}

fn saved(path: &Path) -> Value {
    serde_json::from_slice(&project::json(path).unwrap()).unwrap()
}

fn opened_without_b_and_c(test: &str) -> Opened {
    let dir = test_dir(test);
    let path = dir.path().join("notes.notebook");
    let app =
        NotebookApp::with_transcription(Some(transcriber()), None).with_save_path(save_to(&path));
    let mut first = harness_for(app);
    let photos = [("a.png", png()), ("b.png", page_photo()), ("c.png", png())];
    drop_files_in(&mut first, dir.path(), &photos);
    press_command(&mut first, Key::ArrowRight);
    show_transcript(&mut first);
    press_command(&mut first, Key::S);
    let before = saved(&path);
    for gone in ["b.png", "c.png"] {
        std::fs::remove_file(dir.path().join(gone)).unwrap();
    }

    let (fetch, sent) = counting_fetch();
    // Markdown exports go to `notes.md` beside it, rather than over the project.
    let save_path: SavePath = {
        let (path, markdown) = (path.clone(), dir.path().join("notes.md"));
        Arc::new(move |suggested| {
            let to = if suggested.ends_with(".md") {
                &markdown
            } else {
                &path
            };
            Some(to.clone())
        })
    };
    let app = NotebookApp::with_transcription(Some(Transcriber::new(fetch)), None)
        .with_save_path(save_path)
        .with_project(&path, project::open(&path).unwrap());
    let mut harness = harness_for(app);
    settle(&mut harness);
    Opened {
        dir,
        path,
        before,
        harness,
        sent,
    }
}

fn title(harness: &Harness<'_, NotebookApp>) -> String {
    harness.state().window_title()
}

fn not_found(dir: &Path, name: &str) -> String {
    let path = dir.join(name).display().to_string();
    t!("photo-not-found", path = path)
}

#[test]
fn missing_pages_are_shown_in_their_place() {
    let Opened {
        dir,
        path,
        before,
        mut harness,
        ..
    } = opened_without_b_and_c("shown");
    assert_eq!(thumbnails(&harness), ["a.png", "b.png", "c.png"]);
    let summary = t!("photos-missing", count = 2, names = "b.png, c.png");
    assert!(harness.query_by_label(&summary).is_some());
    assert_eq!(title(&harness), "notes", "nothing changed");

    press_command(&mut harness, Key::ArrowRight);
    assert!(harness.query_by_label(&t!("missing-photo-title")).is_some());
    let why = not_found(dir.path(), "b.png");
    assert!(harness.query_by_label(&why).is_some());

    // In the Cleaned view, once rather than on both sides, with the corners left alone.
    click_command(&harness, Command::ViewCleaned);
    settle(&mut harness);
    assert_eq!(harness.query_all_by_label(&why).count(), 1);
    assert!(disabled(&harness, &Command::PlaceCorners.label()));
    assert!(disabled(&harness, &Command::ResetCorners.label()));

    press(&mut harness, COMMAND_SHIFT, Key::S);
    assert_eq!(saved(&path), before, "saved again as it was");
}

#[test]
fn a_changed_photo_says_so() {
    let Opened { dir, path, .. } = opened_without_b_and_c("changed");
    std::fs::write(dir.path().join("a.png"), "another photo").unwrap();
    let mut harness =
        harness_for(NotebookApp::default().with_project(&path, project::open(&path).unwrap()));
    settle(&mut harness);
    let summary = t!("photos-missing", count = 3, names = "a.png, b.png, c.png");
    assert!(harness.query_by_label(&summary).is_some());
    let changed = dir.path().join("a.png").display().to_string();
    let why = t!("photo-changed", path = changed);
    assert!(harness.query_by_label(&why).is_some());
}

#[test]
fn a_missing_pages_transcript_can_be_corrected() {
    let Opened {
        path,
        mut harness,
        sent,
        ..
    } = opened_without_b_and_c("corrected");
    press_command(&mut harness, Key::ArrowRight);
    show_transcript(&mut harness);
    assert!(harness.query_by_label(&t!("missing-photo-title")).is_some());
    assert!(disabled(&harness, &Command::ForceRetranscribe.label()));

    correct_plumber(&mut harness);
    assert_eq!(title(&harness), t!("window-title-edited", name = "notes"));
    press_command(&mut harness, Key::S);
    let pages = project::open(&path).unwrap();
    let b = pages.iter().find(|p| p.name == "b.png").unwrap();
    assert!(b.missing_photo().is_some());
    assert!(b.transcript.as_ref().unwrap().corrected());
    assert_eq!(sent.load(Ordering::SeqCst), 0, "nothing sent");
}

#[test]
fn missing_pages_are_never_sent() {
    let Opened {
        mut harness, sent, ..
    } = opened_without_b_and_c("never_sent");
    // c.png, untranscribed: the Transcribed view doesn't transcribe it, nor does ⌘T.
    press_command(&mut harness, Key::ArrowRight);
    press_command(&mut harness, Key::ArrowRight);
    click_command(&harness, Command::ViewTranscribed);
    settle(&mut harness);
    let cant = t!("cant-transcribe-missing", name = "c.png");
    assert!(harness.query_by_label(&cant).is_some());
    press_command(&mut harness, Key::T);
    step_until_transcribed(&mut harness);
    assert_eq!(sent.load(Ordering::SeqCst), 0);

    // Only a.png is left to transcribe.
    click_command(&harness, Command::TranscribeAll);
    step_until_transcribed(&mut harness);
    assert_eq!(sent.load(Ordering::SeqCst), 1);
    assert!(disabled(&harness, &Command::TranscribeAll.label()));
}

#[test]
fn missing_pages_can_be_moved_and_deleted() {
    let Opened {
        path, mut harness, ..
    } = opened_without_b_and_c("move_delete");
    press_command(&mut harness, Key::ArrowRight);
    press(&mut harness, COMMAND_SHIFT, Key::ArrowLeft);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "c.png"]);
    press_command(&mut harness, Key::ArrowRight);
    press_command(&mut harness, Key::ArrowRight);
    // c.png has no transcript, so it goes without asking.
    press_command(&mut harness, Key::Backspace);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png"]);
    assert_eq!(title(&harness), t!("window-title-edited", name = "notes"));

    press_command(&mut harness, Key::S);
    let pages = project::open(&path).unwrap();
    let names: Vec<&str> = pages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["b.png", "a.png"]);
}

/// Opens the export dialog and chooses `options`.
fn export_dialog(harness: &mut Harness<'_, NotebookApp>, options: &[&str]) {
    click_command(harness, Command::ExportAs);
    settle(harness);
    for option in options {
        harness.get_by_label(option).click();
        settle(harness);
    }
}

/// See `decisions/2026-11-17-missing-page-export.md`.
#[test]
fn a_pdf_needs_the_photos() {
    let Opened { mut harness, .. } = opened_without_b_and_c("export_pdf");
    let everything = [t!("export-everything"), t!("export-pdf")];
    export_dialog(&mut harness, &everything.each_ref().map(String::as_str));
    // Each page once; missing pages aren't loading.
    let blocked = [
        t!("export-needs-relink", names = "b.png"),
        t!("export-needs-relink-and-transcribe", names = "c.png"),
        t!("export-not-transcribed", names = "a.png"),
    ];
    harness.get_by_label(&blocked.join(". "));
    assert!(export_go_disabled(&harness));
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);

    // a.png, the current page, has its photo.
    press_command(&mut harness, Key::T);
    step_until_transcribed(&mut harness);
    export_dialog(
        &mut harness,
        &[&t!("export-current-page"), &t!("export-pdf")],
    );
    assert!(!export_go_disabled(&harness));
}

#[test]
fn markdown_has_missing_pages_transcripts() {
    let Opened {
        dir, mut harness, ..
    } = opened_without_b_and_c("export_markdown");
    press_command(&mut harness, Key::T);
    step_until_transcribed(&mut harness);
    // c.png has no transcript, so it goes without asking.
    press_command(&mut harness, Key::ArrowRight);
    press_command(&mut harness, Key::ArrowRight);
    press_command(&mut harness, Key::Backspace);
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);

    let copied = export_to_clipboard(&mut harness);
    let (a, b) = copied.split_once("<!-- b.png -->").expect(&copied);
    assert!(a.starts_with("<!-- a.png -->"), "{copied}");
    assert!(b.contains("ring the plumber re:"), "{copied}");

    export(
        &mut harness,
        &[&t!("export-everything"), &t!("export-markdown-file")],
    );
    step_until(&mut harness, &exported(2, ""));
    let written = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    assert_eq!(written, copied);
}

#[test]
fn markdown_needs_missing_pages_transcribed() {
    let Opened { mut harness, .. } = opened_without_b_and_c("export_untranscribed");
    let options = [t!("export-everything"), t!("export-markdown-clipboard")];
    export_dialog(&mut harness, &options.each_ref().map(String::as_str));
    harness.get_by_label(&t!("export-not-transcribed", names = "a.png, c.png"));
    assert!(export_go_disabled(&harness));
}

/// Its photo, under any name, gives a missing page its photo back: see
/// `decisions/2026-11-15-page-ids.md`.
#[test]
fn a_dropped_photo_fills_in_its_missing_page() {
    let Opened {
        dir,
        path,
        mut harness,
        ..
    } = opened_without_b_and_c("found");
    let moved = dir.path().join("moved");
    std::fs::create_dir(&moved).unwrap();
    drop_files_in(&mut harness, &moved, &[("IMG_0002.png", page_photo())]);
    assert_eq!(thumbnails(&harness), ["a.png", "IMG_0002.png", "c.png"]);
    assert_eq!(selected_thumbnails(&harness), ["IMG_0002.png"]);
    let found = t!("photo-found", name = "b.png");
    assert!(harness.query_by_label(&found).is_some());
    assert!(harness.query_by_label(&t!("missing-photo-title")).is_none());
    assert_eq!(title(&harness), t!("window-title-edited", name = "notes"));

    press_command(&mut harness, Key::S);
    let pages = project::open(&path).unwrap();
    let page = pages.get(1).unwrap();
    assert_eq!(page.name, "IMG_0002.png");
    assert_eq!(page.path, Some(moved.join("IMG_0002.png")));
    assert!(page.missing_photo().is_none());
    assert!(page.transcript.is_some(), "its transcript stays");
}

#[test]
fn another_photo_named_like_a_missing_page_is_another_page() {
    let Opened { mut harness, .. } = opened_without_b_and_c("same_name");
    drop_files(&mut harness, &[("b.png", png())]);
    assert_eq!(thumbnails(&harness), ["a.png", "b.png", "b.png", "c.png"]);
    let summary = t!("photos-missing", count = 2, names = "b.png, c.png");
    assert!(
        harness.query_by_label(&summary).is_none(),
        "a drop clears it"
    );
    // The missing one is still missing.
    press_command(&mut harness, Key::ArrowRight);
    assert!(harness.query_by_label(&t!("missing-photo-title")).is_some());
}

#[test]
fn the_placeholder_never_takes_focus() {
    let Opened {
        dir, mut harness, ..
    } = opened_without_b_and_c("focus");
    press_command(&mut harness, Key::ArrowRight);
    show_transcript(&mut harness);
    let placeholder = [t!("missing-photo-title"), not_found(dir.path(), "b.png")];
    let mut reached = Vec::new();
    for _ in 0..30 {
        press(&mut harness, Modifiers::NONE, Key::Tab);
        reached.extend(focused(&harness));
    }
    assert!(reached.iter().any(|r| r == "plumber"), "{reached:?}");
    assert!(
        !reached.iter().any(|r| placeholder.contains(r)),
        "{reached:?}"
    );
}
