use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::NotebookApp;

/// Stands in for a file dragged from the desktop.
#[derive(Debug)]
struct TestFile {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl egui::DroppedFile for TestFile {
    fn path(&self) -> &Path {
        &self.path
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.bytes.clone())
    }
}

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::new(40, 30)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn harness() -> Harness<'static, NotebookApp> {
    Harness::new_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::default(),
    )
}

fn drop_files(harness: &mut Harness<'_, NotebookApp>, files: &[(&str, Vec<u8>)]) {
    for (name, bytes) in files {
        harness.input_mut().dropped_files.push(Arc::new(TestFile {
            path: PathBuf::from("/Users/someone/Desktop").join(name),
            bytes: bytes.clone(),
        }));
    }
    harness.run();
}

/// Thumbnail labels, left to right.
fn thumbnails(harness: &Harness<'_, NotebookApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Image)
        .filter_map(|node| node.accesskit_node().label())
        .filter(|label| !label.starts_with("Current: "))
        .collect()
}

#[test]
fn starts_with_drop_zone() {
    let harness = harness();
    harness.get_by_label("Drop image files here");
}

#[test]
fn shows_dropped_images_in_filename_order() {
    let mut harness = harness();
    drop_files(&mut harness, &[("b.png", png()), ("a.png", png())]);

    assert!(harness.query_by_label("Drop image files here").is_none());
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);
    harness.get_by_label("Current: a.png");
}

#[test]
fn clicking_a_thumbnail_selects_it() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png()), ("b.png", png())]);

    harness.get_by_label("b.png").click();
    harness.run();

    harness.get_by_label("Current: b.png");
}

#[test]
fn later_drops_merge_in_order_and_keep_selection() {
    let mut harness = harness();
    drop_files(&mut harness, &[("b.png", png()), ("d.png", png())]);
    harness.get_by_label("d.png").click();
    harness.run();

    drop_files(
        &mut harness,
        &[("c.png", png()), ("b.png", png()), ("a.png", png())],
    );

    assert_eq!(thumbnails(&harness), ["a.png", "b.png", "c.png", "d.png"]);
    harness.get_by_label("Current: d.png");
}

#[test]
fn reports_files_that_are_not_images() {
    let mut harness = harness();
    drop_files(&mut harness, &[("notes.txt", b"hello".to_vec())]);

    harness.get_by_label("Drop image files here");
    harness.get_by_label_contains("Couldn't open notes.txt");
}

#[test]
fn unimplemented_controls_are_disabled() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png())]);

    assert!(
        !harness
            .get_by_label("Original")
            .accesskit_node()
            .is_disabled()
    );
    for label in ["Cleaned", "Transcribed", "Delete", "Re-export", "Export"] {
        let node = harness.get_by_label(label);
        assert!(node.accesskit_node().is_disabled(), "{label} is enabled");
    }
}
