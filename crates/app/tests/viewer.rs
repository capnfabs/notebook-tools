use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use eframe::egui::{
    self,
    accesskit::{Role, Toggled},
};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::{
    Event, Fetch, Load, NotebookApp, OnTranscribeCompleted, PickFiles, SavePath, Transcriber,
};
use notebook_core::cache::Cache;
use notebook_core::compare;
use notebook_core::llm_model::Model;
use notebook_core::ocr::{Fetched, OcrError};
use notebook_core::{Loaded, Quad, is_page_shaped};

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
    harness_for(NotebookApp::with_transcription(Some(transcriber), None))
}

/// Transcribes with `transcriber`, caching in `cache_dir`.
fn harness_with_cache(transcriber: Transcriber, cache_dir: &Path) -> Harness<'static, NotebookApp> {
    harness_for(NotebookApp::with_transcription(
        Some(transcriber),
        Some(Cache::new(cache_dir)),
    ))
}

fn harness_for(app: NotebookApp) -> Harness<'static, NotebookApp> {
    Harness::new_ui_state(|ui, app: &mut NotebookApp| app.ui(ui), app)
}

/// Every page transcribes to the recorded response, without a cache.
fn transcriber() -> Transcriber {
    Transcriber::new(Arc::new(|_, _, _, _| Ok(recorded())))
}

/// Answers every request with the recorded response, counting them.
fn counting_fetch() -> (Fetch, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let fetch: Fetch = Arc::new(move |_, _, _, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(recorded())
    });
    (fetch, calls)
}

/// An empty cache directory of the test's own. Harnesses given the same one share a cache.
fn cache_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("viewer-cache")
        .join(test);
    std::fs::remove_dir_all(&dir).ok();
    dir
}

fn cached_harness(fetch: Fetch, cache_dir: &Path) -> Harness<'static, NotebookApp> {
    harness_with_cache(Transcriber::new(fetch), cache_dir)
}

/// Opens the Transcribed view of the selected page and waits for its transcript.
fn show_transcript(harness: &mut Harness<'_, NotebookApp>) {
    harness.get_by_label("Transcribed").click();
    step_until(harness, "# Thurs 23 Oct");
}

/// Opens the export dialog, picks `options` in order, and clicks Go.
fn export(harness: &mut Harness<'_, NotebookApp>, options: &[&str]) {
    harness.get_by_label("Export").click();
    settle(harness);
    for option in options {
        harness.get_by_label(option).click();
        settle(harness);
    }
    harness.get_by_label("Go").click();
}

/// Exports everything to the clipboard as markdown, the dialog's defaults, returning what was
/// copied.
fn export_to_clipboard(harness: &mut Harness<'_, NotebookApp>) -> String {
    export(harness, &[]);
    // Copied in the frame that says it was.
    step_until(harness, "to the clipboard");
    let copied: Vec<String> = harness
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(copied.len(), 1, "{copied:?}");
    copied.into_iter().next().unwrap()
}

/// Corrects the transcript's uncertain "plumber" to "plumbers", from the text.
fn correct_plumber(harness: &mut Harness<'_, NotebookApp>) {
    harness.get_by_label("plumber").click();
    settle(harness);
    harness.get_by_label("plumbers").click();
    settle(harness);
}

/// The recorded response from core's OCR tests.
fn recorded() -> Fetched {
    recorded_fixture("success")
}

fn recorded_fixture(name: &str) -> Fetched {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../core/tests/fixtures/ocr/{name}.json"));
    Fetched {
        status: 200,
        body: std::fs::read(path).unwrap(),
        image_size: (1200, 900),
    }
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

/// Steps frames until nothing is being transcribed. The first one acts on any click.
fn step_until_transcribed(harness: &mut Harness<'_, NotebookApp>) {
    harness.step();
    for _ in 0..500 {
        harness.step();
        // The status line and, in the Transcribed view, the page's progress.
        if harness
            .query_all_by_label_contains("Transcribing")
            .next()
            .is_none()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("still transcribing");
}

fn drop_files(harness: &mut Harness<'_, NotebookApp>, files: &[(&str, Vec<u8>)]) {
    start_dropping(harness, files);
    settle(harness);
}

/// Drops `files` without waiting for them to load.
fn start_dropping(harness: &mut Harness<'_, NotebookApp>, files: &[(&str, Vec<u8>)]) {
    for (name, bytes) in files {
        harness.input_mut().dropped_files.push(Arc::new(TestFile {
            path: PathBuf::from("/Users/someone/Desktop").join(name),
            bytes: bytes.clone(),
        }));
    }
    harness.step();
}

/// Loads pages as the app does, but only while `gate` isn't held for writing.
fn gated_loader(gate: Arc<RwLock<()>>) -> Load {
    Arc::new(move |bytes, params, corners| {
        let _open = gate.read().unwrap();
        Loaded::new(bytes, params, corners)
    })
}

/// Steps frames until no page is loading and the viewer has its image, which both happen off the
/// UI thread, then runs until the UI is idle. The first frame acts on any input.
fn settle(harness: &mut Harness<'_, NotebookApp>) {
    harness.step();
    for _ in 0..500 {
        if !harness.state().loading() {
            harness.run();
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
        harness.step();
    }
    panic!("still loading");
}

/// The eval log's events, less each one's page and model, as `(kind, detail)`.
fn events(harness: &Harness<'_, NotebookApp>) -> Vec<String> {
    let events = harness.state().eval_log().events();
    events
        .iter()
        .map(|event| match event {
            Event::Transcribed { from_cache, .. } => format!("transcribed from_cache={from_cache}"),
            Event::Corrected {
                word,
                from,
                to,
                via,
                ..
            } => {
                format!("corrected {word} {from}->{to} {via:?}")
            }
            Event::AllResolved {
                uncertain, unfound, ..
            } => format!("all resolved {uncertain} unfound={unfound}"),
            Event::LineEdited { line, from, to, .. } => format!("line {line} {from}->{to}"),
        })
        .collect()
}

/// Thumbnail labels, left to right.
fn thumbnails(harness: &Harness<'_, NotebookApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Image)
        .filter_map(|node| node.accesskit_node().label())
        .filter(|label| !label.starts_with("Current: "))
        .collect()
}

/// The centre of the node labelled `label`.
fn centre(harness: &Harness<'_, NotebookApp>, label: &str) -> egui::Pos2 {
    let bounds = harness
        .get_by_label(label)
        .accesskit_node()
        .bounding_box()
        .unwrap();
    egui::pos2(
        ((bounds.x0 + bounds.x1) / 2.0) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    )
}

/// Drags the thumbnail labelled `label` to `x`, at the same height, and drops it there.
fn drag_thumbnail(harness: &mut Harness<'_, NotebookApp>, label: &str, x: f32) {
    let end = hold_thumbnail(harness, label, x);
    harness.drop_at(end);
    settle(harness);
}

/// Drags the thumbnail labelled `label` to `x`, at the same height, without dropping it, and
/// returns where the pointer is.
fn hold_thumbnail(harness: &mut Harness<'_, NotebookApp>, label: &str, x: f32) -> egui::Pos2 {
    let start = centre(harness, label);
    let end = egui::pos2(x, start.y);
    hold(harness, start, end);
    end
}

/// Presses at `from` and moves to `to` in a few steps, without letting go. Doesn't wait for
/// loading, so works while loads are held up.
fn hold(harness: &mut Harness<'_, NotebookApp>, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for step in 1..=5 {
        let t = step as f32 / 5.0;
        harness.hover_at(from + (to - from) * t);
        harness.step();
    }
}

/// Drags from `from` to `to` and lets go there.
fn drag(harness: &mut Harness<'_, NotebookApp>, from: egui::Pos2, to: egui::Pos2) {
    hold(harness, from, to);
    harness.drop_at(to);
    harness.step();
}

/// Presses and lets go at `at`.
fn click_at(harness: &mut Harness<'_, NotebookApp>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    harness.drop_at(at);
    harness.step();
}

/// The left and right edges of what's painted as 3pt lines: the selected page's frame (a rect)
/// and the drop marker (a line segment), each cut to its clip rect.
fn painted_3pt(harness: &Harness<'_, NotebookApp>) -> (Vec<egui::Rangef>, Vec<egui::Rangef>) {
    let (mut frames, mut markers) = (Vec::new(), Vec::new());
    for clipped in &harness.output().shapes {
        let clip = clipped.clip_rect.x_range();
        let visible = |xs: egui::Rangef| xs.intersection(clip);
        match &clipped.shape {
            egui::Shape::Rect(rect) if rect.stroke.width == 3.0 => {
                // Drawn outside the thumbnail.
                frames.push(visible(rect.rect.expand(3.0).x_range()));
            }
            egui::Shape::LineSegment { points, stroke } if stroke.width == 3.0 => {
                let x = points[0].x;
                markers.push(visible(egui::Rangef::new(x - 1.5, x + 1.5)));
            }
            _ => {}
        }
    }
    (frames, markers)
}

fn press_command(harness: &mut Harness<'_, NotebookApp>, key: egui::Key) {
    press(harness, egui::Modifiers::COMMAND, key);
}

fn press(harness: &mut Harness<'_, NotebookApp>, modifiers: egui::Modifiers, key: egui::Key) {
    harness.key_press_modifiers(modifiers, key);
    settle(harness);
}

const COMMAND_SHIFT: egui::Modifiers = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);

/// The labels of the thumbnails marked selected.
fn selected_thumbnails(harness: &Harness<'_, NotebookApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Image)
        .filter(|node| node.accesskit_node().toggled() == Some(Toggled::True))
        .filter_map(|node| node.accesskit_node().label())
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
    settle(&mut harness);

    harness.get_by_label("Current: b.png");
}

#[test]
fn later_drops_merge_in_order_and_keep_selection() {
    let mut harness = harness();
    drop_files(&mut harness, &[("b.png", png()), ("d.png", png())]);
    harness.get_by_label("d.png").click();
    settle(&mut harness);

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
    harness.get_by_label("Couldn't open notes.txt: not a JPEG, PNG or HEIC image");
}

#[test]
fn files_that_are_not_images_drop_out_of_the_strip() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png()), ("c.png", png())]);
    harness.get_by_label("c.png").click();
    settle(&mut harness);

    drop_files(
        &mut harness,
        &[("b.txt", b"hello".to_vec()), ("d.txt", b"hi".to_vec())],
    );

    assert_eq!(thumbnails(&harness), ["a.png", "c.png"]);
    harness.get_by_label_contains("Couldn't open b.txt");
    harness.get_by_label_contains("Couldn't open d.txt");
    harness.get_by_label("Current: c.png");
}

#[test]
fn pages_hold_their_place_while_loading() {
    let gate = Arc::new(RwLock::new(()));
    let app = NotebookApp::default().with_loader(gated_loader(gate.clone()));
    let mut harness = harness_for(app);
    drop_files(&mut harness, &[("a.png", png()), ("b.png", png())]);
    harness.get_by_label("b.png").click();
    settle(&mut harness);
    press_command(&mut harness, egui::Key::ArrowLeft);

    let held = gate.write().unwrap();
    start_dropping(&mut harness, &[("d.png", png()), ("c.png", png())]);
    harness.step();
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "c.png", "d.png"]);
    harness.get_by_label("Loading 2…");
    // A page that's loading can be selected, but there's nothing to show yet.
    harness.get_by_label("c.png").click();
    harness.step();
    harness.step();
    assert_eq!(selected_thumbnails(&harness), ["c.png"]);
    assert!(harness.query_by_label_contains("Current: c.png").is_none());

    drop(held);
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "c.png", "d.png"]);
    assert!(harness.query_by_label_contains("Loading").is_none());
    harness.get_by_label("Current: c.png");
}

#[test]
fn the_viewer_shows_the_thumbnail_until_the_page_is_decoded() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png()), ("b.png", page_photo())]);

    harness.get_by_label("b.png").click();
    // Decoded off the UI thread, so not in the frame the click lands in.
    harness.step();
    harness.get_by_label("Current: b.png (loading)");
    settle(&mut harness);
    harness.get_by_label("Current: b.png");

    harness.get_by_label("Cleaned").click();
    harness.step();
    harness.get_by_label("Current: b.png (cleaned, loading)");
    settle(&mut harness);
    harness.get_by_label("Current: b.png (cleaned)");
}

#[test]
fn cleaned_view_shows_the_cleaned_page() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", page_photo())]);

    harness.get_by_label("Cleaned").click();
    settle(&mut harness);
    harness.get_by_label("Current: a.png (cleaned)");

    harness.get_by_label("Original").click();
    settle(&mut harness);
    harness.get_by_label("Current: a.png");
}

#[test]
fn view_stays_when_selecting_another_page() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    harness.get_by_label("Cleaned").click();
    settle(&mut harness);

    harness.get_by_label("b.png").click();
    settle(&mut harness);

    harness.get_by_label("Current: b.png (cleaned)");
}

/// The page's corners are drawn relative to the image's rect, so that rect must be where the image
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

/// `page_photo` in the Cleaned view, cleaned.
fn cleaned_view(harness: &mut Harness<'_, NotebookApp>) {
    drop_files(harness, &[("a.png", page_photo())]);
    harness.get_by_label("Cleaned").click();
    settle(harness);
}

/// Where pixel `(x, y)` of `page_photo`, 200 x 150, is on screen in a.png's photo.
fn on_photo(harness: &Harness<'_, NotebookApp>, x: f32, y: f32) -> egui::Pos2 {
    let bounds = harness.get_by_label("Current: a.png");
    let bounds = bounds.accesskit_node().bounding_box().unwrap();
    egui::pos2(
        (bounds.x0 + bounds.width() * f64::from(x) / 200.0) as f32,
        (bounds.y0 + bounds.height() * f64::from(y) / 150.0) as f32,
    )
}

/// Width over height of a.png's cleaned image.
fn cleaned_aspect(harness: &Harness<'_, NotebookApp>) -> f64 {
    let image = harness.get_by_label("Current: a.png (cleaned)");
    let bounds = image.accesskit_node().bounding_box().unwrap();
    bounds.width() / bounds.height()
}

fn assert_near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} isn't within {tolerance} of {expected}"
    );
}

fn disabled(harness: &Harness<'_, NotebookApp>, label: &str) -> bool {
    harness.get_by_label(label).accesskit_node().is_disabled()
}

/// Drags the bottom-right corner to `(x, y)` on the photo, and waits for the page to be cleaned
/// again.
fn move_bottom_right(harness: &mut Harness<'_, NotebookApp>, x: f32, y: f32) {
    let from = centre(harness, "Bottom-right corner");
    let to = on_photo(harness, x, y);
    drag(harness, from, to);
    settle(harness);
}

#[test]
fn cleaned_view_shows_the_photo_with_its_corners_beside_the_cleaned_page() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    harness.get_by_label("Current: a.png");
    // The page is 120 x 110.
    assert_near(cleaned_aspect(&harness), 120.0 / 110.0, 0.1);
    let corner = centre(&harness, "Top-left corner");
    assert!(
        corner.distance(on_photo(&harness, 40.0, 20.0)) < 8.0,
        "{corner:?}"
    );
    for label in [
        "Top-right corner",
        "Bottom-right corner",
        "Bottom-left corner",
    ] {
        harness.get_by_label(label);
    }
    assert!(disabled(&harness, "Reset Corners"));

    harness.get_by_label("Original").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Top-left corner").is_none());
}

#[test]
fn dragging_a_corner_cleans_the_page_again() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    let before = cleaned_aspect(&harness);

    move_bottom_right(&mut harness, 160.0, 75.0);
    let corner = centre(&harness, "Bottom-right corner");
    assert!(
        corner.distance(on_photo(&harness, 160.0, 75.0)) < 2.0,
        "{corner:?}"
    );
    // Straightened to the average of opposite sides: (120 + 132) / 2 by (110 + 55) / 2.
    assert_near(cleaned_aspect(&harness), 126.0 / 82.5, 0.1);

    harness.get_by_label("Reset Corners").click();
    settle(&mut harness);
    assert_near(cleaned_aspect(&harness), before, 0.01);
    assert!(disabled(&harness, "Reset Corners"));
}

#[test]
fn corners_show_they_can_be_dragged() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    let cursor = |harness: &Harness<'_, NotebookApp>| harness.output().platform_output.cursor_icon;

    let corner = centre(&harness, "Top-left corner");
    harness.hover_at(corner);
    harness.step();
    assert_eq!(cursor(&harness), egui::CursorIcon::Grab);

    hold(&mut harness, corner, corner + egui::vec2(20.0, 20.0));
    assert_eq!(cursor(&harness), egui::CursorIcon::Grabbing);
}

#[test]
fn placing_corners_takes_four_clicks_in_any_order() {
    let mut harness = harness();
    cleaned_view(&mut harness);

    harness.get_by_label("Place Corners").click();
    settle(&mut harness);
    harness.get_by_label_contains("Click the page's four corners");
    assert!(harness.query_by_label("Top-left corner").is_none());
    for (x, y) in [(160.0, 75.0), (40.0, 20.0), (40.0, 75.0), (160.0, 20.0)] {
        let at = on_photo(&harness, x, y);
        click_at(&mut harness, at);
    }
    settle(&mut harness);

    // The top half of the page, 120 x 55.
    assert_near(cleaned_aspect(&harness), 120.0 / 55.0, 0.1);
    let corner = centre(&harness, "Top-left corner");
    assert!(
        corner.distance(on_photo(&harness, 40.0, 20.0)) < 2.0,
        "{corner:?}"
    );
    assert!(!disabled(&harness, "Reset Corners"));
}

#[test]
fn escape_stops_placing_corners() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    let before = cleaned_aspect(&harness);

    harness.get_by_label("Place Corners").click();
    settle(&mut harness);
    for (x, y) in [(40.0, 20.0), (160.0, 20.0)] {
        let at = on_photo(&harness, x, y);
        click_at(&mut harness, at);
    }
    press(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);

    harness.get_by_label("Top-left corner");
    assert!(
        harness
            .query_by_label_contains("Click the page's four corners")
            .is_none()
    );
    assert_near(cleaned_aspect(&harness), before, 0.01);
    assert!(disabled(&harness, "Reset Corners"));
}

/// Where the corners' handles were painted last frame.
fn painted_handles(harness: &Harness<'_, NotebookApp>) -> Vec<egui::Pos2> {
    let circles = harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Circle(circle) if circle.radius == 5.0 => Some(circle.center),
            _ => None,
        });
    circles.collect()
}

/// The four handles' centres, in [`Quad`] order, as a quad. The photo is drawn at the same scale
/// both ways, so its shape is the outline's.
fn handles(harness: &Harness<'_, NotebookApp>) -> Quad {
    let labels = [
        "Top-left corner",
        "Top-right corner",
        "Bottom-right corner",
        "Bottom-left corner",
    ];
    labels.map(|label| {
        let at = centre(harness, label);
        [at.x, at.y]
    })
}

#[test]
fn a_corner_stays_where_its_let_go() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    let from = centre(&harness, "Bottom-right corner");
    let to = on_photo(&harness, 160.0, 75.0);
    hold(&mut harness, from, to);
    // Only the frame it's let go in, before the page has its new corners: `drop_at` would run
    // another after it.
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    let painted = painted_handles(&harness);
    assert!(
        painted.iter().any(|p| p.distance(to) < 1.0),
        "{painted:?} doesn't include {to:?}"
    );
    assert!(
        painted.iter().all(|p| p.distance(from) > 1.0),
        "{painted:?}"
    );
}

#[test]
fn corners_cant_be_dragged_out_of_a_page_shape() {
    // Into the page, denting it, or past the opposite corner, crossing its sides.
    for (x, y) in [(130.0, 100.0), (190.0, 145.0)] {
        let mut harness = harness();
        cleaned_view(&mut harness);
        let from = centre(&harness, "Top-left corner");
        let to = on_photo(&harness, x, y);
        // In one jump, so it has to find the limit rather than stop short of it.
        harness.hover_at(from);
        harness.step();
        harness.drag_at(from);
        harness.step();
        harness.hover_at(to);
        harness.step();
        harness.drop_at(to);
        settle(&mut harness);
        assert!(
            is_page_shaped(&handles(&harness)),
            "{:?}",
            handles(&harness)
        );
        // Stopped at the limit, not sent back where it was.
        let moved = centre(&harness, "Top-left corner").distance(from);
        assert!(moved > 20.0, "moved only {moved}");
        harness.get_by_label("Current: a.png (cleaned)");
    }
}

#[test]
fn a_fourth_corner_that_spoils_the_outline_is_refused() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    harness.get_by_label("Place Corners").click();
    settle(&mut harness);
    // The fourth is inside the triangle of the other three.
    for (x, y) in [(40.0, 20.0), (160.0, 20.0), (40.0, 130.0), (70.0, 50.0)] {
        let at = on_photo(&harness, x, y);
        click_at(&mut harness, at);
    }
    settle(&mut harness);
    harness.get_by_label_contains("Those corners don't outline a page");
    assert!(harness.query_by_label("Top-left corner").is_none());

    let at = on_photo(&harness, 160.0, 130.0);
    click_at(&mut harness, at);
    settle(&mut harness);
    assert!(is_page_shaped(&handles(&harness)));
    assert_near(cleaned_aspect(&harness), 120.0 / 110.0, 0.05);
}

#[test]
fn only_the_latest_corners_are_kept() {
    let gate = Arc::new(RwLock::new(()));
    let app = NotebookApp::default().with_loader(gated_loader(gate.clone()));
    let mut harness = harness_for(app);
    cleaned_view(&mut harness);

    let held = gate.write().unwrap();
    for y in [75.0, 40.0] {
        let from = centre(&harness, "Bottom-right corner");
        let to = on_photo(&harness, 160.0, y);
        drag(&mut harness, from, to);
    }
    // The old cleaned page stays up while the new one is made.
    harness.get_by_label("Current: a.png (cleaned, loading)");

    drop(held);
    settle(&mut harness);
    // (120 + 150) / 2 by (110 + 20) / 2.
    assert_near(cleaned_aspect(&harness), 135.0 / 65.0, 0.1);
}

#[test]
fn transcription_uses_the_users_corners() {
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let fetch: Fetch = {
        let sent = sent.clone();
        Arc::new(move |_, _, corners, _| {
            sent.lock().unwrap().push(corners);
            Ok(recorded())
        })
    };
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    assert!(harness.query_by_label_contains("Bounds changed").is_none());

    harness.get_by_label("Cleaned").click();
    settle(&mut harness);
    move_bottom_right(&mut harness, 160.0, 75.0);
    show_transcript(&mut harness);
    harness.get_by_label_contains("Bounds changed since this was transcribed");

    harness.get_by_label("Force retranscription").click();
    step_until_transcribed(&mut harness);
    step_until(&mut harness, "# Thurs 23 Oct");
    assert!(harness.query_by_label_contains("Bounds changed").is_none());
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], None);
    let [_, _, [x, y], _] = sent[1].expect("the user's corners");
    assert!(
        (x - 160.0).abs() < 2.0 && (y - 75.0).abs() < 2.0,
        "{:?}",
        sent[1]
    );
}

/// Without a transcriber (no credentials), there's nothing to transcribe or export.
#[test]
fn controls_without_a_transcriber() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png())]);

    for label in ["Original", "Cleaned", "Delete"] {
        let node = harness.get_by_label(label);
        assert!(!node.accesskit_node().is_disabled(), "{label} is disabled");
    }
    for label in [
        "Transcribed",
        "Transcription model",
        "Transcribe all",
        "Re-export",
        "Export",
    ] {
        let node = harness.get_by_label(label);
        assert!(node.accesskit_node().is_disabled(), "{label} is enabled");
    }
}

#[test]
fn delete_selects_the_next_page() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );
    harness.get_by_label("b.png").click();
    settle(&mut harness);

    harness.get_by_label("Delete").click();
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["a.png", "c.png"]);
    harness.get_by_label("Current: c.png");

    // The last page selects the one before it.
    harness.get_by_label("Delete").click();
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["a.png"]);
    harness.get_by_label("Current: a.png");

    harness.get_by_label("Delete").click();
    settle(&mut harness);
    harness.get_by_label("Drop image files here");
}

#[test]
fn deleting_a_transcribed_page_asks_first() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    harness.get_by_label("Delete").click();
    settle(&mut harness);
    harness.get_by_label("Delete a.png and its transcript?");
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);

    harness.get_by_label("Delete").click();
    settle(&mut harness);
    harness.get_by_label("Delete page").click();
    settle(&mut harness);
    harness.get_by_label("b.png isn't transcribed yet.");
    assert_eq!(thumbnails(&harness), ["b.png"]);
}

#[test]
fn transcribed_view_transcribes_once() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    harness.get_by_label("Transcribed").click();
    step_until(&mut harness, "# Thurs 23 Oct");
    harness.get_by_label_contains("~~tap~~ shower");

    harness.get_by_label("Original").click();
    settle(&mut harness);
    harness.get_by_label("Current: a.png");
    harness.get_by_label("Transcribed").click();
    settle(&mut harness);
    harness.get_by_label_contains("# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_transcription_can_be_retried() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let fetch: Fetch = Arc::new(
        move |_, _, _, _| match counter.fetch_add(1, Ordering::SeqCst) {
            0 => Err(overloaded()),
            _ => Ok(recorded()),
        },
    );
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    harness.get_by_label("Transcribed").click();
    step_until(
        &mut harness,
        "Couldn't transcribe a.png: Claude API error 529",
    );
    // A failure isn't retried by itself.
    settle(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    harness.get_by_label("Retry").click();
    step_until(&mut harness, "# Thurs 23 Oct");
    assert!(
        harness
            .query_by_label_contains("Couldn't transcribe")
            .is_none()
    );
}

#[test]
fn selecting_another_page_asks_before_transcribing() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    harness.get_by_label("b.png").click();
    settle(&mut harness);
    harness.get_by_label("b.png isn't transcribed yet.");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    harness.get_by_label("Transcribe").click();
    step_until(&mut harness, "# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn switching_to_transcribed_transcribes_the_shown_page() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    harness.get_by_label("b.png").click();
    harness.get_by_label("Cleaned").click();
    settle(&mut harness);

    harness.get_by_label("Transcribed").click();
    step_until(&mut harness, "# Thurs 23 Oct");
    harness.get_by_label("Current: b.png (cleaned)");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn cached_pages_show_straight_away() {
    let dir = cache_dir("show_straight_away");
    let (fetch, _) = counting_fetch();
    let mut harness = cached_harness(fetch, &dir);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    // Later, the same photo under another name, after an uncached one.
    let (fetch, calls) = counting_fetch();
    let mut harness = cached_harness(fetch, &dir);
    drop_files(&mut harness, &[("a.png", png()), ("b.png", page_photo())]);
    show_transcript(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    harness.get_by_label("b.png").click();
    step_until(&mut harness, "# Thurs 23 Oct");
    assert!(harness.query_by_label("Transcribe").is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1, "b.png not sent");
    harness.get_by_label_contains("From cache: transcribed 20");
    harness.get_by_label_contains(" by claude-opus-5-5");
}

#[test]
fn cached_pages_load_without_a_transcriber() {
    let dir = cache_dir("without_transcriber");
    let (fetch, _) = counting_fetch();
    let mut harness = cached_harness(fetch, &dir);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    let app = NotebookApp::with_transcription(None, Some(Cache::new(&dir)));
    let mut harness = harness_for(app);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    for _ in 0..500 {
        harness.step();
        if !harness.state().eval_log().events().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(
        harness.state().eval_log().events(),
        [Event::Transcribed {
            from_cache: true,
            ..
        }]
    ));
}

/// Answers with the recorded responses in `answers`, in turn, sleeping for `delay` first.
fn answering(
    answers: Vec<Result<Fetched, OcrError>>,
    delay: Duration,
) -> (Fetch, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let answers = std::sync::Mutex::new(answers.into_iter());
    let fetch: Fetch = Arc::new(move |_, _, _, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(delay);
        answers.lock().unwrap().next().expect("an answer left")
    });
    (fetch, calls)
}

/// Checks the status line containing `status` has the Force button on its row, to its right and
/// within the page's half of the window.
fn force_beside(harness: &Harness<'_, NotebookApp>, status: &str) {
    let text = harness.get_by_label_contains(status);
    let text = text.accesskit_node().bounding_box().unwrap();
    let button = harness.get_by_label("Force retranscription");
    let button = button.accesskit_node().bounding_box().unwrap();
    assert!(
        button.y0 < text.y1 && text.y0 < button.y1,
        "{text:?}, {button:?}"
    );
    assert!(text.x1 <= button.x0, "{text:?}, {button:?}");
    let half = harness.ctx.content_rect().center().x;
    assert!(button.x1 <= f64::from(half), "{button:?}");
}

fn overloaded() -> OcrError {
    OcrError::Api {
        status: 529,
        kind: "overloaded_error".into(),
        message: "Overloaded".into(),
    }
}

#[test]
fn force_retranscribes_without_asking_if_uncorrected() {
    let answers = vec![Ok(recorded()), Ok(recorded_fixture("fallback"))];
    let (fetch, calls) = answering(answers, Duration::from_millis(100));
    let mut harness = cached_harness(fetch, &cache_dir("force"));
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    force_beside(&harness, "Transcribed 20");

    harness.get_by_label("Force retranscription").click();
    harness.step();
    harness.step();
    // Progress replaces the old transcript meanwhile.
    harness.get_by_label("Transcribing a.png…");
    assert!(harness.query_by_label("# Thurs 23 Oct").is_none());
    assert!(harness.query_by_label("Force retranscription").is_none());

    step_until(&mut harness, "by claude-opus-4-8");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn the_selected_model_transcribes_and_is_compared_with_the_other() {
    // Answers as the model asked.
    let fetch: Fetch = Arc::new(|_, _, _, model| {
        let recorded = recorded();
        let body = String::from_utf8(recorded.body).unwrap().replace(
            "\"model\": \"claude-opus-5-5\"",
            &format!("\"model\": \"{}\"", model.id()),
        );
        Ok(Fetched {
            body: body.into_bytes(),
            ..recorded
        })
    });
    // Each page sent, by whom, and how it compares with other models' cached transcripts.
    let dir = cache_dir("select_model");
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let on_transcribe_completed: OnTranscribeCompleted = {
        let (sent, cache) = (sent.clone(), Cache::new(&dir));
        Arc::new(
            move |page: &str, bytes: &[u8], t: &notebook_core::transcript::Transcript| {
                let compared = compare::with_cached(&cache, bytes, t);
                let compared: Vec<_> = compared.iter().map(|(m, c)| (*m, c.similarity)).collect();
                sent.lock()
                    .unwrap()
                    .push((page.to_owned(), t.model.clone(), compared));
            },
        )
    };
    let transcriber =
        Transcriber::new(fetch).on_transcribe_completed(on_transcribe_completed.clone());
    let mut harness = harness_with_cache(transcriber, &dir);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    // Its label, then the dropdown (named by the label), then Sort by name, in a row.
    let label = harness
        .query_all_by_role(Role::Label)
        .find(|n| n.value().as_deref() == Some("Transcription model"))
        .expect("label")
        .accesskit_node()
        .bounding_box()
        .unwrap();
    let bounds = |label| {
        let node = harness.get_by_label(label);
        node.accesskit_node().bounding_box().unwrap()
    };
    let (combo, sort) = (bounds("Transcription model"), bounds("Sort by name"));
    assert!(
        label.x1 <= combo.x0 && combo.x1 <= sort.x0,
        "{label:?} {combo:?} {sort:?}"
    );
    assert!(
        label.y0 < combo.y1 && combo.y0 < label.y1,
        "{label:?} {combo:?}"
    );

    harness.get_by_label("Transcription model").click();
    settle(&mut harness);
    harness.get_by_label("Sonnet").click();
    settle(&mut harness);
    harness.get_by_label("Force retranscription").click();
    step_until(&mut harness, "by claude-sonnet-5-5");
    let sent_so_far = std::mem::take(&mut *sent.lock().unwrap());
    let a = || "a.png".to_owned();
    assert_eq!(
        sent_so_far,
        [
            (a(), "claude-opus-5-5".to_owned(), vec![]),
            (
                a(),
                "claude-sonnet-5-5".to_owned(),
                vec![(Model::Opus, 1.0)]
            ),
        ]
    );
    let cache = Cache::new(&dir);
    let photo = page_photo();
    assert!(cache.get(&photo, Model::Opus).is_some());
    assert!(cache.get(&photo, Model::Sonnet).is_some());

    // Either entry will do for a new page, which isn't sent.
    let (fetch, calls) = counting_fetch();
    let transcriber = Transcriber::new(fetch).on_transcribe_completed(on_transcribe_completed);
    let mut harness = harness_with_cache(transcriber, &dir);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(sent.lock().unwrap().is_empty());
    assert_eq!(events(&harness), ["transcribed from_cache=true"]);
}

#[test]
fn failed_force_keeps_the_corrected_transcript_and_the_cache() {
    let dir = cache_dir("failed_force");
    let answers = vec![Ok(recorded()), Err(overloaded())];
    let (fetch, _) = answering(answers, Duration::ZERO);
    let mut harness = cached_harness(fetch, &dir);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    correct_plumber(&mut harness);

    harness.get_by_label("Force retranscription").click();
    settle(&mut harness);
    harness.get_by_label("Retranscribe a.png? Your corrections will be lost.");
    harness.get_by_label("Retranscribe").click();
    step_until(&mut harness, "Retranscription failed: Claude API error 529");
    harness.get_by_label_contains("Using the cached version, transcribed 20");
    force_beside(&harness, "Retranscription failed");
    harness.get_by_label("plumbers");

    // The cached answer is still Claude's.
    let (fetch, calls) = counting_fetch();
    let mut harness = cached_harness(fetch, &dir);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    harness.get_by_label("plumber");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn transcribe_all_starts_straight_away_for_a_few_pages() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );

    harness.get_by_label("Transcribe all").click();
    step_until_transcribed(&mut harness);
    assert!(harness.query_by_label_contains("pages?").is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let all = harness.get_by_label("Transcribe all");
    assert!(all.accesskit_node().is_disabled(), "nothing left");
}

#[test]
fn transcribe_all_asks_first_and_sends_four_at_a_time() {
    let (calls, running, most) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let fetch: Fetch = {
        let (calls, running, most) = (calls.clone(), running.clone(), most.clone());
        Arc::new(move |_, _, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            running.fetch_sub(1, Ordering::SeqCst);
            Ok(recorded())
        })
    };
    let mut harness = harness_with(Transcriber::new(fetch));
    let names = ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"];
    let files: Vec<_> = names.iter().map(|n| (*n, png())).collect();
    drop_files(&mut harness, &files);

    harness.get_by_label("Transcribe all").click();
    settle(&mut harness);
    harness.get_by_label("Transcribe 6 pages?");
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    harness.get_by_label("Transcribe all").click();
    settle(&mut harness);
    harness.get_by_label("Transcribe").click();
    step_until_transcribed(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    assert_eq!(most.load(Ordering::SeqCst), 4);
}

#[test]
fn transcribe_all_uses_the_cache() {
    let dir = cache_dir("transcribe_all");
    let (fetch, _) = counting_fetch();
    let mut harness = cached_harness(fetch, &dir);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    let (fetch, calls) = counting_fetch();
    let mut harness = cached_harness(fetch, &dir);
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    harness.get_by_label("Transcribe all").click();
    step_until_transcribed(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "only b.png sent");
}

#[test]
fn deleting_a_queued_page_means_it_isnt_sent() {
    let (calls, slow) = (Arc::new(AtomicUsize::new(0)), Duration::from_millis(100));
    let fetch: Fetch = {
        let calls = calls.clone();
        Arc::new(move |_, _, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(slow);
            Ok(recorded())
        })
    };
    let mut harness = harness_with(Transcriber::new(fetch));
    let names = ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"];
    let files: Vec<_> = names.iter().map(|n| (*n, png())).collect();
    drop_files(&mut harness, &files);
    harness.get_by_label("Transcribe all").click();
    settle(&mut harness);
    harness.get_by_label("Transcribe").click();
    harness.step();

    harness.get_by_label("f.png").click();
    harness.step();
    harness.get_by_label("Delete").click();
    harness.step();
    step_until_transcribed(&mut harness);
    assert_eq!(thumbnails(&harness).len(), 5);
    assert_eq!(calls.load(Ordering::SeqCst), 5);
}

#[test]
fn dragging_the_split_widens_the_page() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    let width = |harness: &Harness<'_, NotebookApp>| {
        let image = harness.get_by_label("Current: a.png (cleaned)");
        image.accesskit_node().bounding_box().unwrap().width()
    };
    let before = width(&harness);

    let start = centre(&harness, "Resize the transcript");
    drag(&mut harness, start, egui::pos2(start.x + 60.0, start.y));
    settle(&mut harness);
    assert!(
        width(&harness) > before + 50.0,
        "{before} -> {}",
        width(&harness)
    );
}

#[test]
fn transcribed_view_shows_the_page_with_uncertain_words() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.get_by_label("Current: a.png (cleaned)");
    for word in ["plumber", "w/"] {
        harness.get_by_label(word);
        harness.get_by_label(&format!("Box: {word}"));
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
    harness.get_by_label("Keep \"w/\"").click();
    settle(&mut harness);
    // A word corrected again doesn't finish the page again.
    harness.get_by_label("Box: plumbers").click();
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
        (page.as_str(), model.as_str()),
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
    harness.get_by_label("Box: plumbers");
    assert!(harness.query_by_label("plumber").is_none());

    // The replaced reading is still on offer.
    harness.get_by_label("plumbers").click();
    settle(&mut harness);
    harness.get_by_label("Keep \"plumbers\"");
    harness.get_by_label("plumber").click();
    settle(&mut harness);
    harness.get_by_label("Box: plumber");
}

#[test]
fn boxes_take_a_reading_of_your_own() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.get_by_label("Box: plumber").click();
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
    harness.get_by_label("Keep \"w/\"").click();
    settle(&mut harness);

    harness.get_by_label("w/");
    assert!(harness.query_by_label_contains("Keep").is_none());
}

#[test]
fn double_clicking_a_line_edits_it() {
    // Each click takes a few frames, so the default quarter-second frames are too slow for a
    // double-click.
    let mut harness = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::with_transcription(Some(transcriber()), None),
    );
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
fn export_waits_for_every_page_in_scope() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    harness.get_by_label("Export").click();
    settle(&mut harness);
    harness.get_by_label("Everything").click();
    settle(&mut harness);
    harness.get_by_label("Not transcribed yet: b.png");
    assert!(harness.get_by_label("Go").accesskit_node().is_disabled());

    harness.get_by_label("Current page").click();
    settle(&mut harness);
    assert!(harness.query_by_label_contains("Not transcribed").is_none());
    assert!(!harness.get_by_label("Go").accesskit_node().is_disabled());
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

    harness.get_by_label("Export").click();
    harness.step();
    harness.step();
    harness.get_by_label("Still loading: a.png");
    assert!(harness.get_by_label("Go").accesskit_node().is_disabled());

    drop(held);
    settle(&mut harness);
    assert!(harness.query_by_label_contains("Still loading").is_none());
    assert!(!harness.get_by_label("Go").accesskit_node().is_disabled());
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
    harness.get_by_label("Copied a.png to the clipboard");
    settle(&mut harness);
    assert!(
        !harness
            .get_by_label("Re-export")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn reexport_overwrites_the_same_file() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("viewer-reexport/a.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::remove_file(&path).ok();
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
    assert!(
        harness
            .get_by_label("Re-export")
            .accesskit_node()
            .is_disabled()
    );

    export(&mut harness, &["Current page", "Markdown to file"]);
    step_until(&mut harness, "Exported a.png to ");
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("ring the plumber re:"), "{written}");

    correct_plumber(&mut harness);
    harness.get_by_label("Re-export").click();
    // The frame that starts it still shows the first export's status.
    harness.step();
    step_until(&mut harness, "Exported a.png to ");
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

    export(&mut harness, &["Markdown to clipboard"]);
    step_until(&mut harness, "Copied a.png");
    assert_eq!(
        harness.state_mut().frame_time_for_logs(Some(0.2)),
        Some(0.2),
        "no dialog"
    );

    export(&mut harness, &["Markdown to file"]);
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
    harness.get_by_label("Export").click();
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
            "* Everything",
            "Current page",
            "* Markdown to clipboard",
            "Markdown to file",
            "PDF",
        ]
    );
}

#[test]
fn exports_a_pdf() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("viewer-pdf/notebook.pdf");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::remove_file(&path).ok();
    let save_path: SavePath = {
        let path = path.clone();
        Arc::new(move |suggested| {
            assert_eq!(suggested, "notebook.pdf");
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
    harness.get_by_label("Transcribe all").click();
    step_until_transcribed(&mut harness);

    export(&mut harness, &["Everything", "PDF"]);
    step_until(&mut harness, "Exported 2 pages to ");
    let written = std::fs::read(&path).unwrap();
    assert!(written.starts_with(b"%PDF"));
    assert!(
        !harness
            .get_by_label("Re-export")
            .accesskit_node()
            .is_disabled()
    );
}

/// macOS order: the affirmative button rightmost, after Cancel.
#[test]
fn dialogs_put_the_affirmative_button_last() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    let left = |harness: &Harness<'_, NotebookApp>, label: &str| {
        let node = harness.get_by_label(label);
        node.accesskit_node().bounding_box().unwrap().x0
    };

    harness.get_by_label("Export").click();
    settle(&mut harness);
    assert!(left(&harness, "Cancel") < left(&harness, "Go"));
    harness.get_by_label("Cancel").click();
    settle(&mut harness);

    harness.get_by_label("Delete").click();
    settle(&mut harness);
    assert!(left(&harness, "Cancel") < left(&harness, "Delete page"));
}

#[test]
fn dragging_a_thumbnail_moves_the_page() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );

    let past_c = centre(&harness, "c.png").x + 30.0;
    drag_thumbnail(&mut harness, "a.png", past_c);
    assert_eq!(thumbnails(&harness), ["b.png", "c.png", "a.png"]);

    let before_b = centre(&harness, "b.png").x - 30.0;
    drag_thumbnail(&mut harness, "c.png", before_b);
    assert_eq!(thumbnails(&harness), ["c.png", "b.png", "a.png"]);
    // The page shown before rearranging stays shown.
    harness.get_by_label("Current: a.png");
}

/// The drop marker shows wherever the page would go, even at the very start, and stays clear of
/// the selected page's frame.
#[test]
fn drop_marker_is_visible_beside_the_selected_page() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );
    let a = centre(&harness, "a.png").x;
    // Before a, which is selected and first, then between a and b.
    for x in [a - 30.0, a + 25.0] {
        let end = hold_thumbnail(&mut harness, "c.png", x);
        let (frames, markers) = painted_3pt(&harness);
        let [frame] = frames[..] else {
            panic!("frames: {frames:?}");
        };
        let [marker] = markers[..] else {
            panic!("markers at {x}: {markers:?}");
        };
        assert_eq!(marker.span(), 3.0, "marker at {x} is clipped: {marker:?}");
        let clear = (marker.min - frame.max).max(frame.min - marker.max);
        assert!(clear >= 2.0, "marker {marker:?} touches frame {frame:?}");
        harness.drop_at(end);
        settle(&mut harness);
        assert_eq!(
            thumbnails(&harness).first().map(String::as_str),
            Some(if x < a { "c.png" } else { "a.png" })
        );
        harness.get_by_label("Sort by name").click();
        settle(&mut harness);
    }
}

#[test]
fn thumbnails_have_rounded_corners() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png()), ("b.png", png())]);
    let strip_y = centre(&harness, "a.png").y;

    let in_strip = |rect: &&egui::epaint::RectShape| rect.rect.y_range().contains(strip_y);
    let rects: Vec<&egui::epaint::RectShape> = harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) => Some(rect),
            _ => None,
        })
        .filter(in_strip)
        .collect();
    let thumbs: Vec<_> = rects.iter().filter(|r| r.brush.is_some()).collect();
    assert_eq!(thumbs.len(), 2);
    let radius = thumbs[0].corner_radius;
    assert_ne!(radius, egui::CornerRadius::ZERO);
    assert!(thumbs.iter().all(|r| r.corner_radius == radius));
    // The selected page's frame follows the same curve.
    let frame = rects.iter().find(|r| r.stroke.width == 3.0).unwrap();
    assert_eq!(frame.corner_radius, radius);
}

#[test]
fn dropping_a_thumbnail_beside_itself_leaves_it() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png()), ("b.png", png())]);

    let x = centre(&harness, "a.png").x + 10.0;
    drag_thumbnail(&mut harness, "a.png", x);
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);
}

#[test]
fn new_drops_go_last_after_reordering() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("d.png", png())],
    );
    press_command(&mut harness, egui::Key::ArrowRight);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "d.png"]);

    drop_files(&mut harness, &[("e.png", png()), ("c.png", png())]);
    assert_eq!(
        thumbnails(&harness),
        ["b.png", "a.png", "d.png", "c.png", "e.png"]
    );
}

#[test]
fn sort_by_name_restores_filename_order() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png()), ("b.png", png())]);
    let sort = |harness: &Harness<'_, NotebookApp>| {
        !harness
            .get_by_label("Sort by name")
            .accesskit_node()
            .is_disabled()
    };
    assert!(!sort(&harness), "already sorted");

    press_command(&mut harness, egui::Key::ArrowRight);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png"]);
    assert!(sort(&harness));

    harness.get_by_label("Sort by name").click();
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);
    assert!(!sort(&harness));
    harness.get_by_label("Current: a.png");
}

#[test]
fn command_arrows_move_the_selected_page() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );
    harness.get_by_label("b.png").click();
    settle(&mut harness);

    press_command(&mut harness, egui::Key::ArrowRight);
    assert_eq!(thumbnails(&harness), ["a.png", "c.png", "b.png"]);
    // Already last.
    press_command(&mut harness, egui::Key::ArrowRight);
    assert_eq!(thumbnails(&harness), ["a.png", "c.png", "b.png"]);

    press_command(&mut harness, egui::Key::ArrowLeft);
    press_command(&mut harness, egui::Key::ArrowLeft);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "c.png"]);
    press_command(&mut harness, egui::Key::ArrowLeft);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "c.png"]);
    harness.get_by_label("Current: b.png");
}

#[test]
fn active_page_is_marked_and_named() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );
    assert_eq!(selected_thumbnails(&harness), ["a.png"]);
    harness.get_by_label("a.png · 1 of 3");

    harness.get_by_label("c.png").click();
    settle(&mut harness);
    assert_eq!(selected_thumbnails(&harness), ["c.png"]);
    harness.get_by_label("c.png · 3 of 3");
}

#[test]
fn active_page_scrolls_into_view() {
    // Scrolling is animated, which takes more frames than `run` allows by default.
    let mut harness = Harness::builder().with_max_steps(64).build_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::default(),
    );
    let names: Vec<String> = (0..20).map(|i| format!("{i:02}.png")).collect();
    let files: Vec<(&str, Vec<u8>)> = names.iter().map(|n| (n.as_str(), png())).collect();
    drop_files(&mut harness, &files);

    for _ in 1..names.len() {
        press_command(&mut harness, egui::Key::ArrowRight);
    }
    assert_eq!(
        thumbnails(&harness).last().map(String::as_str),
        Some("00.png")
    );
    let screen = harness.ctx.content_rect();
    let x = centre(&harness, "00.png").x;
    assert!(x < screen.right(), "00.png at {x}, screen {screen:?}");
}

#[test]
fn export_follows_page_order() {
    let mut harness = harness_with(transcriber());
    drop_files(
        &mut harness,
        &[("a.png", page_photo()), ("b.png", page_photo())],
    );
    show_transcript(&mut harness);
    harness.get_by_label("b.png").click();
    settle(&mut harness);
    harness.get_by_label("Transcribe").click();
    step_until(&mut harness, "Current: b.png");
    step_until(&mut harness, "# Thurs 23 Oct");

    press_command(&mut harness, egui::Key::ArrowLeft);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png"]);
    let copied = export_to_clipboard(&mut harness);
    let b = copied.find("<!-- b.png -->").unwrap();
    let a = copied.find("<!-- a.png -->").unwrap();
    assert!(b < a, "{copied}");
}

#[test]
fn arrows_select_the_previous_and_next_page() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );
    let none = egui::Modifiers::NONE;

    // Already first.
    press(&mut harness, none, egui::Key::ArrowLeft);
    harness.get_by_label("Current: a.png");
    for _ in 0..3 {
        press(&mut harness, none, egui::Key::ArrowRight);
    }
    harness.get_by_label("Current: c.png");
    press(&mut harness, none, egui::Key::ArrowLeft);
    harness.get_by_label("Current: b.png");
    assert_eq!(thumbnails(&harness), ["a.png", "b.png", "c.png"]);
}

#[test]
fn command_numbers_switch_the_view() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    press_command(&mut harness, egui::Key::Num2);
    harness.get_by_label("Current: a.png (cleaned)");
    press_command(&mut harness, egui::Key::Num1);
    harness.get_by_label("Current: a.png");
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
    harness.get_by_label("Delete b.png and its transcript?");
    press(&mut harness, egui::Modifiers::NONE, egui::Key::ArrowRight);
    press_command(&mut harness, egui::Key::Num1);
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["b.png", "c.png"]);
    harness.get_by_label("Current: b.png (cleaned)");
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
    harness.get_by_label_contains("a.png is already transcribed.");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // It goes after a few seconds.
    harness.run_steps(20);
    assert!(
        harness
            .query_by_label_contains("already transcribed")
            .is_none()
    );
}

#[test]
fn command_t_retries_a_failed_page() {
    let answers = vec![Err(overloaded()), Ok(recorded())];
    let (fetch, calls) = answering(answers, Duration::ZERO);
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    press_command(&mut harness, egui::Key::T);
    step_until(&mut harness, "Couldn't transcribe a.png");
    press_command(&mut harness, egui::Key::T);
    step_until(&mut harness, "# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn command_shift_t_forces_retranscription() {
    let answers = vec![Ok(recorded()), Ok(recorded_fixture("fallback"))];
    let (fetch, calls) = answering(answers, Duration::ZERO);
    let mut harness = cached_harness(fetch, &cache_dir("command_shift_t"));
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    press(&mut harness, COMMAND_SHIFT, egui::Key::T);
    step_until(&mut harness, "by claude-opus-4-8");
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    // A corrected page asks first.
    correct_plumber(&mut harness);
    press(&mut harness, COMMAND_SHIFT, egui::Key::T);
    harness.get_by_label("Retranscribe a.png? Your corrections will be lost.");
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn command_e_exports_then_reexports() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("viewer-command-e/a.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::remove_file(&path).ok();
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
    for option in ["Current page", "Markdown to file", "Go"] {
        harness.get_by_label(option).click();
        settle(&mut harness);
    }
    step_until(&mut harness, "Exported a.png to ");

    correct_plumber(&mut harness);
    press_command(&mut harness, egui::Key::E);
    assert!(harness.query_by_label("Go").is_none());
    step_until(&mut harness, "Exported a.png to ");
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("ring the plumbers re:"), "{written}");
    assert_eq!(asked.load(Ordering::SeqCst), 1);

    // Export… always asks.
    press(&mut harness, COMMAND_SHIFT, egui::Key::E);
    harness.get_by_label("Go");
}

#[test]
fn add_pages_adds_the_picked_files() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("viewer-add-pages");
    std::fs::create_dir_all(&dir).unwrap();
    let paths = vec![dir.join("b.png"), dir.join("a.png")];
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
    let mut harness = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::with_transcription(Some(transcriber()), None),
    );
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
    harness.get_by_label("Current: a.png (cleaned)");
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

    harness.get_by_label("Transcribe all").click();
    settle(&mut harness);
    harness.get_by_label("Transcribe 6 pages?");
    enter(&mut harness);
    step_until_transcribed(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 6);

    // Dangerous ones too.
    show_transcript(&mut harness);
    correct_plumber(&mut harness);
    harness.get_by_label("Force retranscription").click();
    settle(&mut harness);
    harness.get_by_label("Retranscribe a.png? Your corrections will be lost.");
    enter(&mut harness);
    step_until_transcribed(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 7);

    harness.get_by_label("Export").click();
    settle(&mut harness);
    enter(&mut harness);
    step_until(&mut harness, "Copied 6 pages to the clipboard");

    harness.get_by_label("Delete").click();
    settle(&mut harness);
    harness.get_by_label("Delete a.png and its transcript?");
    enter(&mut harness);
    assert_eq!(thumbnails(&harness), &names[1..]);
}

#[test]
fn enter_does_nothing_while_the_affirmative_button_is_disabled() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    harness.get_by_label("Export").click();
    settle(&mut harness);
    harness.get_by_label("Not transcribed yet: b.png");
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);
    harness.get_by_label("Go");
    assert!(harness.query_by_label_contains("Copied").is_none());
}

#[test]
fn enter_is_the_focused_widgets() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.get_by_label("Delete").click();
    settle(&mut harness);
    harness.get_by_label("Cancel").focus();
    settle(&mut harness);
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);
    assert!(harness.query_by_label_contains("Delete a.png").is_none());
    assert_eq!(thumbnails(&harness), ["a.png"]);
}
