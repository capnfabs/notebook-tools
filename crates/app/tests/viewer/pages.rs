//! Adding, loading, selecting, moving and deleting pages.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::{Command, NotebookApp, t};
use std::sync::{Arc, RwLock};

use crate::common::*;

#[test]
fn starts_with_drop_zone() {
    let harness = harness();
    harness.get_by_label(&t!("drop-zone-title"));
}

#[test]
fn shows_dropped_images_in_filename_order() {
    let mut harness = harness();
    drop_files(&mut harness, &[("b.png", png()), ("a.png", png())]);

    assert!(harness.query_by_label(&t!("drop-zone-title")).is_none());
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);
    harness.get_by_label(&t!("current-page", name = "a.png"));
}

#[test]
fn clicking_a_thumbnail_selects_it() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png()), ("b.png", png())]);

    harness.get_by_label("b.png").click();
    settle(&mut harness);

    harness.get_by_label(&t!("current-page", name = "b.png"));
}

#[test]
fn later_drops_merge_in_order_and_keep_selection() {
    let mut harness = harness();
    let b = png();
    drop_files(&mut harness, &[("b.png", b.clone()), ("d.png", png())]);
    harness.get_by_label("d.png").click();
    settle(&mut harness);

    // b.png again is the same photo, so it's left out.
    drop_files(
        &mut harness,
        &[("c.png", png()), ("b.png", b), ("a.png", png())],
    );

    assert_eq!(thumbnails(&harness), ["a.png", "b.png", "c.png", "d.png"]);
    harness.get_by_label(&t!("current-page", name = "d.png"));
}

#[test]
fn reports_files_that_are_not_images() {
    let mut harness = harness();
    drop_files(&mut harness, &[("notes.txt", b"hello".to_vec())]);

    harness.get_by_label(&t!("drop-zone-title"));
    harness.get_by_label(&t!(
        "open-failed",
        name = "notes.txt",
        error = t!("decode-unrecognized")
    ));
}

#[test]
fn photos_too_small_to_read_are_refused() {
    let mut harness = harness();
    let small = image::RgbaImage::new(150, 100);
    let mut bytes = Vec::new();
    small
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    drop_files(&mut harness, &[("small.png", bytes), ("a.png", png())]);

    assert_eq!(thumbnails(&harness), ["a.png"]);
    harness.get_by_label(&t!(
        "open-failed",
        name = "small.png",
        error = t!("decode-too-small", size = "150×100", min = 200)
    ));
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
    harness.get_by_label(&t!(
        "open-failed",
        name = "b.txt",
        error = t!("decode-unrecognized")
    ));
    harness.get_by_label(&t!(
        "open-failed",
        name = "d.txt",
        error = t!("decode-unrecognized")
    ));
    harness.get_by_label(&t!("current-page", name = "c.png"));
}

#[test]
fn pages_hold_their_place_while_loading() {
    let gate = Arc::new(RwLock::new(()));
    let app = NotebookApp::default().with_loader(gated_loader(gate.clone()));
    let mut harness = harness_for(app);
    drop_files(&mut harness, &[("a.png", png()), ("b.png", png())]);
    harness.get_by_label("b.png").click();
    settle(&mut harness);
    press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowLeft);

    let held = gate.write().unwrap();
    start_dropping(&mut harness, &[("d.png", png()), ("c.png", png())]);
    harness.step();
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "c.png", "d.png"]);
    harness.get_by_label(&t!("status-loading", count = 2));
    // A page that's loading can be selected, but there's nothing to show yet.
    harness.get_by_label("c.png").click();
    harness.step();
    harness.step();
    assert_eq!(selected_thumbnails(&harness), ["c.png"]);
    assert!(
        harness
            .query_by_label_contains(&t!("current-page", name = "c.png"))
            .is_none()
    );

    drop(held);
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "c.png", "d.png"]);
    let loading = before_mark(t!("status-loading", count = MARK));
    assert!(harness.query_by_label_contains(&loading).is_none());
    harness.get_by_label(&t!("current-page", name = "c.png"));
}

#[test]
fn the_viewer_shows_the_thumbnail_until_the_page_is_decoded() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png()), ("b.png", page_photo())]);

    harness.get_by_label("b.png").click();
    // Decoded off the UI thread, so not in the frame the click lands in.
    harness.step();
    harness.get_by_label(&t!("current-page-loading", name = "b.png"));
    settle(&mut harness);
    harness.get_by_label(&t!("current-page", name = "b.png"));

    click_command(&harness, Command::ViewCleaned);
    harness.step();
    harness.get_by_label(&t!("current-page-cleaned-loading", name = "b.png"));
    settle(&mut harness);
    harness.get_by_label(&t!("current-page-cleaned", name = "b.png"));
}

#[test]
fn cleaned_view_shows_the_cleaned_page() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", page_photo())]);

    click_command(&harness, Command::ViewCleaned);
    settle(&mut harness);
    harness.get_by_label(&t!("current-page-cleaned", name = "a.png"));

    click_command(&harness, Command::ViewOriginal);
    settle(&mut harness);
    harness.get_by_label(&t!("current-page", name = "a.png"));
}

#[test]
fn view_stays_when_selecting_another_page() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    click_command(&harness, Command::ViewCleaned);
    settle(&mut harness);

    harness.get_by_label("b.png").click();
    settle(&mut harness);

    harness.get_by_label(&t!("current-page-cleaned", name = "b.png"));
}

/// Without a transcriber (no credentials) or a cache, there's nothing to transcribe or export.
#[test]
fn controls_without_a_transcriber_or_cache() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", png())]);

    for label in [
        &Command::ViewOriginal.label(),
        &Command::ViewCleaned.label(),
        &Command::Delete.label(),
    ] {
        let node = harness.get_by_label(label);
        assert!(!node.accesskit_node().is_disabled(), "{label} is disabled");
    }
    for label in [
        &Command::ViewTranscribed.label(),
        &Command::TranscribeAll.label(),
        &t!("command-reexport"),
        &Command::ExportAs.label(),
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

    click_command(&harness, Command::Delete);
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["a.png", "c.png"]);
    harness.get_by_label(&t!("current-page", name = "c.png"));

    // The last page selects the one before it.
    click_command(&harness, Command::Delete);
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["a.png"]);
    harness.get_by_label(&t!("current-page", name = "a.png"));

    click_command(&harness, Command::Delete);
    settle(&mut harness);
    harness.get_by_label(&t!("drop-zone-title"));
}

#[test]
fn deleting_a_transcribed_page_asks_first() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    click_command(&harness, Command::Delete);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-delete", name = "a.png"));
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);

    click_command(&harness, Command::Delete);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-delete-button")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("not-transcribed-yet", name = "b.png"));
    assert_eq!(thumbnails(&harness), ["b.png"]);
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
    harness.get_by_label(&t!("current-page", name = "a.png"));
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
        click_command(&harness, Command::SortByName);
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
    press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowRight);
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
    let sort =
        |harness: &Harness<'_, NotebookApp>| !disabled(harness, &Command::SortByName.label());
    assert!(!sort(&harness), "already sorted");

    press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowRight);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png"]);
    assert!(sort(&harness));

    click_command(&harness, Command::SortByName);
    settle(&mut harness);
    assert_eq!(thumbnails(&harness), ["a.png", "b.png"]);
    assert!(!sort(&harness));
    harness.get_by_label(&t!("current-page", name = "a.png"));
}

#[test]
fn command_shift_arrows_move_the_selected_page() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );
    harness.get_by_label("b.png").click();
    settle(&mut harness);

    press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowRight);
    assert_eq!(thumbnails(&harness), ["a.png", "c.png", "b.png"]);
    // Already last.
    press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowRight);
    assert_eq!(thumbnails(&harness), ["a.png", "c.png", "b.png"]);

    press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowLeft);
    press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowLeft);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "c.png"]);
    press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowLeft);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png", "c.png"]);
    harness.get_by_label(&t!("current-page", name = "b.png"));
}

#[test]
fn active_page_is_marked_and_named() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );
    assert_eq!(selected_thumbnails(&harness), ["a.png"]);
    harness.get_by_label(&t!("page-position", name = "a.png", number = 1, count = 3));

    harness.get_by_label("c.png").click();
    settle(&mut harness);
    assert_eq!(selected_thumbnails(&harness), ["c.png"]);
    harness.get_by_label(&t!("page-position", name = "c.png", number = 3, count = 3));
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
        press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowRight);
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
        &[("a.png", page_photo()), ("b.png", another_page_photo())],
    );
    show_transcript(&mut harness);
    harness.get_by_label("b.png").click();
    settle(&mut harness);
    click_command(&harness, Command::Transcribe);
    step_until(&mut harness, &t!("current-page", name = "b.png"));
    step_until(&mut harness, "# Thurs 23 Oct");

    press(&mut harness, COMMAND_SHIFT, egui::Key::ArrowLeft);
    assert_eq!(thumbnails(&harness), ["b.png", "a.png"]);
    let copied = export_to_clipboard(&mut harness);
    let b = copied.find("<!-- b.png -->").unwrap();
    let a = copied.find("<!-- a.png -->").unwrap();
    assert!(b < a, "{copied}");
}

#[test]
fn command_arrows_select_the_previous_and_next_page() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );
    let command = egui::Modifiers::COMMAND;

    // Already first.
    press(&mut harness, command, egui::Key::ArrowLeft);
    harness.get_by_label(&t!("current-page", name = "a.png"));
    for _ in 0..3 {
        press(&mut harness, command, egui::Key::ArrowRight);
    }
    harness.get_by_label(&t!("current-page", name = "c.png"));
    press(&mut harness, command, egui::Key::ArrowLeft);
    harness.get_by_label(&t!("current-page", name = "b.png"));
    assert_eq!(thumbnails(&harness), ["a.png", "b.png", "c.png"]);
}
