//! Pages are told apart by their IDs, not their names: two photos called `IMG_0001.png` from
//! different folders are two pages, and the same photo dropped twice is one. See
//! `decisions/2026-11-15-page-ids.md`.

use std::path::PathBuf;

use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::{Command, NotebookApp, t};
use notebook_core::project;
use tempfile::TempDir;

use crate::common::*;

const NAME: &str = "IMG_0001.png";

/// Two different photos called [`NAME`], from `hike/` and `trip/` in a test directory, dropped in
/// that order, and the project file they'd be saved to.
struct Twins {
    dir: TempDir,
    project: PathBuf,
    harness: Harness<'static, NotebookApp>,
}

impl Twins {
    fn photo(&self, folder: &str) -> PathBuf {
        self.dir.path().join(folder).join(NAME)
    }

    /// Saves the project, and the photos its pages name, in order.
    fn saved(&mut self) -> Vec<Option<PathBuf>> {
        press_command(&mut self.harness, Key::S);
        let pages = project::open(&self.project).unwrap();
        pages.iter().map(|p| p.path.clone()).collect()
    }
}

fn twins(test: &str, app: NotebookApp) -> Twins {
    let dir = test_dir(test);
    let project = dir.path().join("p.notebook");
    let mut harness = harness_for(app.with_save_path(save_to(&project)));
    for (folder, photo) in [("hike", page_photo()), ("trip", another_page_photo())] {
        let folder = dir.path().join(folder);
        std::fs::create_dir(&folder).unwrap();
        drop_files_in(&mut harness, &folder, &[(NAME, photo)]);
    }
    Twins {
        dir,
        project,
        harness,
    }
}

fn in_folder(twins: &Twins, folder: &str) -> Option<PathBuf> {
    Some(twins.photo(folder))
}

#[test]
fn photos_with_the_same_name_are_both_pages() {
    let mut twins = twins("both", NotebookApp::default());
    assert_eq!(thumbnails(&twins.harness), [NAME, NAME]);
    let both = vec![in_folder(&twins, "hike"), in_folder(&twins, "trip")];
    assert_eq!(twins.saved(), both);

    // The second one goes, and only it.
    press_command(&mut twins.harness, Key::ArrowRight);
    press_command(&mut twins.harness, Key::Backspace);
    settle(&mut twins.harness);
    assert_eq!(thumbnails(&twins.harness), [NAME]);
    assert_eq!(twins.saved(), vec![in_folder(&twins, "hike")]);
}

#[test]
fn transcribing_one_leaves_the_other() {
    let mut twins = twins(
        "transcribe",
        NotebookApp::with_transcription(Some(transcriber()), None),
    );
    let harness = &mut twins.harness;
    show_transcript(harness);
    assert_eq!(events(harness), ["transcribed from_cache=false"]);

    press_command(harness, Key::ArrowRight);
    settle(harness);
    assert!(
        harness.query_by_label("plumber").is_none(),
        "not transcribed"
    );
    press_command(harness, Key::ArrowLeft);
    settle(harness);
    assert!(harness.query_by_label("plumber").is_some());
}

#[test]
fn corners_stay_with_their_page() {
    let mut twins = twins("corners", NotebookApp::default());
    click_command(&twins.harness, Command::ViewCleaned);
    settle(&mut twins.harness);
    press_command(&mut twins.harness, Key::ArrowRight);
    settle(&mut twins.harness);
    tab_to(&mut twins.harness, &t!("corner-bottom-right"));
    press(&mut twins.harness, Modifiers::SHIFT, Key::ArrowLeft);
    settle(&mut twins.harness);

    press_command(&mut twins.harness, Key::S);
    let pages = project::open(&twins.project).unwrap();
    let corners: Vec<bool> = pages.iter().map(|p| p.corners.is_some()).collect();
    assert_eq!(corners, [false, true]);
}

#[test]
fn the_same_photo_dropped_again_is_left_out() {
    let mut harness = harness();
    let photo = png();
    drop_files(&mut harness, &[("a.png", photo.clone())]);
    // Under another name too, and silently.
    drop_files(
        &mut harness,
        &[("a.png", photo.clone()), ("copy.png", photo)],
    );
    assert_eq!(thumbnails(&harness), ["a.png"]);
    let error = before_mark(t!("open-failed", name = MARK, error = MARK));
    assert!(harness.query_by_label_contains(&error).is_none());
}
