use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use eframe::egui::{
    self,
    accesskit::{Role, Toggled},
};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::{NotebookApp, SavePath, Transcriber};
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
    harness_for(NotebookApp::with_transcriber(Some(transcriber)))
}

fn harness_for(app: NotebookApp) -> Harness<'static, NotebookApp> {
    Harness::new_ui_state(|ui, app: &mut NotebookApp| app.ui(ui), app)
}

/// Every page transcribes to the recorded response.
fn transcriber() -> Transcriber {
    Arc::new(|_, _| Ok(recorded_transcript()))
}

/// Opens the Transcribed view of the selected page and waits for its transcript.
fn show_transcript(harness: &mut Harness<'_, NotebookApp>) {
    harness.get_by_label("Transcribed").click();
    step_until(harness, "# Thurs 23 Oct");
}

/// Opens the export dialog, picks `options` in order, and clicks Go.
fn export(harness: &mut Harness<'_, NotebookApp>, options: &[&str]) {
    harness.get_by_label("Export").click();
    harness.run();
    for option in options {
        harness.get_by_label(option).click();
        harness.run();
    }
    harness.get_by_label("Go").click();
}

/// Corrects the transcript's uncertain "plumber" to "plumbers", from the text.
fn correct_plumber(harness: &mut Harness<'_, NotebookApp>) {
    harness.get_by_label("plumber").click();
    harness.run();
    harness.get_by_label("plumbers").click();
    harness.run();
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
    harness.run();
}

/// Drags the thumbnail labelled `label` to `x`, at the same height, without dropping it, and
/// returns where the pointer is.
fn hold_thumbnail(harness: &mut Harness<'_, NotebookApp>, label: &str, x: f32) -> egui::Pos2 {
    let start = centre(harness, label);
    harness.hover_at(start);
    harness.run();
    harness.drag_at(start);
    harness.run();
    for step in 1..=5 {
        let t = step as f32 / 5.0;
        harness.hover_at(egui::pos2(start.x + (x - start.x) * t, start.y));
        harness.run();
    }
    egui::pos2(x, start.y)
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
    harness.key_press_modifiers(egui::Modifiers::COMMAND, key);
    harness.run();
}

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

/// Without a transcriber (no credentials), there's nothing to transcribe or export.
#[test]
fn controls_without_a_transcriber() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png())]);

    for label in ["Original", "Cleaned", "Debug", "Delete"] {
        let node = harness.get_by_label(label);
        assert!(!node.accesskit_node().is_disabled(), "{label} is disabled");
    }
    for label in ["Transcribed", "Re-export", "Export"] {
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
    harness.run();

    harness.get_by_label("Delete").click();
    harness.run();
    assert_eq!(thumbnails(&harness), ["a.png", "c.png"]);
    harness.get_by_label("Current: c.png");

    // The last page selects the one before it.
    harness.get_by_label("Delete").click();
    harness.run();
    assert_eq!(thumbnails(&harness), ["a.png"]);
    harness.get_by_label("Current: a.png");

    harness.get_by_label("Delete").click();
    harness.run();
    harness.get_by_label("Drop image files here");
}

#[test]
fn deleting_a_transcribed_page_asks_first() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    harness.get_by_label("Delete").click();
    harness.run();
    harness.get_by_label("Delete a.png and its transcript?");
    harness.get_by_label("Cancel").click();
    harness.run();
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);

    harness.get_by_label("Delete").click();
    harness.run();
    harness.get_by_label("Delete page").click();
    // b.png starts transcribing once it's selected, so its spinner keeps the UI from settling.
    step_until(&mut harness, "Current: b.png");
    assert_eq!(thumbnails(&harness), ["b.png"]);
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
    harness.run();
    harness.get_by_label("Keep \"plumbers\"");
    harness.get_by_label("plumber").click();
    harness.run();
    harness.get_by_label("Box: plumber");
}

#[test]
fn boxes_take_a_reading_of_your_own() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    harness.get_by_label("Box: plumber").click();
    harness.run();
    let field = harness.get_by_role(Role::TextInput);
    field.focus();
    field.type_text("Plumbing Co");
    harness.run_ok();
    harness.key_press(egui::Key::Enter);
    harness.run();

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
    harness.run();
    harness.get_by_label("Keep \"w/\"").click();
    harness.run();

    harness.get_by_label("w/");
    assert!(harness.query_by_label_contains("Keep").is_none());
}

#[test]
fn double_clicking_a_line_edits_it() {
    // Each click takes a few frames, so the default quarter-second frames are too slow for a
    // double-click.
    let mut harness = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::with_transcriber(Some(transcriber())),
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
    harness.run();

    harness.get_by_label("# Thurs 23 Oct 2026");
    assert!(harness.query_by_role(Role::TextInput).is_none());

    // Escape drops the edit.
    harness.run_steps(30);
    let line = harness.get_by_label("# Thurs 23 Oct 2026");
    line.click();
    line.click();
    harness.run_ok();
    harness.get_by_role(Role::TextInput).type_text("!!");
    harness.run_ok();
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.get_by_label("# Thurs 23 Oct 2026");
}

#[test]
fn export_waits_for_every_page_in_scope() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    harness.get_by_label("Export").click();
    harness.run();
    harness.get_by_label("Everything").click();
    harness.run();
    harness.get_by_label("Not transcribed yet: b.png");
    assert!(harness.get_by_label("Go").accesskit_node().is_disabled());

    harness.get_by_label("Current page").click();
    harness.run();
    assert!(harness.query_by_label_contains("Not transcribed").is_none());
    assert!(!harness.get_by_label("Go").accesskit_node().is_disabled());
}

#[test]
fn exports_markdown_to_the_clipboard() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    correct_plumber(&mut harness);

    export(&mut harness, &["Markdown to clipboard"]);
    harness.step();
    let copied: Vec<&str> = harness
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(copied.len(), 1);
    assert!(
        copied[0].starts_with("<!-- a.png -->\n# Thurs 23 Oct\n- ring the plumbers re:"),
        "{}",
        copied[0]
    );
    harness.run();
    harness.get_by_label("Copied a.png to the clipboard");
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
    let mut harness =
        harness_for(NotebookApp::with_transcriber(Some(transcriber())).with_save_path(save_path));
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    assert!(
        harness
            .get_by_label("Re-export")
            .accesskit_node()
            .is_disabled()
    );

    export(&mut harness, &["Current page", "Markdown to file"]);
    harness.run();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("ring the plumber re:"), "{written}");
    harness.get_by_label_contains("Exported a.png to ");

    correct_plumber(&mut harness);
    harness.get_by_label("Re-export").click();
    harness.run();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("ring the plumbers re:"), "{written}");
    assert_eq!(asked.load(Ordering::SeqCst), 1, "asked where to save once");
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
    harness.run();
    assert!(left(&harness, "Cancel") < left(&harness, "Go"));
    harness.get_by_label("Cancel").click();
    harness.run();

    harness.get_by_label("Delete").click();
    harness.run();
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
        harness.run();
        assert_eq!(
            thumbnails(&harness).first().map(String::as_str),
            Some(if x < a { "c.png" } else { "a.png" })
        );
        harness.get_by_label("Sort by name").click();
        harness.run();
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
    harness.run();
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
    harness.run();

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
    harness.run();
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
    step_until(&mut harness, "Current: b.png");
    step_until(&mut harness, "# Thurs 23 Oct");

    press_command(&mut harness, egui::Key::ArrowLeft);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png"]);
    export(&mut harness, &["Everything", "Markdown to clipboard"]);
    harness.step();
    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap();
    let b = copied.find("<!-- b.png -->").unwrap();
    let a = copied.find("<!-- a.png -->").unwrap();
    assert!(b < a, "{copied}");
}
