use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::{NotebookApp, Transcriber};
use notebook_core::ocr::{Transcript, parse_response};

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

/// A cream page on a brown table, which cleanup finds an outline for.
fn page_photo() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(200, 150, |x, y| {
        if (40..160).contains(&x) && (20..130).contains(&y) {
            image::Rgba([235, 225, 200, 255])
        } else {
            image::Rgba([120, 80, 50, 255])
        }
    });
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn harness() -> Harness<'static, NotebookApp> {
    Harness::new_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::default(),
    )
}

fn harness_with(transcriber: Transcriber) -> Harness<'static, NotebookApp> {
    Harness::new_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::with_transcriber(Some(transcriber)),
    )
}

/// The recorded response from core's OCR tests.
fn recorded_transcript() -> Transcript {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/ocr/success.json");
    parse_response(200, &std::fs::read(path).unwrap(), (1200, 900)).unwrap()
}

/// Steps frames until a label containing `text` appears. Transcription finishes on another
/// thread, and the spinner keeps requesting repaints, so `run` won't do.
fn step_until(harness: &mut Harness<'_, NotebookApp>, text: &str) {
    for _ in 0..500 {
        harness.step();
        if harness.query_by_label_contains(text).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{text:?} never appeared");
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
fn cleaned_view_shows_the_cleaned_page() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", page_photo())]);

    harness.get_by_label("Cleaned").click();
    harness.run();
    harness.get_by_label("Current: a.png (cleaned)");

    harness.get_by_label("Original").click();
    harness.run();
    harness.get_by_label("Current: a.png");
}

#[test]
fn view_stays_when_selecting_another_page() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    harness.get_by_label("Cleaned").click();
    harness.run();

    harness.get_by_label("b.png").click();
    harness.run();

    harness.get_by_label("Current: b.png (cleaned)");
}

/// The debug outline is drawn relative to the image's rect, so that rect must be where the image
/// is drawn, not the whole panel it's centred in.
#[test]
fn image_rect_matches_the_drawn_image() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", page_photo())]);

    let node = harness.get_by_label("Current: a.png");
    let bounds = node.accesskit_node().bounding_box().unwrap();
    let aspect = bounds.width() / bounds.height();
    assert!((aspect - 200.0 / 150.0).abs() < 0.02, "{bounds:?}");
}

#[test]
fn debug_overlay_shows_what_cleanup_found() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    assert!(harness.query_by_label_contains("Page outline").is_none());

    harness.get_by_label("Debug").click();
    harness.run();
    harness.get_by_label_contains("Page outline: (");
    harness.get_by_label_contains("Black / white point");

    // A blank image has no page to find.
    harness.get_by_label("b.png").click();
    harness.run();
    harness.get_by_label_contains("No page outline");
}

/// Transcribed is disabled because the harness has no transcriber (no credentials).
#[test]
fn unimplemented_controls_are_disabled() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png())]);

    for label in ["Original", "Cleaned", "Debug"] {
        let node = harness.get_by_label(label);
        assert!(!node.accesskit_node().is_disabled(), "{label} is disabled");
    }
    for label in ["Transcribed", "Delete", "Re-export", "Export"] {
        let node = harness.get_by_label(label);
        assert!(node.accesskit_node().is_disabled(), "{label} is enabled");
    }
}

#[test]
fn transcribed_view_transcribes_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let mut harness = harness_with(Arc::new(move |_, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(recorded_transcript())
    }));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    harness.get_by_label("Transcribed").click();
    step_until(&mut harness, "# Thurs 23 Oct");
    harness.get_by_label_contains("~~tap~~ shower");

    harness.get_by_label("Original").click();
    harness.run();
    harness.get_by_label("Current: a.png");
    harness.get_by_label("Transcribed").click();
    harness.run();
    harness.get_by_label_contains("# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_transcription_can_be_retried() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let mut harness = harness_with(Arc::new(move |_, _| {
        match counter.fetch_add(1, Ordering::SeqCst) {
            0 => Err("Claude API error 529 (overloaded_error): Overloaded".into()),
            _ => Ok(recorded_transcript()),
        }
    }));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    harness.get_by_label("Transcribed").click();
    step_until(
        &mut harness,
        "Couldn't transcribe a.png: Claude API error 529",
    );
    // A failure isn't retried by itself.
    harness.run();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    harness.get_by_label("Retry").click();
    step_until(&mut harness, "# Thurs 23 Oct");
    assert!(
        harness
            .query_by_label_contains("Couldn't transcribe")
            .is_none()
    );
}
