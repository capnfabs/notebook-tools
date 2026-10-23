//! The Claude OCR request and recorded responses. Never calls the API, except the ignored live
//! test:
//!
//! ```sh
//! scripts/with-claude-token.sh cargo test -p notebook_core --test ocr -- --ignored live
//! ```
//!
//! It saves the raw response to `target/tmp/ocr-live/`; copy one into `tests/fixtures/ocr/` to
//! record it.

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use notebook_core::ocr::{
    Credentials, EFFORT, MODEL, OcrError, Transcript, encode_jpeg, parse_response, request_body,
};
use serde::Deserialize;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(format!("ocr/{name}.json"))).unwrap()
}

fn parse(name: &str) -> Result<Transcript, OcrError> {
    parse_response(200, &fixture(name), (1200, 900))
}

/// Panics if any of `transcript`'s boxes isn't ordered or inside the image sent. Boxes tend to sit
/// a little low, so one on the bottom line can overrun slightly.
fn assert_boxes_in_bounds(transcript: &Transcript) {
    let (w, h) = (
        transcript.image_size.0 as f32,
        transcript.image_size.1 as f32,
    );
    let slack = w.max(h) * 0.01;
    let boxes = transcript.lines.iter().map(|l| l.bbox);
    for [x0, y0, x1, y1] in boxes
        .chain(transcript.uncertain.iter().map(|u| u.bbox))
        .flatten()
    {
        assert!(
            -slack <= x0 && x0 <= x1 && x1 <= w + slack,
            "{x0} {x1} in {w}"
        );
        assert!(
            -slack <= y0 && y0 <= y1 && y1 <= h + slack,
            "{y0} {y1} in {h}"
        );
    }
}

#[test]
fn request_body_snapshot() {
    let img = RgbaImage::from_pixel(2, 2, Rgba([255, 255, 255, 255]));
    insta::assert_json_snapshot!(request_body(&encode_jpeg(&img), EFFORT));
}

#[test]
fn request_body_sets_effort() {
    let body = request_body(&[], "low");
    assert_eq!(body["output_config"]["effort"], "low");
}

#[test]
fn parses_success() {
    let transcript = parse("success").unwrap();
    assert_eq!(transcript.model, MODEL);
    assert_eq!(transcript.image_size, (1200, 900));
    insta::assert_debug_snapshot!("success_transcript", transcript);
    insta::assert_snapshot!("success_markdown", transcript.markdown());
}

/// A real response for `spread-wood-1.jpg`, recorded by the live test.
#[test]
fn parses_recorded_response() {
    let transcript = parse("spread-wood-1").unwrap();
    assert_eq!(transcript.model, MODEL);
    assert!(transcript.usage.output_tokens > 0);
    // Two of these claim a line one off from where their text is.
    let spans = transcript.uncertain_spans();
    assert_eq!(spans.len(), 5);
    let markdown = transcript.markdown();
    for (u, span) in transcript.uncertain.iter().zip(spans) {
        assert_eq!(markdown.get(span.unwrap()), Some(u.text.as_str()));
    }
    insta::assert_snapshot!("spread_wood_1_markdown", markdown);
}

/// One request made by the bbox spike, as in its `runs.json`.
#[derive(Deserialize)]
struct SpikeRun {
    photo: String,
    variant: String,
    sent_size: (u32, u32),
    status: Option<u16>,
    error: Option<String>,
}

/// Real responses for `spread-wood-3` to `-6` at every effort and three image sizes, recorded by
/// `examples/bbox_spike.rs`, which also draws their boxes.
#[test]
fn parses_bbox_spike_responses() {
    let dir = fixtures_dir().join("ocr/bbox-spike");
    let runs: Vec<SpikeRun> =
        serde_json::from_slice(&std::fs::read(dir.join("runs.json")).unwrap()).unwrap();
    assert!(!runs.is_empty());
    for run in runs {
        let photo = run.photo.trim_end_matches(".jpg");
        let name = format!("{photo}-{}", run.variant);
        // Never answered, so nothing was recorded.
        let Some(status) = run.status else { continue };
        let body = std::fs::read(dir.join(format!("{name}.json"))).unwrap();
        match (parse_response(status, &body, run.sent_size), run.error) {
            (Ok(transcript), None) => {
                assert!(transcript.usage.output_tokens > 0, "{name}");
                assert_boxes_in_bounds(&transcript);
                // Some aren't found: their line index can be far off. See
                // `decisions/2026-10-23-bbox-accuracy.md`.
                let markdown = transcript.markdown();
                for (u, span) in transcript
                    .uncertain
                    .iter()
                    .zip(transcript.uncertain_spans())
                {
                    if let Some(span) = span {
                        assert_eq!(&markdown[span], u.text, "{name}");
                    }
                }
            }
            (Err(err), Some(recorded)) => assert_eq!(err.to_string(), recorded, "{name}"),
            (parsed, recorded) => panic!("{name}: parsed {parsed:?}, recorded {recorded:?}"),
        }
    }
}

#[test]
fn parses_answer_after_fallback() {
    let transcript = parse("fallback").unwrap();
    assert_eq!(transcript.model, "claude-opus-4-8");
    assert_eq!(transcript.lines, parse("success").unwrap().lines);
}

#[test]
fn refusal_is_an_error() {
    assert_eq!(
        parse("refusal"),
        Err(OcrError::Refusal {
            category: Some("cyber".into()),
            explanation: Some("Fixture refusal explanation.".into()),
        })
    );
}

#[test]
fn truncated_answer_is_an_error() {
    assert_eq!(parse("max_tokens"), Err(OcrError::MaxTokens));
}

#[test]
fn api_error_status() {
    assert_eq!(
        parse_response(529, &fixture("api_error"), (1, 1)),
        Err(OcrError::Api {
            status: 529,
            kind: "overloaded_error".into(),
            message: "Overloaded".into(),
        })
    );
    let Err(OcrError::Api { status, kind, .. }) = parse_response(502, b"<html>Bad gateway", (1, 1))
    else {
        panic!("expected an API error");
    };
    assert_eq!((status, kind.as_str()), (502, "unknown"));
}

#[test]
fn non_json_answer_is_a_parse_error() {
    let body = String::from_utf8(fixture("success"))
        .unwrap()
        .replace(r#""text": "{"#, r#""text": "not json {"#);
    assert!(matches!(
        parse_response(200, body.as_bytes(), (1, 1)),
        Err(OcrError::Parse(_))
    ));
}

/// Sends a real photo to Claude. Costs money, so it's opt-in.
#[test]
#[ignore]
fn live_transcribes_fixture_photo() {
    let Some(credentials) = Credentials::from_env() else {
        eprintln!("skipped: set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN");
        return;
    };
    let name = std::env::var("OCR_LIVE_PHOTO").unwrap_or_else(|_| "spread-wood-1".into());
    let bytes = std::fs::read(fixtures_dir().join(format!("{name}.jpg"))).unwrap();
    let img = notebook_core::decode(&bytes).unwrap();
    let page = notebook_core::ocr::image_to_send(&img, &notebook_core::CleanParams::default());
    let (status, body) =
        notebook_core::ocr::send(&credentials, &request_body(&encode_jpeg(&page), EFFORT)).unwrap();

    let out_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("ocr-live");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out = out_dir.join(format!("{name}.json"));
    std::fs::write(&out, &body).unwrap();
    eprintln!("response saved to {}", out.display());

    let transcript = parse_response(status, &body, page.dimensions()).unwrap();
    eprintln!(
        "{} {:?}\n{}",
        transcript.model,
        transcript.usage,
        transcript.markdown()
    );
    assert!(transcript.lines.iter().any(|l| !l.text_md.is_empty()));
    assert_boxes_in_bounds(&transcript);
}
