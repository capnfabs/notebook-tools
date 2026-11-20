//! The Cleaned view, and placing and dragging page corners.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::{Command, Fetch, NotebookApp, Transcriber, t};
use notebook_core::claude_transcribe::Fetched;
use notebook_core::geom::is_page_shaped;
use std::sync::{Arc, RwLock};

use crate::common::*;

/// The page's corners are drawn relative to the image's rect, so that rect must be where the image
/// is drawn, not the whole panel it's centred in.
#[test]
fn image_rect_matches_the_drawn_image() {
    let mut harness = harness();
    drop_files(&mut harness, &[("a.png", page_photo())]);

    let label = t!("current-page", name = "a.png");
    let node = harness.get_by_label(&label);
    let bounds = node.accesskit_node().bounding_box().unwrap();
    let aspect = bounds.width() / bounds.height();
    assert!((aspect - 200.0 / 150.0).abs() < 0.02, "{bounds:?}");
}

#[test]
fn cleaned_view_shows_the_photo_with_its_corners_beside_the_cleaned_page() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    harness.get_by_label(&t!("current-page", name = "a.png"));
    // The page is 120 x 110.
    assert_near(cleaned_aspect(&harness), 120.0 / 110.0, 0.1);
    let corner = centre(&harness, &t!("corner-top-left"));
    assert!(
        corner.distance(on_photo(&harness, 40.0, 20.0)) < 8.0,
        "{corner:?}"
    );
    for label in [
        &t!("corner-top-right"),
        &t!("corner-bottom-right"),
        &t!("corner-bottom-left"),
    ] {
        harness.get_by_label(label);
    }
    assert!(disabled(&harness, &Command::ResetCorners.label()));

    click_command(&harness, Command::ViewOriginal);
    settle(&mut harness);
    assert!(harness.query_by_label(&t!("corner-top-left")).is_none());
}

#[test]
fn dragging_a_corner_cleans_the_page_again() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    let before = cleaned_aspect(&harness);

    move_bottom_right(&mut harness, 160.0, 75.0);
    let corner = centre(&harness, &t!("corner-bottom-right"));
    assert!(
        corner.distance(on_photo(&harness, 160.0, 75.0)) < 2.0,
        "{corner:?}"
    );
    // Straightened to the average of opposite sides: (120 + 132) / 2 by (110 + 55) / 2.
    assert_near(cleaned_aspect(&harness), 126.0 / 82.5, 0.1);

    click_command(&harness, Command::ResetCorners);
    settle(&mut harness);
    assert_near(cleaned_aspect(&harness), before, 0.01);
    assert!(disabled(&harness, &Command::ResetCorners.label()));
}

#[test]
fn corners_show_they_can_be_dragged() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    let cursor = |harness: &Harness<'_, NotebookApp>| harness.output().platform_output.cursor_icon;

    let corner = centre(&harness, &t!("corner-top-left"));
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

    click_command(&harness, Command::PlaceCorners);
    settle(&mut harness);
    harness.get_by_label(&t!("corners-placing-prompt"));
    assert!(harness.query_by_label(&t!("corner-top-left")).is_none());
    for (x, y) in [(160.0, 75.0), (40.0, 20.0), (40.0, 75.0), (160.0, 20.0)] {
        let at = on_photo(&harness, x, y);
        click_at(&mut harness, at);
    }
    settle(&mut harness);

    // The top half of the page, 120 x 55.
    assert_near(cleaned_aspect(&harness), 120.0 / 55.0, 0.1);
    let corner = centre(&harness, &t!("corner-top-left"));
    assert!(
        corner.distance(on_photo(&harness, 40.0, 20.0)) < 2.0,
        "{corner:?}"
    );
    assert!(!disabled(&harness, &Command::ResetCorners.label()));
}

#[test]
fn escape_stops_placing_corners() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    let before = cleaned_aspect(&harness);

    click_command(&harness, Command::PlaceCorners);
    settle(&mut harness);
    for (x, y) in [(40.0, 20.0), (160.0, 20.0)] {
        let at = on_photo(&harness, x, y);
        click_at(&mut harness, at);
    }
    press(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);

    harness.get_by_label(&t!("corner-top-left"));
    assert!(
        harness
            .query_by_label(&t!("corners-placing-prompt"))
            .is_none()
    );
    assert_near(cleaned_aspect(&harness), before, 0.01);
    assert!(disabled(&harness, &Command::ResetCorners.label()));
}

#[test]
fn a_corner_stays_where_its_let_go() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    let from = centre(&harness, &t!("corner-bottom-right"));
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
        let from = centre(&harness, &t!("corner-top-left"));
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
        let moved = centre(&harness, &t!("corner-top-left")).distance(from);
        assert!(moved > 20.0, "moved only {moved}");
        harness.get_by_label(&t!("current-page-cleaned", name = "a.png"));
    }
}

#[test]
fn a_fourth_corner_that_spoils_the_outline_is_refused() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    click_command(&harness, Command::PlaceCorners);
    settle(&mut harness);
    // The fourth is inside the triangle of the other three.
    for (x, y) in [(40.0, 20.0), (160.0, 20.0), (40.0, 130.0), (70.0, 50.0)] {
        let at = on_photo(&harness, x, y);
        click_at(&mut harness, at);
    }
    settle(&mut harness);
    harness.get_by_label(&t!("corners-refused-prompt"));
    assert!(harness.query_by_label(&t!("corner-top-left")).is_none());

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
        let from = centre(&harness, &t!("corner-bottom-right"));
        let to = on_photo(&harness, 160.0, y);
        drag(&mut harness, from, to);
    }
    // The old cleaned page stays up while the new one is made.
    harness.get_by_label(&t!("current-page-cleaned-loading", name = "a.png"));

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
        Arc::new(move |bytes, _, corners, _| {
            sent.lock().unwrap().push(corners);
            Ok(Fetched {
                crop: sent_crop(bytes, corners),
                ..recorded()
            })
        })
    };
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    click_command(&harness, Command::ViewCleaned);
    settle(&mut harness);
    move_bottom_right(&mut harness, 160.0, 75.0);
    show_transcript(&mut harness);
    click_command(&harness, Command::ForceRetranscribe);
    step_until_transcribed(&mut harness);
    step_until(&mut harness, "# Thurs 23 Oct");
    assert!(!warns_lines_outside(&harness));
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], None);
    let [_, _, [x, y], _] = sent[1].expect("the user's corners");
    let s = PAGE_PHOTO_SCALE as f32;
    assert!(
        (x - 160.0 * s).abs() < 2.0 * s && (y - 75.0 * s).abs() < 2.0 * s,
        "{:?}",
        sent[1]
    );
}

#[test]
fn boxes_follow_the_page_when_its_corners_move() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    // As Claude put it: 330..520 of 1200 across, 190..250 of 900 down.
    let before = on_page(&harness, &t!("uncertain-box", word = "plumber"));
    for (b, e) in before.iter().zip([0.275, 0.211, 0.433, 0.278]) {
        assert_near(*b, e, 0.02);
    }

    // The top half of the page, which holds every line.
    recrop(
        &mut harness,
        [(40.0, 20.0), (160.0, 20.0), (160.0, 75.0), (40.0, 75.0)],
    );
    let after = on_page(&harness, &t!("uncertain-box", word = "plumber"));
    for (a, e) in after.iter().zip([0.275, 0.422, 0.433, 0.556]) {
        assert_near(*a, e, 0.03);
    }
    assert!(!warns_lines_outside(&harness));
    assert!(!warns_missed_page(&harness));
}

#[test]
fn cropping_lines_off_the_page_warns() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    // The top fifth of the page, which holds the first line only.
    recrop(
        &mut harness,
        [(40.0, 20.0), (160.0, 20.0), (160.0, 45.0), (40.0, 45.0)],
    );
    harness.get_by_label_contains(&t!("transcript-lines-cut-off", count = 3));
    assert!(!warns_missed_page(&harness));
}

#[test]
fn growing_the_crop_asks_for_retranscription() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    grow_crop(&mut harness);
    assert!(warns_missed_page(&harness));
    assert!(!warns_lines_outside(&harness));
}

#[test]
fn a_focused_corner_is_ringed_and_moves_with_the_arrows() {
    let mut harness = harness();
    drop_files(
        &mut harness,
        &[("a.png", page_photo()), ("b.png", another_page_photo())],
    );
    click_command(&harness, Command::ViewCleaned);
    settle(&mut harness);
    let label = t!("corner-top-left");
    // The ring around the focused handle.
    let rings = |harness: &Harness<'_, NotebookApp>| -> Vec<egui::Pos2> {
        let circles = harness
            .output()
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Circle(circle) if circle.radius == 8.0 => Some(circle.center),
                _ => None,
            });
        circles.collect()
    };
    assert!(rings(&harness).is_empty());

    tab_to(&mut harness, &label);
    let before = centre(&harness, &label);
    let ring = rings(&harness);
    assert!(
        ring.len() == 1 && ring[0].distance(before) < 1.0,
        "{ring:?}"
    );

    // The arrows nudge it, not change page; Shift nudges further.
    for _ in 0..3 {
        press(&mut harness, egui::Modifiers::NONE, egui::Key::ArrowRight);
    }
    press(&mut harness, egui::Modifiers::SHIFT, egui::Key::ArrowDown);
    let moved = centre(&harness, &label) - before;
    assert!((moved - egui::vec2(3.0, 10.0)).length() < 1.0, "{moved:?}");
    assert_eq!(selected_thumbnails(&harness), ["a.png"]);
    assert!(!disabled(&harness, &Command::ResetCorners.label()));
    assert_eq!(focused(&harness).as_deref(), Some(label.as_str()));
}

#[test]
fn a_held_arrow_moves_the_corner_once_let_go() {
    let mut harness = harness();
    cleaned_view(&mut harness);
    let label = t!("corner-bottom-right");
    tab_to(&mut harness, &label);
    let before = centre(&harness, &label);

    // Key repeats while held move the handle, but the page is only cleaned again once let go.
    harness.key_down(egui::Key::ArrowLeft);
    harness.run_ok();
    harness.key_down(egui::Key::ArrowLeft);
    harness.run_ok();
    assert!(centre(&harness, &label).x < before.x - 1.5);
    assert!(disabled(&harness, &Command::ResetCorners.label()));
    harness.key_up(egui::Key::ArrowLeft);
    settle(&mut harness);
    assert!(!disabled(&harness, &Command::ResetCorners.label()));
    assert_near(
        f64::from(centre(&harness, &label).x),
        f64::from(before.x - 2.0),
        1.0,
    );
}
