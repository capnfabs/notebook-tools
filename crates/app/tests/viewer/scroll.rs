//! Where each page's transcript is scrolled to: its own, kept while the app runs. See
//! `decisions/2026-11-18-transcript-scroll-per-page.md`.

use eframe::egui;
use egui::Key;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::NotebookApp;

use crate::common::*;

/// Too short for the whole transcript.
const HEIGHT: f32 = 110.0;

/// Two pages, both transcribed, the first shown in the Transcribed view, too short for its
/// transcript.
fn two_transcribed_pages() -> Harness<'static, NotebookApp> {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(800.0, HEIGHT))
        .build_ui_state(
            |ui, app: &mut NotebookApp| app.ui(ui),
            NotebookApp::with_transcription(Some(transcriber()), None),
        );
    drop_files(
        &mut harness,
        &[("a.png", page_photo()), ("b.png", another_page_photo())],
    );
    show_transcript(&mut harness);
    press_command(&mut harness, Key::ArrowRight);
    press_command(&mut harness, Key::T);
    step_until(&mut harness, "# Thurs 23 Oct");
    press_command(&mut harness, Key::ArrowLeft);
    assert_eq!(selected_thumbnails(&harness), ["a.png"]);
    harness
}

fn heading_top(harness: &Harness<'_, NotebookApp>) -> f32 {
    harness.get_by_label("# Thurs 23 Oct").rect().top()
}

#[test]
fn another_page_opens_at_the_top() {
    let mut harness = two_transcribed_pages();
    let top = heading_top(&harness);
    scroll_transcript(&mut harness, 40.0);
    assert!(heading_top(&harness) < top - 1.0, "scrolled");

    press_command(&mut harness, Key::ArrowRight);
    assert_eq!(selected_thumbnails(&harness), ["b.png"]);
    let b = heading_top(&harness);
    assert!(b >= top - 1.0, "b at the top: {b} vs {top}");
}

#[test]
fn a_page_comes_back_where_it_was() {
    let mut harness = two_transcribed_pages();
    let top = heading_top(&harness);
    scroll_transcript(&mut harness, 40.0);
    let scrolled = heading_top(&harness);
    assert!(scrolled < top - 1.0, "scrolled");

    // Scrolling another page leaves this one's place alone.
    press_command(&mut harness, Key::ArrowRight);
    scroll_transcript(&mut harness, -40.0);
    assert!(heading_top(&harness) >= top - 1.0, "b at the top");
    press_command(&mut harness, Key::ArrowLeft);
    assert_eq!(selected_thumbnails(&harness), ["a.png"]);
    let back = heading_top(&harness);
    assert!((back - scrolled).abs() <= 1.0, "{back} vs {scrolled}");
}
