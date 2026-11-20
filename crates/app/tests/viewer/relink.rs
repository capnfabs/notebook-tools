//! Relinking pages whose photos are missing: Relink…, Use Changed File, and dropping a photo that
//! looks like a missing page's by its EXIF. See `decisions/2026-11-17-relink-photos.md`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::accesskit::Role;
use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::{Command, NotebookApp, Pick, PickFiles, t};
use notebook_core::photo_meta::{PhotoIds, testing};
use notebook_core::project;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::*;

/// The size the photos are taken at: as small as can be added (`notebook_core::photo_meta::MIN_SIDE`),
/// to load quickly.
const SIZE: (u32, u32) = (268, 201);

/// When b.jpg was taken, to the second only: some cameras say no more.
fn b_taken() -> PhotoIds {
    PhotoIds {
        date_time_original: Some("2026:05:01 12:34:56".into()),
        ..PhotoIds::default()
    }
}

fn a_ids() -> PhotoIds {
    PhotoIds {
        photo_identifier: Some("6F1C2A8E-1B4D-4C1A-9E77-0D3B5A2F9C10".into()),
        date_time_original: Some("2026:05:01 09:00:00".into()),
        sub_sec_time_original: Some("250".into()),
        offset_time_original: Some("+10:00".into()),
        ..PhotoIds::default()
    }
}

/// A cream page on a `table`, `size` wide and high, as a JPEG whose EXIF says `ids`.
fn photo(table: u8, (w, h): (u32, u32), ids: &PhotoIds) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let inside = (w / 5..w * 4 / 5).contains(&x) && (h / 8..h * 7 / 8).contains(&y);
        if inside {
            image::Rgb([235, 225, 200])
        } else {
            image::Rgb([table, 80, 50])
        }
    });
    testing::jpeg(&img, ids)
}

/// b.jpg as first taken.
fn b_photo() -> Vec<u8> {
    photo(120, SIZE, &b_taken())
}

/// What the stand-in file chooser was asked, and the file it picks.
#[derive(Clone, Default)]
struct Chooser {
    asked: Arc<Mutex<Vec<Pick>>>,
    picks: Arc<Mutex<Option<PathBuf>>>,
}

impl Chooser {
    fn picks(&self, path: &Path) {
        *self.picks.lock().unwrap() = Some(path.to_path_buf());
    }

    fn asked(&self) -> Vec<Pick> {
        self.asked.lock().unwrap().clone()
    }

    fn pick_files(&self) -> PickFiles {
        let chooser = self.clone();
        Arc::new(move |pick| {
            chooser.asked.lock().unwrap().push(pick);
            chooser.picks.lock().unwrap().clone().into_iter().collect()
        })
    }
}

/// A project, `photos/notes.notebook`, of a.jpg and b.jpg (transcribed), photos with EXIF in
/// `photos/`, opened again once b.jpg has gone, with b.jpg shown.
struct Opened {
    dir: TempDir,
    path: PathBuf,
    harness: Harness<'static, NotebookApp>,
    chooser: Chooser,
}

impl Opened {
    fn photos(&self) -> PathBuf {
        self.dir.path().join("photos")
    }

    /// Writes `bytes` to `name` in the test's directory, and returns where.
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Clicks Relink… with the chooser picking `path`.
    fn relink(&mut self, path: &Path) {
        self.chooser.picks(path);
        click_command(&self.harness, Command::RelinkPhoto);
        settle(&mut self.harness);
    }

    fn saved_b(&mut self) -> Value {
        press_command(&mut self.harness, Key::S);
        let json = project::json(&self.path).unwrap();
        let saved: Value = serde_json::from_slice(&json).unwrap();
        saved["pages"][1].clone()
    }
}

fn opened_without_b(test: &str) -> Opened {
    opened_with_b(test, |b| std::fs::remove_file(b).unwrap())
}

/// [`opened_without_b`], but doing `change` to b.jpg rather than removing it.
fn opened_with_b(test: &str, change: impl FnOnce(&Path)) -> Opened {
    let dir = test_dir(test);
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let path = photos.join("notes.notebook");
    let app =
        NotebookApp::with_transcription(Some(transcriber()), None).with_save_path(save_to(&path));
    let mut first = harness_for(app);
    let files = [("a.jpg", photo(100, SIZE, &a_ids())), ("b.jpg", b_photo())];
    drop_files_in(&mut first, &photos, &files);
    press_command(&mut first, Key::ArrowRight);
    show_transcript(&mut first);
    press_command(&mut first, Key::S);
    change(&photos.join("b.jpg"));

    let chooser = Chooser::default();
    let app = NotebookApp::with_transcription(Some(transcriber()), None)
        .with_save_path(save_to(&path))
        .with_file_picker(chooser.pick_files())
        .with_project(&path, project::open(&path).unwrap());
    let mut harness = harness_for(app);
    settle(&mut harness);
    press_command(&mut harness, Key::ArrowRight);
    assert!(harness.query_by_label(&t!("missing-photo-title")).is_some());
    Opened {
        dir,
        path,
        harness,
        chooser,
    }
}

fn found(harness: &Harness<'_, NotebookApp>) -> bool {
    harness.query_by_label(&t!("missing-photo-title")).is_none()
}

fn edited(harness: &Harness<'_, NotebookApp>) -> bool {
    harness.state().window_title() == t!("window-title-edited", name = "notes")
}

fn click(harness: &mut Harness<'_, NotebookApp>, label: &str) {
    harness.get_by_role_and_label(Role::Button, label).click();
    settle(harness);
}

fn shown(harness: &Harness<'_, NotebookApp>, text: &str) -> bool {
    harness.query_by_label(text).is_some()
}

fn count(harness: &Harness<'_, NotebookApp>, text: &str) -> usize {
    harness.query_all_by_label(text).count()
}

#[test]
fn the_placeholder_offers_relink_and_says_drops_work() {
    let Opened {
        mut harness,
        chooser,
        ..
    } = opened_without_b("offers");
    assert!(shown(&harness, &t!("missing-photo-drop-note")));
    assert!(!disabled(&harness, &Command::RelinkPhoto.label()));
    assert!(
        !shown(&harness, &Command::UseChangedFile.label()),
        "only for a changed photo"
    );
    // Nothing picked: nothing changes.
    click_command(&harness, Command::RelinkPhoto);
    settle(&mut harness);
    assert!(!found(&harness) && !edited(&harness));
    assert_eq!(chooser.asked().len(), 1);

    press_command(&mut harness, Key::ArrowLeft);
    assert!(
        !shown(&harness, &Command::RelinkPhoto.label()),
        "a.jpg isn't missing"
    );
}

#[test]
fn the_same_file_is_relinked_without_asking() {
    let mut opened = opened_without_b("same_file");
    let moved = opened.write("moved/IMG_0002.jpg", &b_photo());
    opened.relink(&moved);
    // The chooser opened where b.jpg was.
    assert_eq!(
        opened.chooser.asked(),
        [Pick::Relink {
            near: Some(opened.photos())
        }]
    );
    let harness = &opened.harness;
    assert!(found(harness));
    let summary = t!("photos-missing", count = 1, names = "b.jpg");
    assert!(!shown(harness, &summary), "nothing's missing now");
    assert_eq!(thumbnails(harness), ["a.jpg", "IMG_0002.jpg"]);
    assert!(shown(harness, &t!("photo-found", name = "b.jpg")));
    assert!(edited(harness));

    let b = opened.saved_b();
    assert_eq!(b["photo"]["path"], "../moved/IMG_0002.jpg");
    assert_eq!(b["name"], "IMG_0002.jpg");
    assert!(b["transcript"].is_object(), "its transcript stays");
}

#[test]
fn the_chooser_opens_in_the_nearest_folder_left() {
    let mut opened = opened_without_b("nearest");
    let photos = opened.photos();
    // Its folder's gone too, so the project's elsewhere: the folder above.
    let project = opened.write("notes.notebook", &std::fs::read(&opened.path).unwrap());
    std::fs::remove_dir_all(&photos).unwrap();
    let app = NotebookApp::default()
        .with_file_picker(opened.chooser.pick_files())
        .with_project(&project, project::open(&project).unwrap());
    opened.harness = harness_for(app);
    settle(&mut opened.harness);
    click_command(&opened.harness, Command::RelinkPhoto);
    settle(&mut opened.harness);
    assert_eq!(
        opened.chooser.asked(),
        [Pick::Relink {
            near: Some(opened.dir.path().to_path_buf())
        }]
    );
}

#[test]
fn a_reencoded_photo_asks_first() {
    let mut opened = opened_without_b("reencoded");
    // Another file: other bytes, the same EXIF and size.
    let other = opened.write("IMG_0002.jpg", &photo(121, SIZE, &b_taken()));
    opened.relink(&other);
    let asks = t!("relink-looks-alike", file = "IMG_0002.jpg", page = "b.jpg");
    assert!(shown(&opened.harness, &asks));
    // The table: both photos' fields, side by side.
    for cell in [
        t!("relink-column-old", name = "b.jpg"),
        t!("relink-column-new", name = "IMG_0002.jpg"),
        t!("relink-field-taken"),
    ] {
        assert!(shown(&opened.harness, &cell), "{cell}");
    }
    assert_eq!(count(&opened.harness, "2026-05-01 12:34:56"), 2);
    assert_eq!(count(&opened.harness, "268×201"), 2);

    click(&mut opened.harness, &t!("dialog-cancel"));
    assert!(!found(&opened.harness) && !edited(&opened.harness));

    opened.relink(&other);
    click(&mut opened.harness, &t!("relink-button"));
    assert!(found(&opened.harness));
    assert!(edited(&opened.harness));
    assert_eq!(opened.saved_b()["photo"]["path"], "../IMG_0002.jpg");
}

#[test]
fn a_resized_photo_scales_corners_and_transcript() {
    let mut opened = opened_without_b("resized");
    let before = opened.saved_b();
    let double = opened.write("big.jpg", &photo(120, (536, 402), &b_taken()));
    opened.relink(&double);
    assert!(shown(&opened.harness, "268×201") && shown(&opened.harness, "536×402"));
    let aspect = t!(
        "relink-aspect",
        ratio = "4:3",
        turned = t!("relink-landscape")
    );
    assert_eq!(count(&opened.harness, &aspect), 2);
    click(&mut opened.harness, &t!("relink-button"));
    assert!(found(&opened.harness));

    let after = opened.saved_b();
    assert_eq!(after["photo"]["size"], serde_json::json!([536, 402]));
    let corners = |page: &Value| -> Vec<f64> {
        let corners = page["transcript"]["crop"]["corners"].as_array().unwrap();
        corners
            .iter()
            .flat_map(|p| p.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()))
            .collect()
    };
    let doubled: Vec<f64> = corners(&before).iter().map(|c| c * 2.0).collect();
    for (after, doubled) in corners(&after).iter().zip(&doubled) {
        assert_near(*after, *doubled, 0.01);
    }
}

#[test]
fn another_photo_warns_but_can_be_relinked() {
    let mut opened = opened_without_b("unmatched");
    let other = opened.write("other.jpg", &photo(140, SIZE, &PhotoIds::default()));
    opened.relink(&other);
    let warns = t!("relink-unmatched", file = "other.jpg", page = "b.jpg");
    assert!(shown(&opened.harness, &warns));
    // b.jpg has a date; other.jpg has none.
    assert!(shown(&opened.harness, &t!("relink-no-value")));
    click(&mut opened.harness, &t!("relink-anyway-button"));
    assert!(found(&opened.harness));
}

#[test]
fn another_shape_is_refused() {
    let mut opened = opened_without_b("other_shape");
    // The same photo, cropped.
    let cropped = opened.write("cropped.jpg", &photo(120, (201, 201), &b_taken()));
    opened.relink(&cropped);
    let refused = t!("relink-different-shape");
    assert!(shown(&opened.harness, &refused));
    assert!(shown(&opened.harness, &t!("relink-field-aspect")));
    assert!(
        opened
            .harness
            .query_by_role_and_label(Role::Button, &t!("relink-button"))
            .is_none(),
        "no way round it"
    );
    click(&mut opened.harness, &t!("dialog-ok"));
    assert!(!shown(&opened.harness, &refused));
    assert!(!found(&opened.harness) && !edited(&opened.harness));
}

#[test]
fn a_photo_too_small_to_read_is_refused() {
    let mut opened = opened_without_b("too_small");
    // The same photo, shrunk past what can be read.
    let tiny = opened.write("tiny.jpg", &photo(120, (4, 3), &b_taken()));
    opened.relink(&tiny);
    let error = t!(
        "open-failed",
        name = "tiny.jpg",
        error = t!("decode-too-small", size = "4×3", min = 200)
    );
    assert!(shown(&opened.harness, &error));
    assert!(!found(&opened.harness) && !edited(&opened.harness));
}

#[test]
fn another_pages_photo_is_refused() {
    let mut opened = opened_without_b("another_page");
    let a = opened.photos().join("a.jpg");
    opened.relink(&a);
    let refused = t!("relink-already-a-page", file = "a.jpg", page = "a.jpg");
    assert!(shown(&opened.harness, &refused));
    click(&mut opened.harness, &t!("dialog-ok"));
    assert!(!found(&opened.harness));
}

#[test]
fn a_changed_photo_can_be_used() {
    let mut opened = opened_with_b("changed", |b| {
        // Edited in place: other bytes, the same photo.
        std::fs::write(b, photo(121, SIZE, &b_taken())).unwrap();
    });
    assert!(!disabled(&opened.harness, &Command::RelinkPhoto.label()));
    click_command(&opened.harness, Command::UseChangedFile);
    settle(&mut opened.harness);
    assert!(opened.chooser.asked().is_empty(), "no chooser");
    let asks = t!("relink-looks-alike", file = "b.jpg", page = "b.jpg");
    assert!(shown(&opened.harness, &asks));
    click(&mut opened.harness, &t!("relink-button"));
    assert!(found(&opened.harness));
    assert!(edited(&opened.harness), "a new hash to save");
}

#[test]
fn the_buttons_take_the_keyboard_focus() {
    let mut opened = opened_without_b("focus");
    let other = opened.write("IMG_0002.jpg", &photo(121, SIZE, &b_taken()));
    opened.chooser.picks(&other);
    tab_to(&mut opened.harness, &Command::RelinkPhoto.label());
    press(&mut opened.harness, Modifiers::NONE, Key::Enter);
    settle(&mut opened.harness);
    let asks = t!("relink-looks-alike", file = "IMG_0002.jpg", page = "b.jpg");
    assert!(shown(&opened.harness, &asks));
    tab_to(&mut opened.harness, &t!("dialog-cancel"));
    tab_to(&mut opened.harness, &t!("relink-button"));
    press(&mut opened.harness, Modifiers::NONE, Key::Enter);
    settle(&mut opened.harness);
    assert!(found(&opened.harness));
}

// Drops.

#[test]
fn a_dropped_photo_that_looks_alike_asks() {
    let mut opened = opened_without_b("drop_asks");
    let dir = opened.dir.path().to_path_buf();
    drop_files_in(
        &mut opened.harness,
        &dir,
        &[("IMG_0002.jpg", photo(121, SIZE, &b_taken()))],
    );
    let asks = t!("relink-dropped-one", file = "IMG_0002.jpg", page = "b.jpg");
    assert!(shown(&opened.harness, &asks));
    click(&mut opened.harness, &t!("relink-button"));
    assert!(found(&opened.harness));
    assert_eq!(thumbnails(&opened.harness), ["a.jpg", "IMG_0002.jpg"]);
}

#[test]
fn a_dropped_photo_that_looks_alike_can_be_another_page() {
    let mut opened = opened_without_b("drop_new_page");
    let dir = opened.dir.path().to_path_buf();
    drop_files_in(
        &mut opened.harness,
        &dir,
        &[("IMG_0002.jpg", photo(121, SIZE, &b_taken()))],
    );
    click(&mut opened.harness, &t!("relink-add-as-new-page"));
    assert_eq!(
        thumbnails(&opened.harness),
        ["IMG_0002.jpg", "a.jpg", "b.jpg"]
    );
    assert_eq!(selected_thumbnails(&opened.harness), ["b.jpg"]);
    assert!(!found(&opened.harness), "b.jpg is still missing");
}

#[test]
fn the_same_file_dropped_never_asks() {
    let mut opened = opened_without_b("drop_same");
    let dir = opened.dir.path().to_path_buf();
    drop_files_in(&mut opened.harness, &dir, &[("IMG_0002.jpg", b_photo())]);
    assert!(found(&opened.harness));
    assert_eq!(thumbnails(&opened.harness), ["a.jpg", "IMG_0002.jpg"]);
}

#[test]
fn another_shape_dropped_is_another_page() {
    let mut opened = opened_without_b("drop_shape");
    let dir = opened.dir.path().to_path_buf();
    drop_files_in(
        &mut opened.harness,
        &dir,
        &[("sq.jpg", photo(120, (201, 201), &b_taken()))],
    );
    let hint = t!(
        "relink-dropped-other-shape",
        file = "sq.jpg",
        pages = "b.jpg"
    );
    assert!(shown(&opened.harness, &hint));
    assert_eq!(thumbnails(&opened.harness), ["a.jpg", "b.jpg", "sq.jpg"]);
}

/// b.jpg and c.jpg, taken in the same second, both missing.
fn opened_without_b_and_c(test: &str) -> Opened {
    let dir = test_dir(test);
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let path = photos.join("notes.notebook");
    let mut first = harness_for(NotebookApp::default().with_save_path(save_to(&path)));
    let files = [
        ("a.jpg", photo(100, SIZE, &a_ids())),
        ("b.jpg", b_photo()),
        ("c.jpg", photo(130, SIZE, &b_taken())),
    ];
    drop_files_in(&mut first, &photos, &files);
    press_command(&mut first, Key::S);
    for gone in ["b.jpg", "c.jpg"] {
        std::fs::remove_file(photos.join(gone)).unwrap();
    }
    let app = NotebookApp::default()
        .with_save_path(save_to(&path))
        .with_project(&path, project::open(&path).unwrap());
    let mut harness = harness_for(app);
    settle(&mut harness);
    Opened {
        dir,
        path,
        harness,
        chooser: Chooser::default(),
    }
}

#[test]
fn a_photo_like_several_pages_asks_which() {
    let mut opened = opened_without_b_and_c("drop_which");
    let dir = opened.dir.path().to_path_buf();
    drop_files_in(
        &mut opened.harness,
        &dir,
        &[("IMG_0003.jpg", photo(131, SIZE, &b_taken()))],
    );
    assert!(shown(
        &opened.harness,
        &t!("relink-dropped-several", file = "IMG_0003.jpg")
    ));
    let taken = t!("relink-taken", when = "2026-05-01 12:34:56");
    let c = t!(
        "relink-photo-line",
        name = "c.jpg",
        size = "268×201",
        taken = taken
    );
    opened
        .harness
        .get_by_role_and_label(Role::RadioButton, &c)
        .click();
    settle(&mut opened.harness);
    click(&mut opened.harness, &t!("relink-button"));
    assert_eq!(
        thumbnails(&opened.harness),
        ["a.jpg", "b.jpg", "IMG_0003.jpg"]
    );
    assert_eq!(selected_thumbnails(&opened.harness), ["IMG_0003.jpg"]);
}

#[test]
fn the_status_line_says_whats_still_missing() {
    let mut opened = opened_without_b_and_c("still_missing");
    let both = t!("photos-missing", count = 2, names = "b.jpg, c.jpg");
    assert!(shown(&opened.harness, &both));
    let dir = opened.dir.path().to_path_buf();

    drop_files_in(&mut opened.harness, &dir, &[("IMG_0002.jpg", b_photo())]);
    assert!(!shown(&opened.harness, &both));
    let c = t!("photos-missing", count = 1, names = "c.jpg");
    assert!(shown(&opened.harness, &c));

    // Relinked after asking.
    let c_resized = photo(130, (536, 402), &b_taken());
    drop_files_in(&mut opened.harness, &dir, &[("IMG_0003.jpg", c_resized)]);
    assert!(shown(&opened.harness, &c), "until the user answers");
    click(&mut opened.harness, &t!("relink-button"));
    assert!(!shown(&opened.harness, &c));
}

#[test]
fn cancel_all_stops_asking_but_keeps_the_rest_of_the_drop() {
    let mut opened = opened_without_b_and_c("drop_cancel");
    let dir = opened.dir.path().to_path_buf();
    let files = [
        ("IMG_0003.jpg", photo(131, SIZE, &b_taken())),
        ("IMG_0004.jpg", photo(132, SIZE, &b_taken())),
        ("new.jpg", photo(140, SIZE, &PhotoIds::default())),
    ];
    drop_files_in(&mut opened.harness, &dir, &files);
    assert!(shown(
        &opened.harness,
        &t!("relink-dropped-several", file = "IMG_0003.jpg")
    ));
    assert_eq!(
        thumbnails(&opened.harness),
        ["a.jpg", "b.jpg", "c.jpg", "new.jpg"],
        "the unambiguous file is in already"
    );
    click(&mut opened.harness, &t!("dialog-cancel-all"));
    assert!(!shown(
        &opened.harness,
        &t!("relink-dropped-several", file = "IMG_0004.jpg")
    ));
    assert_eq!(
        thumbnails(&opened.harness),
        ["a.jpg", "b.jpg", "c.jpg", "new.jpg"]
    );
}

#[test]
fn the_last_question_says_cancel() {
    let mut opened = opened_without_b("drop_cancel_one");
    let dir = opened.dir.path().to_path_buf();
    drop_files_in(
        &mut opened.harness,
        &dir,
        &[("IMG_0002.jpg", photo(121, SIZE, &b_taken()))],
    );
    assert!(
        opened
            .harness
            .query_by_role_and_label(Role::Button, &t!("dialog-cancel-all"))
            .is_none()
    );
    click(&mut opened.harness, &t!("dialog-cancel"));
    assert_eq!(thumbnails(&opened.harness), ["a.jpg", "b.jpg"]);
}
