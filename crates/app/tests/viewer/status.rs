//! What the Transcribed view says about where a transcript came from.

use notebook_app::t;

use crate::common::*;

#[test]
fn a_short_status_sits_beside_its_buttons() {
    // 800pt wide, so the page gets half and the buttons take most of that: one line wraps to two.
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    let status = before_mark(t!("transcript-from-claude", when = MARK));
    assert_eq!(status_placement(&harness, &status), Status::Beside);
}

#[test]
fn a_long_status_goes_above_its_buttons() {
    // Room for the warning beside the buttons, but not on one line: three lines in all.
    let mut harness = harness_of_width(1400.0);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    grow_crop(&mut harness);
    let status = t!("transcript-missed-page");
    assert_eq!(status_placement(&harness, &status), Status::Above);
}

#[test]
fn only_warnings_take_the_warning_colour() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    grow_crop(&mut harness);
    let warn = harness.ctx.global_style().visuals.warn_fg_color;
    assert_eq!(text_colour(&harness, &t!("transcript-missed-page")), warn);
    let from = before_mark(t!("transcript-from-claude", when = MARK));
    assert_ne!(text_colour(&harness, &from), warn);
}
