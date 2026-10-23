//! The Claude OCR request and recorded responses. Never calls the API, except the ignored live
//! test:
//!
//! ```sh
//! ANTHROPIC_API_KEY=... cargo test -p notebook_core --test ocr -- --ignored live
//! ```
//!
//! It saves the raw response to `target/tmp/ocr-live/`; copy one into `tests/fixtures/ocr/` to
//! record it.

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use notebook_core::ocr::{
    Credentials, MODEL, OcrError, Transcript, encode_jpeg, parse_response, request_body,
};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(format!("ocr/{name}.json"))).unwrap()
}

fn parse(name: &str) -> Result<Transcript, OcrError> {
    parse_response(200, &fixture(name), (1200, 900))
}

#[test]
fn request_body_snapshot() {
    let img = RgbaImage::from_pixel(2, 2, Rgba([255, 255, 255, 255]));
    insta::assert_json_snapshot!(request_body(&encode_jpeg(&img)));
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
    let (page, _) = notebook_core::crop_to_page(&img, &notebook_core::CleanParams::default());
    let (status, body) =
        notebook_core::ocr::send(&credentials, &request_body(&encode_jpeg(&page))).unwrap();

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
    let (w, h) = (page.width() as f32, page.height() as f32);
    let boxes = transcript.lines.iter().map(|l| l.bbox);
    for [x0, y0, x1, y1] in boxes
        .chain(transcript.uncertain.iter().map(|u| u.bbox))
        .flatten()
    {
        assert!(0.0 <= x0 && x0 <= x1 && x1 <= w, "{x0} {x1} in {w}");
        assert!(0.0 <= y0 && y0 <= y1 && y1 <= h, "{y0} {y1} in {h}");
    }
}
