//! Helpers the viewer tests share: harnesses, stand-ins for files, Claude and dialogs, and ways to
//! drive the UI.
#![allow(dead_code)]

use eframe::egui;
use eframe::egui::{accesskit::Role, accesskit::Toggled};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::{Command, Event, Fetch, Load, NotebookApp, Transcriber, t};
use notebook_core::cache::Cache;
use notebook_core::claude_transcribe::{Fetched, OcrError, image_to_send};
use notebook_core::clean::{CleanParams, Crop};
use notebook_core::decode::decode;
use notebook_core::geom::Quad;
use notebook_core::loaded::Loaded;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Stands in for a file dragged from the desktop.
#[derive(Debug)]
pub struct TestFile {
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

pub fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::new(40, 30)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// A cream page on a brown table, which cleanup finds an outline for.
pub fn page_photo() -> Vec<u8> {
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

pub fn harness() -> Harness<'static, NotebookApp> {
    Harness::new_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::default(),
    )
}

pub fn harness_with(transcriber: Transcriber) -> Harness<'static, NotebookApp> {
    harness_for(NotebookApp::with_transcription(Some(transcriber), None))
}

/// Transcribes with `transcriber`, caching in `cache_dir`.
pub fn harness_with_cache(
    transcriber: Transcriber,
    cache_dir: &Path,
) -> Harness<'static, NotebookApp> {
    harness_for(NotebookApp::with_transcription(
        Some(transcriber),
        Some(Cache::new(cache_dir)),
    ))
}

pub fn harness_for(app: NotebookApp) -> Harness<'static, NotebookApp> {
    Harness::new_ui_state(|ui, app: &mut NotebookApp| app.ui(ui), app)
}

/// Every page transcribes to the recorded response, without a cache.
pub fn transcriber() -> Transcriber {
    Transcriber::new(Arc::new(|_, _, _, _| Ok(recorded())))
}

/// Answers every request with the recorded response, counting them.
pub fn counting_fetch() -> (Fetch, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let fetch: Fetch = Arc::new(move |_, _, _, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(recorded())
    });
    (fetch, calls)
}

/// An empty directory of the test's own, removed when dropped. Unique, so overlapping runs of the
/// suite, from two terminals say, don't share files. Harnesses given the same one share a cache.
pub fn test_dir(test: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(&format!("viewer-{test}-"))
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
}

/// [`harness_with_cache`], transcribing with `fetch`.
pub fn cached_harness(fetch: Fetch, cache_dir: &Path) -> Harness<'static, NotebookApp> {
    harness_with_cache(Transcriber::new(fetch), cache_dir)
}

/// Clicks the button that does `command`.
pub fn click_command(harness: &Harness<'_, NotebookApp>, command: Command) {
    harness.get_by_label(&command.label()).click();
}

/// Opens the Transcribed view of the selected page and waits for its transcript.
pub fn show_transcript(harness: &mut Harness<'_, NotebookApp>) {
    click_command(harness, Command::ViewTranscribed);
    step_until(harness, "# Thurs 23 Oct");
}

/// Opens the export dialog, picks `options` in order, and clicks Go.
pub fn export(harness: &mut Harness<'_, NotebookApp>, options: &[&str]) {
    click_command(harness, Command::ExportAs);
    settle(harness);
    for option in options {
        harness.get_by_label(option).click();
        settle(harness);
    }
    harness
        .get_by_role_and_label(Role::Button, &t!("export-go"))
        .click();
}

/// Exports everything to the clipboard as markdown, the dialog's defaults, returning what was
/// copied.
pub fn export_to_clipboard(harness: &mut Harness<'_, NotebookApp>) -> String {
    export(harness, &[]);
    let mut copied: Vec<String> = Vec::new();
    wait_for("the export to be copied", || {
        harness.step();
        copied = harness
            .output()
            .platform_output
            .commands
            .iter()
            .filter_map(|c| match c {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .collect();
        !copied.is_empty()
    });
    assert_eq!(copied.len(), 1, "{copied:?}");
    copied.into_iter().next().unwrap()
}

/// Corrects the transcript's uncertain "plumber" to "plumbers", from the text.
pub fn correct_plumber(harness: &mut Harness<'_, NotebookApp>) {
    harness.get_by_label("plumber").click();
    settle(harness);
    harness.get_by_label("plumbers").click();
    settle(harness);
}

/// The recorded response from core's OCR tests, as if for [`page_photo`].
pub fn recorded() -> Fetched {
    recorded_fixture("success")
}

pub fn recorded_fixture(name: &str) -> Fetched {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../core/tests/fixtures/claude/{name}.json"));
    Fetched {
        status: 200,
        body: std::fs::read(path).unwrap(),
        crop: sent_crop(&page_photo(), None),
    }
}

/// Where `photo` is cut when sent cropped to `corners`, else its detected ones, but at the size of
/// the image the recorded responses are for.
pub fn sent_crop(photo: &[u8], corners: Option<Quad>) -> Crop {
    let photo = decode(photo).unwrap();
    let (_, crop) = image_to_send(&photo, &CleanParams::default(), corners);
    Crop {
        size: (1200, 900),
        ..crop
    }
}

/// Steps frames until a label containing `text` appears. Transcription finishes on another
/// thread, and the spinner keeps requesting repaints, so `run` won't do.
pub fn step_until(harness: &mut Harness<'_, NotebookApp>, text: &str) {
    wait_for(&format!("{text:?} to appear"), || {
        harness.step();
        harness.query_by_label_contains(text).is_some()
    });
}

/// Stands in for a message's argument, to find the message whatever it's given: see
/// [`before_mark`].
pub const MARK: &str = "\u{1}";

/// `text` up to the first [`MARK`] in it.
pub fn before_mark(text: String) -> String {
    text.split(MARK).next().unwrap_or_default().to_owned()
}

/// Says a transcript was made by `model`, whenever that was.
pub fn by(model: &str) -> String {
    t!("transcribed-when", at = "none", time = "", model = model)
}

/// Says `count` pages, or the page `name`, were exported, wherever to.
pub fn exported(count: usize, name: &str) -> String {
    before_mark(t!("export-saved", count = count, name = name, path = MARK))
}

/// Steps frames until nothing is being transcribed. The first one acts on any click.
pub fn step_until_transcribed(harness: &mut Harness<'_, NotebookApp>) {
    harness.step();
    wait_for("transcription to finish", || {
        harness.step();
        !harness.state().transcribing()
    });
}

/// How long to wait for work done off the UI thread. Far longer than it takes, because the whole
/// suite runs at once and a busy machine can starve any one test for seconds.
pub const PATIENCE: Duration = Duration::from_secs(30);

/// Calls `done` until it's true, sleeping in between so other threads get a turn, and fails the
/// test after [`PATIENCE`]. Goes by the clock rather than counting tries, since under load a try
/// can take any time.
pub fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "gave up waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn drop_files(harness: &mut Harness<'_, NotebookApp>, files: &[(&str, Vec<u8>)]) {
    start_dropping(harness, files);
    settle(harness);
}

/// Drops `files` without waiting for them to load.
pub fn start_dropping(harness: &mut Harness<'_, NotebookApp>, files: &[(&str, Vec<u8>)]) {
    start_dropping_from(harness, Path::new("/Users/someone/Desktop"), files);
}

/// Drops `files` as if from `dir`, without waiting for them to load.
fn start_dropping_from(
    harness: &mut Harness<'_, NotebookApp>,
    dir: &Path,
    files: &[(&str, Vec<u8>)],
) {
    for (name, bytes) in files {
        harness.input_mut().dropped_files.push(Arc::new(TestFile {
            path: dir.join(name),
            bytes: bytes.clone(),
        }));
    }
    harness.step();
}

/// Writes `files` to `dir` and drops them from there, so a project can find them again.
pub fn drop_files_in(
    harness: &mut Harness<'_, NotebookApp>,
    dir: &Path,
    files: &[(&str, Vec<u8>)],
) {
    for (name, bytes) in files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    start_dropping_from(harness, dir, files);
    settle(harness);
}

/// Asks to close the window, as its close button does, and returns what the app told the window
/// to do in that frame.
pub fn close_window(harness: &mut Harness<'_, NotebookApp>) -> Vec<egui::ViewportCommand> {
    let root = harness
        .input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default();
    root.events.push(egui::ViewportEvent::Close);
    step_for_commands(harness)
}

/// Steps one frame, and returns what the app told the window to do in it.
pub fn step_for_commands(harness: &mut Harness<'_, NotebookApp>) -> Vec<egui::ViewportCommand> {
    harness.step();
    harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|v| v.commands.clone())
        .unwrap_or_default()
}

/// Loads pages as the app does, but only while `gate` isn't held for writing.
pub fn gated_loader(gate: Arc<RwLock<()>>) -> Load {
    Arc::new(move |bytes, params, corners| {
        let _open = gate.read().unwrap();
        Loaded::new(bytes, params, corners)
    })
}

/// Answers as `fetch` does, but only while `gate` isn't held for writing. Holds work up for as
/// long as a test needs, where a sleep would only usually be long enough.
pub fn gated_fetch(fetch: Fetch, gate: Arc<RwLock<()>>) -> Fetch {
    Arc::new(move |bytes, params, corners, model| {
        let _open = gate.read().unwrap();
        fetch(bytes, params, corners, model)
    })
}

/// Steps frames until no page is loading and the viewer has its image, which both happen off the
/// UI thread, then runs until the UI is idle. The first frame acts on any input. A transcription
/// or export in flight shows a spinner, which repaints until it's done, so that waits for it too.
pub fn settle(harness: &mut Harness<'_, NotebookApp>) {
    wait_for("pages to load", || {
        harness.step();
        !harness.state().loading()
    });
    wait_for("the UI to go idle", || harness.run_ok().is_some());
}

/// The eval log's events, less each one's page and model, as `(kind, detail)`.
pub fn events(harness: &Harness<'_, NotebookApp>) -> Vec<String> {
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
            Event::Reset { .. } => "reset".to_owned(),
        })
        .collect()
}

/// Thumbnail labels, left to right.
pub fn thumbnails(harness: &Harness<'_, NotebookApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Image)
        .filter_map(|node| node.accesskit_node().label())
        .filter(|label| !label.starts_with(&before_mark(t!("current-page", name = MARK))))
        .collect()
}

/// The centre of the node labelled `label`.
pub fn centre(harness: &Harness<'_, NotebookApp>, label: &str) -> egui::Pos2 {
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
pub fn drag_thumbnail(harness: &mut Harness<'_, NotebookApp>, label: &str, x: f32) {
    let end = hold_thumbnail(harness, label, x);
    harness.drop_at(end);
    settle(harness);
}

/// Drags the thumbnail labelled `label` to `x`, at the same height, without dropping it, and
/// returns where the pointer is.
pub fn hold_thumbnail(harness: &mut Harness<'_, NotebookApp>, label: &str, x: f32) -> egui::Pos2 {
    let start = centre(harness, label);
    let end = egui::pos2(x, start.y);
    hold(harness, start, end);
    end
}

/// Presses at `from` and moves to `to` in a few steps, without letting go. Doesn't wait for
/// loading, so works while loads are held up.
pub fn hold(harness: &mut Harness<'_, NotebookApp>, from: egui::Pos2, to: egui::Pos2) {
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
pub fn drag(harness: &mut Harness<'_, NotebookApp>, from: egui::Pos2, to: egui::Pos2) {
    hold(harness, from, to);
    harness.drop_at(to);
    harness.step();
}

/// Presses and lets go at `at`.
pub fn click_at(harness: &mut Harness<'_, NotebookApp>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    harness.drop_at(at);
    harness.step();
}

/// The left and right edges of what's painted as 3pt lines: the selected page's frame (a rect)
/// and the drop marker (a line segment), each cut to its clip rect.
pub fn painted_3pt(harness: &Harness<'_, NotebookApp>) -> (Vec<egui::Rangef>, Vec<egui::Rangef>) {
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

pub fn press_command(harness: &mut Harness<'_, NotebookApp>, key: egui::Key) {
    press(harness, egui::Modifiers::COMMAND, key);
}

pub fn press(harness: &mut Harness<'_, NotebookApp>, modifiers: egui::Modifiers, key: egui::Key) {
    harness.key_press_modifiers(modifiers, key);
    settle(harness);
}

pub const COMMAND_SHIFT: egui::Modifiers = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);

/// The labels of the thumbnails marked selected.
pub fn selected_thumbnails(harness: &Harness<'_, NotebookApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Image)
        .filter(|node| node.accesskit_node().toggled() == Some(Toggled::True))
        .filter_map(|node| node.accesskit_node().label())
        .collect()
}

/// `page_photo` in the Cleaned view, cleaned.
pub fn cleaned_view(harness: &mut Harness<'_, NotebookApp>) {
    drop_files(harness, &[("a.png", page_photo())]);
    click_command(harness, Command::ViewCleaned);
    settle(harness);
}

/// Where pixel `(x, y)` of `page_photo`, 200 x 150, is on screen in a.png's photo.
pub fn on_photo(harness: &Harness<'_, NotebookApp>, x: f32, y: f32) -> egui::Pos2 {
    let label = t!("current-page", name = "a.png");
    let bounds = harness.get_by_label(&label);
    let bounds = bounds.accesskit_node().bounding_box().unwrap();
    egui::pos2(
        (bounds.x0 + bounds.width() * f64::from(x) / 200.0) as f32,
        (bounds.y0 + bounds.height() * f64::from(y) / 150.0) as f32,
    )
}

/// Width over height of a.png's cleaned image.
pub fn cleaned_aspect(harness: &Harness<'_, NotebookApp>) -> f64 {
    let label = t!("current-page-cleaned", name = "a.png");
    let image = harness.get_by_label(&label);
    let bounds = image.accesskit_node().bounding_box().unwrap();
    bounds.width() / bounds.height()
}

pub fn assert_near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} isn't within {tolerance} of {expected}"
    );
}

pub fn disabled(harness: &Harness<'_, NotebookApp>, label: &str) -> bool {
    harness.get_by_label(label).accesskit_node().is_disabled()
}

/// Drags the bottom-right corner to `(x, y)` on the photo, and waits for the page to be cleaned
/// again.
pub fn move_bottom_right(harness: &mut Harness<'_, NotebookApp>, x: f32, y: f32) {
    let from = centre(harness, &t!("corner-bottom-right"));
    let to = on_photo(harness, x, y);
    drag(harness, from, to);
    settle(harness);
}

/// Where the corners' handles were painted last frame.
pub fn painted_handles(harness: &Harness<'_, NotebookApp>) -> Vec<egui::Pos2> {
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
pub fn handles(harness: &Harness<'_, NotebookApp>) -> Quad {
    let labels = [
        &t!("corner-top-left"),
        &t!("corner-top-right"),
        &t!("corner-bottom-right"),
        &t!("corner-bottom-left"),
    ];
    labels.map(|label| {
        let at = centre(harness, label);
        [at.x, at.y]
    })
}

/// Whether the transcript's status warns that some of its lines are outside the page's bounds.
pub fn warns_lines_outside(harness: &Harness<'_, NotebookApp>) -> bool {
    // The test page has fewer lines than this.
    (1..=20).any(|count| {
        let warning = t!("transcript-lines-cut-off", count = count);
        harness.query_by_label_contains(&warning).is_some()
    })
}

/// Whether the transcript's status warns that much of the page wasn't transcribed.
pub fn warns_missed_page(harness: &Harness<'_, NotebookApp>) -> bool {
    let warning = t!("transcript-missed-page");
    harness.query_by_label_contains(&warning).is_some()
}

/// Places a.png's corners at `corners` in the Cleaned view, then goes back to its transcript.
pub fn recrop(harness: &mut Harness<'_, NotebookApp>, corners: [(f32, f32); 4]) {
    click_command(harness, Command::ViewCleaned);
    settle(harness);
    click_command(harness, Command::PlaceCorners);
    settle(harness);
    for (x, y) in corners {
        let at = on_photo(harness, x, y);
        click_at(harness, at);
    }
    settle(harness);
    show_transcript(harness);
}

/// The page grown to most of the photo, table and all, so its status says it needs retranscribing.
pub fn grow_crop(harness: &mut Harness<'_, NotebookApp>) {
    recrop(
        harness,
        [(5.0, 5.0), (195.0, 5.0), (195.0, 145.0), (5.0, 145.0)],
    );
}

/// Where the box labelled `label` is on a.png's cleaned page, as fractions of its width and height:
/// `[x0, y0, x1, y1]`.
pub fn on_page(harness: &Harness<'_, NotebookApp>, label: &str) -> [f64; 4] {
    let page_label = t!("current-page-cleaned", name = "a.png");
    let page = harness.get_by_label(&page_label);
    let page = page.accesskit_node().bounding_box().unwrap();
    let node = harness.get_by_label(label);
    let b = node.accesskit_node().bounding_box().unwrap();
    [
        (b.x0 - page.x0) / page.width(),
        (b.y0 - page.y0) / page.height(),
        (b.x1 - page.x0) / page.width(),
        (b.y1 - page.y0) / page.height(),
    ]
}

/// Answers with the recorded responses in `answers`, in turn.
pub fn answering(answers: Vec<Result<Fetched, OcrError>>) -> (Fetch, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let answers = std::sync::Mutex::new(answers.into_iter());
    let fetch: Fetch = Arc::new(move |_, _, _, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        answers.lock().unwrap().next().expect("an answer left")
    });
    (fetch, calls)
}

/// Where the transcript's status went relative to its buttons.
#[derive(Debug, PartialEq)]
pub enum Status {
    Beside,
    Above,
}

/// Checks the status line containing `status` sits with its buttons: Reset then Force at the
/// bottom right of the page's side of the split, the text left of or above them, none of it
/// below them. Returns which.
pub fn status_placement(harness: &Harness<'_, NotebookApp>, status: &str) -> Status {
    let bounds = |node: egui_kittest::Node<'_>| node.accesskit_node().bounding_box().unwrap();
    let text = bounds(harness.get_by_label_contains(status));
    let force = bounds(harness.get_by_label(&Command::ForceRetranscribe.label()));
    let reset = bounds(harness.get_by_label(&Command::ResetTranscript.label()));
    let all = format!("text {text:?}, reset {reset:?}, force {force:?}");
    assert!(reset.x1 <= force.x0, "{all}");
    assert_eq!(reset.y1, force.y1, "{all}");
    assert!(text.y1 <= force.y1, "clipped: {all}");
    // Against the split, left of the transcript.
    let transcript = bounds(harness.get_by_label("# Thurs 23 Oct"));
    let gap = transcript.x0 - force.x1;
    assert!(
        (0.0..32.0).contains(&gap),
        "{all}, transcript {transcript:?}"
    );
    if text.x1 <= reset.x0 {
        // The last line level with the buttons.
        assert!(text.y1 > reset.y0, "{all}");
        Status::Beside
    } else {
        assert!(text.y1 <= reset.y0, "overlaps: {all}");
        Status::Above
    }
}

/// A harness `width` wide that transcribes every page to the recorded response.
pub fn harness_of_width(width: f32) -> Harness<'static, NotebookApp> {
    Harness::builder()
        .with_size(egui::vec2(width, 600.0))
        .build_ui_state(
            |ui, app: &mut NotebookApp| app.ui(ui),
            NotebookApp::with_transcription(Some(transcriber()), None),
        )
}

/// The colour the text `needle` is painted in.
pub fn text_colour(harness: &Harness<'_, NotebookApp>, needle: &str) -> egui::Color32 {
    text_format(harness, needle).color
}

/// How the text `needle` is painted.
pub fn text_format(harness: &Harness<'_, NotebookApp>, needle: &str) -> egui::TextFormat {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| {
            let egui::Shape::Text(text) = &clipped.shape else {
                return None;
            };
            let job = &text.galley.job;
            let at = job.text.find(needle)?;
            let at = egui::text::ByteIndex(at);
            let section = job.sections.iter().find(|s| s.byte_range.contains(&at))?;
            Some(section.format.clone())
        })
        .unwrap_or_else(|| panic!("{needle} isn't painted"))
}

pub fn overloaded() -> OcrError {
    OcrError::Api {
        status: 529,
        kind: "overloaded_error".into(),
        message: "Overloaded".into(),
    }
}

/// [`harness_with`] [`transcriber`], at 60 frames a second. Each click takes a few frames, so the
/// default quarter-second frames are too slow for a double-click.
pub fn quick_harness() -> Harness<'static, NotebookApp> {
    Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::with_transcription(Some(transcriber()), None),
    )
}

/// Double-clicks the line labelled `label` to edit it.
pub fn edit_line(harness: &mut Harness<'_, NotebookApp>, label: &str) {
    // Far enough after any earlier click that the double-click isn't a triple.
    harness.run_steps(30);
    let line = harness.get_by_label(label);
    line.click();
    line.click();
    harness.run_ok();
}

/// Whether the radio button or checkbox labelled `label` is on.
pub fn checked(harness: &Harness<'_, NotebookApp>, label: &str) -> bool {
    harness.get_by_label(label).accesskit_node().toggled() == Some(Toggled::True)
}

/// Whether line 1's box is outlined on a.png's cleaned page, as when the line is pointed at.
pub fn line_box_outlined(harness: &Harness<'_, NotebookApp>) -> bool {
    // Line 1's box, as Claude put it: 130..980 of 1200 across, 190..250 of 900 down.
    let page = harness
        .get_by_label(&t!("current-page-cleaned", name = "a.png"))
        .rect();
    let expected = egui::Rect::from_min_max(
        page.min + egui::vec2(130.0 / 1200.0, 190.0 / 900.0) * page.size(),
        page.min + egui::vec2(980.0 / 1200.0, 250.0 / 900.0) * page.size(),
    );
    // Painted as a rect, or a quad if the page was cut elsewhere, with a 1pt outline outside.
    harness.output().shapes.iter().any(|clipped| {
        let painted = clipped.shape.visual_bounding_rect();
        (painted.min - expected.min).length() < 3.0 && (painted.max - expected.max).length() < 3.0
    })
}

/// The label of what has the keyboard focus, if anything does.
pub fn focused(harness: &Harness<'_, NotebookApp>) -> Option<String> {
    let node = harness.query_by(|node| node.is_focused())?;
    let node = node.accesskit_node();
    // A label's text is its value.
    Some(node.label().or_else(|| node.value()).unwrap_or_default())
}

/// Presses Tab until what's labelled `label` has the keyboard focus.
pub fn tab_to(harness: &mut Harness<'_, NotebookApp>, label: &str) {
    let mut reached = Vec::new();
    for _ in 0..100 {
        let now = focused(harness);
        if now.as_deref() == Some(label) {
            return;
        }
        reached.push(now);
        press(harness, egui::Modifiers::NONE, egui::Key::Tab);
    }
    panic!("Tab never reached {label}, only {reached:?}");
}
