//! Projects: saving and opening them, the window's title, and asking to save before closing,
//! quitting, or starting or opening another project. See `decisions/2026-10-31-projects.md`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use eframe::egui::{Key, Modifiers, ViewportCommand};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::{Command, NotebookApp, Pick, PickFiles, SavePath, Transcriber, t};
use notebook_core::project;
use serde_json::Value;

use crate::common::*;

/// Stand-ins for the save dialog and the file picker, which answer with `answer` and count what
/// they're asked.
#[derive(Clone)]
struct Dialogs {
    answer: Arc<Mutex<Option<PathBuf>>>,
    saves: Arc<AtomicUsize>,
}

impl Dialogs {
    fn answering(path: &Path) -> Self {
        Self {
            answer: Arc::new(Mutex::new(Some(path.to_path_buf()))),
            saves: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// From now on, the dialogs are cancelled.
    fn cancel(&self) {
        *self.answer.lock().unwrap() = None;
    }

    fn saves(&self) -> usize {
        self.saves.load(Ordering::SeqCst)
    }

    /// An app that transcribes with `fetch` and asks these dialogs.
    fn app(&self, fetch: notebook_app::Fetch) -> Harness<'static, NotebookApp> {
        let (answer, saves) = (self.answer.clone(), self.saves.clone());
        let save_path: SavePath = Arc::new(move |suggested| {
            assert!(suggested.ends_with(".notebook"), "{suggested}");
            saves.fetch_add(1, Ordering::SeqCst);
            answer.lock().unwrap().clone()
        });
        let answer = self.answer.clone();
        let pick_files: PickFiles = Arc::new(move |pick| {
            assert_eq!(pick, Pick::Project);
            answer.lock().unwrap().clone().into_iter().collect()
        });
        let app = NotebookApp::with_transcription(Some(Transcriber::new(fetch)), None)
            .with_save_path(save_path)
            .with_file_picker(pick_files);
        harness_for(app)
    }
}

/// A test directory, and dialogs that answer with `notes.notebook` in it.
fn setup(test: &str) -> (tempfile::TempDir, PathBuf, Dialogs) {
    let dir = test_dir(test);
    let path = dir.path().join("notes.notebook");
    let dialogs = Dialogs::answering(&path);
    (dir, path, dialogs)
}

fn saved(path: &Path) -> Value {
    serde_json::from_slice(&project::json(path).unwrap()).unwrap()
}

fn saved_names(path: &Path) -> Vec<String> {
    let saved = saved(path);
    let pages = saved["pages"].as_array().unwrap();
    pages
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_owned())
        .collect()
}

fn title(harness: &Harness<'_, NotebookApp>) -> String {
    harness.state().window_title()
}

fn asks_to_save(harness: &Harness<'_, NotebookApp>) -> bool {
    let name = harness.state().project_name();
    let question = t!("unsaved-title", name = name);
    harness.query_by_label(&question).is_some()
}

fn answer(harness: &mut Harness<'_, NotebookApp>, button: &str) {
    harness.get_by_label(button).click();
    harness.run();
}

fn cancels_close(commands: &[ViewportCommand]) -> bool {
    commands.contains(&ViewportCommand::CancelClose)
}

#[test]
fn corners_and_corrections_come_back_when_the_project_is_opened() {
    let (dir, path, dialogs) = setup("round_trip");
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(
        &mut harness,
        dir.path(),
        &[("a.png", page_photo()), ("b.png", png())],
    );
    show_transcript(&mut harness);
    correct_plumber(&mut harness);
    recrop(
        &mut harness,
        [(30.0, 15.0), (170.0, 10.0), (165.0, 135.0), (35.0, 140.0)],
    );
    click_command(&harness, Command::ViewCleaned);
    settle(&mut harness);
    let corners = handles(&harness);
    press_command(&mut harness, Key::S);
    assert_eq!(dialogs.saves(), 1, "asked where, being untitled");
    assert_eq!(title(&harness), "notes");
    assert_eq!(saved_names(&path), ["a.png", "b.png"]);

    let (fetch, fetches) = counting_fetch();
    let mut reopened = dialogs.app(fetch);
    press_command(&mut reopened, Key::O);
    settle(&mut reopened);
    assert_eq!(thumbnails(&reopened), ["a.png", "b.png"]);
    assert_eq!(title(&reopened), "notes", "nothing changed yet");
    click_command(&reopened, Command::ViewCleaned);
    settle(&mut reopened);
    assert_eq!(handles(&reopened), corners);
    show_transcript(&mut reopened);
    assert!(reopened.query_by_label("plumbers").is_some(), "corrected");
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        0,
        "not sent to Claude again"
    );
}

#[test]
fn the_title_says_whether_there_are_unsaved_changes() {
    let (dir, path, dialogs) = setup("title");
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    assert_eq!(title(&harness), t!("project-untitled"));

    drop_files_in(
        &mut harness,
        dir.path(),
        &[("a.png", png()), ("b.png", png())],
    );
    let untitled = t!("project-untitled");
    assert_eq!(title(&harness), t!("window-title-edited", name = untitled));

    press(&mut harness, COMMAND_SHIFT, Key::S);
    assert!(path.exists());
    assert_eq!(title(&harness), "notes");
    assert_eq!(harness.state().project_path(), Some(path.as_path()));

    press(&mut harness, COMMAND_SHIFT, Key::ArrowRight);
    assert_eq!(title(&harness), t!("window-title-edited", name = "notes"));
    // Back as it was saved.
    press(&mut harness, COMMAND_SHIFT, Key::ArrowLeft);
    assert_eq!(title(&harness), "notes");
}

#[test]
fn closing_with_unsaved_changes_asks_first() {
    let (dir, _path, dialogs) = setup("close");
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    assert!(
        !cancels_close(&close_window(&mut harness)),
        "nothing to lose"
    );

    drop_files_in(&mut harness, dir.path(), &[("a.png", png())]);
    let commands = close_window(&mut harness);
    assert!(cancels_close(&commands));
    harness.run();
    assert!(asks_to_save(&harness));

    answer(&mut harness, &t!("dialog-cancel"));
    assert!(!harness.state().quitting());
    assert!(!asks_to_save(&harness));
    assert_eq!(thumbnails(&harness), ["a.png"]);

    close_window(&mut harness);
    harness.run();
    answer(&mut harness, &t!("unsaved-discard"));
    assert!(harness.state().quitting());
    assert_eq!(dialogs.saves(), 0);
    // The window may close now.
    assert!(!cancels_close(&close_window(&mut harness)));
}

#[test]
fn saving_before_quitting_quits_once_saved() {
    let (dir, path, dialogs) = setup("quit_save");
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(&mut harness, dir.path(), &[("a.png", png())]);

    // Cancelling the save dialog cancels quitting.
    dialogs.cancel();
    press_command(&mut harness, Key::Q);
    assert!(asks_to_save(&harness));
    answer(&mut harness, &t!("unsaved-save"));
    assert_eq!(dialogs.saves(), 1);
    assert!(!harness.state().quitting());
    assert!(!asks_to_save(&harness));

    *dialogs.answer.lock().unwrap() = Some(path.clone());
    press_command(&mut harness, Key::Q);
    answer(&mut harness, &t!("unsaved-save"));
    assert!(harness.state().quitting());
    assert_eq!(saved_names(&path), ["a.png"]);
}

#[test]
fn quitting_without_changes_doesnt_ask() {
    let (_dir, _path, dialogs) = setup("quit_clean");
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    press_command(&mut harness, Key::Q);
    assert!(!asks_to_save(&harness));
    assert!(harness.state().quitting());
}

#[test]
fn a_new_project_starts_empty_once_changes_are_dealt_with() {
    let (dir, _path, dialogs) = setup("new");
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(&mut harness, dir.path(), &[("a.png", png())]);

    press_command(&mut harness, Key::N);
    assert!(asks_to_save(&harness));
    // Escape cancels.
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert!(!asks_to_save(&harness));
    assert_eq!(thumbnails(&harness), ["a.png"]);

    press_command(&mut harness, Key::N);
    // ⌘⌫ doesn't save.
    press_command(&mut harness, Key::Backspace);
    assert!(!asks_to_save(&harness));
    assert!(thumbnails(&harness).is_empty());
    assert!(harness.query_by_label(&t!("drop-zone-title")).is_some());
    assert_eq!(title(&harness), t!("project-untitled"));
    assert_eq!(dialogs.saves(), 0);

    // Nothing to lose now.
    press_command(&mut harness, Key::N);
    assert!(!asks_to_save(&harness));
}

#[test]
fn opening_asks_to_save_first_and_enter_saves() {
    let (dir, path, dialogs) = setup("open");
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(&mut harness, dir.path(), &[("a.png", png())]);
    press_command(&mut harness, Key::S);
    drop_files_in(&mut harness, dir.path(), &[("b.png", png())]);

    press_command(&mut harness, Key::O);
    assert!(asks_to_save(&harness));
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(!asks_to_save(&harness));
    assert_eq!(saved_names(&path), ["a.png", "b.png"], "saved first");
    assert_eq!(dialogs.saves(), 1, "saved where it was");
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"], "then opened");
    assert_eq!(title(&harness), "notes");
}

#[test]
fn a_damaged_project_leaves_the_open_one_alone() {
    let (dir, path, dialogs) = setup("damaged");
    std::fs::write(&path, "not a project").unwrap();
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(&mut harness, dir.path(), &[("a.png", png())]);
    press_command(&mut harness, Key::O);
    press_command(&mut harness, Key::Backspace);
    assert_eq!(thumbnails(&harness), ["a.png"]);
    let error = before_mark(t!(
        "project-not-a-project",
        path = path.display().to_string(),
        error = MARK
    ));
    assert!(harness.query_by_label_contains(&error).is_some());
}

#[test]
fn missing_photos_are_reported_and_kept() {
    let (dir, path, dialogs) = setup("missing");
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(
        &mut harness,
        dir.path(),
        &[("a.png", png()), ("b.png", png())],
    );
    press_command(&mut harness, Key::S);
    let before = saved(&path);
    std::fs::remove_file(dir.path().join("a.png")).unwrap();

    let mut reopened = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    press_command(&mut reopened, Key::O);
    settle(&mut reopened);
    assert_eq!(thumbnails(&reopened), ["b.png"]);
    let missing = dir.path().join("a.png").display().to_string();
    let error = t!("photo-not-found", name = "a.png", path = missing);
    assert!(reopened.query_by_label(&error).is_some());
    assert_eq!(title(&reopened), "notes", "nothing changed");

    press(&mut reopened, COMMAND_SHIFT, Key::S);
    assert_eq!(saved(&path), before, "a.png's page saved again as it was");
}

#[test]
fn transcripts_for_the_old_project_dont_reach_the_new_one() {
    let (dir, path, dialogs) = setup("old_jobs");
    let gate = Arc::new(RwLock::new(()));
    let fetch = gated_fetch(Arc::new(|_, _, _, _| Ok(recorded())), gate.clone());
    let mut harness = dialogs.app(fetch);
    drop_files_in(&mut harness, dir.path(), &[("a.png", page_photo())]);
    let held = gate.write().unwrap();
    // Not `press`, which would wait for the transcription.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::T);
    harness.step();
    assert!(harness.state().transcribing());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.step();
    assert!(asks_to_save(&harness));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Backspace);
    harness.step();
    assert!(!harness.state().transcribing(), "the new project's isn't");
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    drop_files_in(&mut harness, &other, &[("a.png", png())]);
    drop(held);
    // Long enough for the old transcription to come back, were anything collecting it.
    std::thread::sleep(std::time::Duration::from_millis(100));
    settle(&mut harness);

    press_command(&mut harness, Key::S);
    assert_eq!(saved(&path)["pages"][0]["transcript"], Value::Null);
}

#[test]
fn every_answer_to_save_is_reachable_by_keyboard() {
    let (dir, _path, dialogs) = setup("keyboard");
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(&mut harness, dir.path(), &[("a.png", png())]);
    press_command(&mut harness, Key::N);
    tab_to(&mut harness, &t!("unsaved-save"));
    tab_to(&mut harness, &t!("dialog-cancel"));
    tab_to(&mut harness, &t!("unsaved-discard"));
    // Enter is the focused button's.
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(!asks_to_save(&harness));
    assert!(thumbnails(&harness).is_empty());
    assert_eq!(dialogs.saves(), 0);
}

// Opening a project by dropping it or naming it on the command line. See
// `decisions/2026-10-31-open-by-drop-and-argument.md`.

/// Saves a project of `photos`, dropped from `dir`, at the path `dialogs` answers with.
fn save_project(dir: &Path, dialogs: &Dialogs, photos: &[&str]) {
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    let files: Vec<(&str, Vec<u8>)> = photos.iter().map(|name| (*name, png())).collect();
    drop_files_in(&mut harness, dir, &files);
    press_command(&mut harness, Key::S);
}

#[test]
fn dropping_a_project_asks_to_save_then_opens_it() {
    let (dir, path, dialogs) = setup("drop_project");
    save_project(dir.path(), &dialogs, &["a.png"]);
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(&mut harness, dir.path(), &[("b.png", png())]);

    drop_paths(&mut harness, &[&path]);
    assert!(asks_to_save(&harness));
    assert_eq!(thumbnails(&harness), ["b.png"], "not yet");
    harness.get_by_label(&t!("unsaved-discard")).click();
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["a.png"]);
    assert_eq!(title(&harness), "notes");
}

#[test]
fn files_dropped_with_a_project_are_left_out() {
    let (dir, path, dialogs) = setup("drop_project_and_photos");
    save_project(dir.path(), &dialogs, &["a.png"]);
    let photo = dir.path().join("c.png");
    std::fs::write(&photo, png()).unwrap();
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));

    drop_paths(&mut harness, &[&photo, &path]);
    assert!(!asks_to_save(&harness), "nothing to lose");
    assert_eq!(thumbnails(&harness), ["a.png"]);
    assert_eq!(title(&harness), "notes", "c.png wasn't added");
    let left_out = t!("drop-others-left-out", count = 1);
    assert!(harness.query_by_label(&left_out).is_some());
}

#[test]
fn dropping_two_projects_opens_neither() {
    let (dir, path, dialogs) = setup("drop_two_projects");
    save_project(dir.path(), &dialogs, &["a.png"]);
    let other = dir.path().join("other.notebook");
    std::fs::copy(&path, &other).unwrap();
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(&mut harness, dir.path(), &[("b.png", png())]);

    drop_paths(&mut harness, &[&path, &other]);
    assert!(!asks_to_save(&harness));
    assert_eq!(thumbnails(&harness), ["b.png"]);
    let error = t!("drop-several-projects");
    assert!(harness.query_by_label(&error).is_some());
}

#[test]
fn a_project_dropped_while_asking_to_save_is_ignored() {
    let (dir, path, dialogs) = setup("drop_project_while_asking");
    save_project(dir.path(), &dialogs, &["a.png"]);
    let mut harness = dialogs.app(Arc::new(|_, _, _, _| Ok(recorded())));
    drop_files_in(&mut harness, dir.path(), &[("b.png", png())]);
    press_command(&mut harness, Key::N);
    assert!(asks_to_save(&harness));

    drop_paths(&mut harness, &[&path]);
    answer(&mut harness, &t!("unsaved-discard"));
    assert!(!asks_to_save(&harness));
    assert!(
        harness.query_by_label(&t!("drop-zone-title")).is_some(),
        "the new project the user asked for, not the one dropped"
    );
}

#[test]
fn dragging_files_over_the_window_says_what_dropping_them_does() {
    let dir = test_dir("hover");
    let (photo, notes) = (dir.path().join("a.png"), dir.path().join("notes.notebook"));
    let other = dir.path().join("other.notebook");
    let says = |harness: &Harness<'_, NotebookApp>, text: String| {
        assert!(harness.query_by_label(&text).is_some(), "{text}");
    };
    let mut empty = harness();
    let mut with_pages = harness();
    drop_files(&mut with_pages, &[("b.png", png())]);
    for harness in [&mut empty, &mut with_pages] {
        hover_paths(harness, &[&photo]);
        says(harness, t!("drop-overlay"));
        hover_paths(harness, &[&photo, &notes]);
        says(harness, t!("drop-overlay-open", name = "notes"));
        hover_paths(harness, &[&notes, &other]);
        says(harness, t!("drop-overlay-one-project"));
    }

    // The empty window lists the photos it takes only while photos would be added.
    let lists_formats = |harness: &Harness<'_, NotebookApp>| {
        harness.query_by_label(&t!("drop-zone-formats")).is_some()
    };
    hover_paths(&mut empty, &[&photo]);
    assert!(lists_formats(&empty));
    hover_paths(&mut empty, &[&photo, &notes]);
    assert!(!lists_formats(&empty), "opening a project");
    hover_paths(&mut empty, &[&notes, &other]);
    assert!(!lists_formats(&empty), "several projects");
    hover_paths(&mut empty, &[]);
    assert!(lists_formats(&empty), "nothing dragged");
}

#[test]
fn a_project_named_on_the_command_line_starts_open() {
    let (dir, path, dialogs) = setup("command_line");
    save_project(dir.path(), &dialogs, &["a.png", "b.png"]);
    std::fs::remove_file(dir.path().join("a.png")).unwrap();

    let notebook_app::cli::Launch::Open {
        project: Some((opened_path, opened)),
        offline: false,
        ..
    } = notebook_app::cli::launch([path.clone().into()]).unwrap()
    else {
        panic!("didn't open {}", path.display());
    };
    assert_eq!(opened_path, path);
    let mut harness = harness_for(NotebookApp::default().with_project(&path, opened));
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["b.png"]);
    assert_eq!(title(&harness), "notes", "nothing changed");
    let missing = dir.path().join("a.png").display().to_string();
    let error = t!("photo-not-found", name = "a.png", path = missing);
    assert!(harness.query_by_label(&error).is_some());
}
