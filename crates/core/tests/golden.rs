//! Cleanup on real photos (shrunk to 1000px, metadata stripped), against hand-labelled page
//! corners and golden output images.
//!
//! Cleanup is expected to change while it's being tuned. After a deliberate change, look at the
//! images in `target/tmp/golden-actual/` and regenerate with `UPDATE_GOLDEN=1 cargo test`.

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_line_segment_mut};
use notebook_core::{CleanParams, Quad, clean, decode, detect_outline, shrink_to_fit};

/// Long side the goldens are stored and compared at; enough to see a cleanup regression.
const GOLDEN_SIDE_PX: u32 = 512;
/// Mean absolute difference per channel allowed between output and golden.
const GOLDEN_TOLERANCE: f64 = 2.0;
/// How far a detected corner may be from the hand-labelled one, as a fraction of the long side.
const CORNER_TOLERANCE: f32 = 0.02;

struct Fixture {
    name: &'static str,
    /// Hand-labelled page outline (TL, TR, BR, BL) in fixture pixels.
    outline: Quad,
    /// Outline detection doesn't handle this one yet (cleanup falls back to lighting only).
    /// Checked by the ignored `hard_outlines_match_hand_labels`; see `tasks/outline-detection.md`.
    hard: bool,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "spread-wood-1",
        outline: [[97.0, 38.0], [875.0, 61.0], [899.0, 705.0], [68.0, 704.0]],
        hard: false,
    },
    // Shadowed bottom of the page is as bright as the wood beside it.
    Fixture {
        name: "spread-wood-2",
        outline: [[141.0, 43.0], [914.0, 66.0], [920.0, 702.0], [125.0, 699.0]],
        hard: true,
    },
    // Light grey table, about as bright as the paper; the next page is partly in shot. The
    // outline is the left page.
    Fixture {
        name: "page-grey-table",
        outline: [[88.0, 47.0], [607.0, 54.0], [607.0, 940.0], [33.0, 940.0]],
        hard: true,
    },
];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.png"))
}

fn actual_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden-actual");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn load(name: &str) -> RgbaImage {
    let bytes = std::fs::read(fixtures_dir().join(format!("{name}.jpg"))).unwrap();
    decode(&bytes).unwrap()
}

/// The photo with the detected outline in red and the hand-labelled one in green.
fn draw_outlines(img: &RgbaImage, detected: Option<&Quad>, labelled: &Quad) -> RgbaImage {
    let mut out = img.clone();
    let mut draw = |quad: &Quad, colour: Rgba<u8>| {
        for i in 0..4 {
            let (a, b) = (quad[i], quad[(i + 1) % 4]);
            draw_line_segment_mut(&mut out, (a[0], a[1]), (b[0], b[1]), colour);
            draw_filled_circle_mut(&mut out, (a[0] as i32, a[1] as i32), 4, colour);
        }
    };
    draw(labelled, Rgba([0, 200, 0, 255]));
    if let Some(quad) = detected {
        draw(quad, Rgba([255, 0, 0, 255]));
    }
    out
}

#[test]
fn fixtures_have_no_metadata() {
    for entry in std::fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        // Photos live at the top level; subdirectories hold other fixtures, like API responses.
        if path.is_dir() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        // EXIF (which holds GPS location) lives in an APP1 segment starting "Exif\0\0".
        assert!(
            !bytes.windows(6).any(|w| w == b"Exif\0\0"),
            "{} has EXIF metadata; strip it with `exiftool -all= -overwrite_original`",
            path.display()
        );
    }
}

#[test]
fn outlines_match_hand_labels() {
    check_outlines(FIXTURES.iter().filter(|f| !f.hard));
}

/// Photos outline detection can't handle yet. Run with `cargo test -- --ignored` when working
/// on detection.
#[test]
#[ignore]
fn hard_outlines_match_hand_labels() {
    check_outlines(FIXTURES.iter().filter(|f| f.hard));
}

fn check_outlines<'a>(fixtures: impl Iterator<Item = &'a Fixture>) {
    let params = CleanParams::default();
    let mut failures = Vec::new();
    for fixture in fixtures {
        let img = load(fixture.name);
        let detected = detect_outline(&img, &params);
        let annotated = draw_outlines(&img, detected.as_ref().ok(), &fixture.outline);
        let path = actual_dir().join(format!("{}-outline.png", fixture.name));
        annotated.save(&path).unwrap();

        let tolerance = CORNER_TOLERANCE * img.width().max(img.height()) as f32;
        let ok = detected.as_ref().is_ok_and(|quad| {
            quad.iter()
                .zip(&fixture.outline)
                .all(|(d, l)| (d[0] - l[0]).hypot(d[1] - l[1]) <= tolerance)
        });
        if !ok {
            failures.push(format!(
                "{}: detected {detected:?}, labelled {:?} (see {})",
                fixture.name,
                fixture.outline,
                path.display()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn cleaned_output_matches_golden() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let params = CleanParams::default();
    let mut failures = Vec::new();
    for fixture in FIXTURES {
        let cleaned = clean(&load(fixture.name), &params);
        let actual = shrink_to_fit(cleaned.image, GOLDEN_SIDE_PX);
        let golden = golden_path(fixture.name);
        if update {
            std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
            actual.save(&golden).unwrap();
            continue;
        }
        let actual_path = actual_dir().join(format!("{}-cleaned.png", fixture.name));
        actual.save(&actual_path).unwrap();

        let Ok(expected) = image::open(&golden) else {
            failures.push(format!(
                "{}: no golden at {}",
                fixture.name,
                golden.display()
            ));
            continue;
        };
        let expected = expected.into_rgba8();
        if expected.dimensions() != actual.dimensions() {
            failures.push(format!(
                "{}: size {:?}, golden {:?} (see {})",
                fixture.name,
                actual.dimensions(),
                expected.dimensions(),
                actual_path.display()
            ));
            continue;
        }
        let diff: u64 = actual
            .as_raw()
            .iter()
            .zip(expected.as_raw())
            .map(|(&a, &b)| u64::from(a.abs_diff(b)))
            .sum();
        let mean = diff as f64 / actual.as_raw().len() as f64;
        if mean > GOLDEN_TOLERANCE {
            failures.push(format!(
                "{}: mean difference {mean:.2} > {GOLDEN_TOLERANCE} (see {})",
                fixture.name,
                actual_path.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}\nIf the change is intended, rerun with UPDATE_GOLDEN=1",
        failures.join("\n")
    );
}
