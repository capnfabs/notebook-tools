//! The Claude request and recorded responses. Never calls the API, except the ignored live test:
//!
//! ```sh
//! scripts/with-claude-token.sh cargo test -p notebook_core --test claude_transcribe -- --ignored live
//! ```
//!
//! `OCR_LIVE_MODEL=sonnet` sends it to Sonnet instead of Opus. It saves the raw response to
//! `target/tmp/ocr-live/`; copy one into `tests/fixtures/claude/` to record it.

mod common;

use std::path::Path;

use common::{RECORDED_CROP, fixtures_dir, photo, response};
use image::{Rgba, RgbaImage};
use notebook_core::claude_transcribe::{
    Credentials, EFFORT, OcrError, encode_jpeg, image_to_send, parse_response, request_body, send,
};
use notebook_core::clean::{CleanParams, Crop};
use notebook_core::llm_model::Model;
use notebook_core::markdown_style::MarkdownStyle;
use notebook_core::transcript::{Span, Transcript};
use serde::Deserialize;

fn parse(name: &str) -> Result<Transcript, OcrError> {
    parse_response(
        200,
        &response(name),
        RECORDED_CROP,
        &MarkdownStyle::default(),
    )
}

/// Panics if any of `transcript`'s boxes isn't ordered or inside the image sent. Boxes tend to sit
/// a little low, so one on the bottom line can overrun slightly.
fn assert_boxes_in_bounds(transcript: &Transcript) {
    let (w, h) = (transcript.crop.size.0 as f32, transcript.crop.size.1 as f32);
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
    insta::assert_json_snapshot!(request_body(&encode_jpeg(&img), Model::Opus, EFFORT));
}

#[test]
fn request_body_sets_effort() {
    let body = request_body(&[], Model::Opus, "low");
    assert_eq!(body["output_config"]["effort"], "low");
}

#[test]
fn request_body_sets_model() {
    for model in Model::ALL {
        assert_eq!(request_body(&[], model, EFFORT)["model"], model.id());
    }
    assert_eq!(Model::Sonnet.id(), "claude-sonnet-5-5");
}

#[test]
fn parses_success() {
    let transcript = parse("success").unwrap();
    assert_eq!(transcript.model, Model::Opus.id());
    assert_eq!(transcript.crop, RECORDED_CROP);
    insta::assert_debug_snapshot!("success_transcript", transcript);
    insta::assert_snapshot!("success_markdown", transcript.markdown());
}

/// A real response for `spread-wood-1.jpg`, recorded by the live test.
#[test]
fn parses_recorded_response() {
    let transcript = parse("spread-wood-1").unwrap();
    assert_eq!(transcript.model, Model::Opus.id());
    assert!(transcript.usage.output_tokens > 0);
    assert_eq!(transcript.usage.thinking_tokens, 1393);
    // Two of these claim a line one off from where their text is.
    let spans = transcript.uncertain_spans();
    assert_eq!(spans.len(), 5);
    for (u, span) in transcript.uncertain.iter().zip(spans) {
        assert_eq!(span_text(&transcript, span), Some(u.text.as_str()));
    }
    insta::assert_snapshot!("spread_wood_1_markdown", transcript.markdown());
}

fn span_text(transcript: &Transcript, span: Option<Span>) -> Option<&str> {
    let Span { line, range } = span?;
    transcript.lines[line].text_md.get(range)
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

/// Real responses for photos `spread-wood-3` to `-6` at every effort and three image sizes,
/// recorded by a since-deleted spike: see `tasks/done/bbox-spike.md`.
#[test]
fn parses_bbox_spike_responses() {
    let dir = fixtures_dir().join("claude/bbox-spike");
    let runs: Vec<SpikeRun> =
        serde_json::from_slice(&std::fs::read(dir.join("runs.json")).unwrap()).unwrap();
    assert!(!runs.is_empty());
    let (mut words, mut unfound) = (0, 0);
    for run in runs {
        let photo = run.photo.trim_end_matches(".jpg");
        let name = format!("{photo}-{}", run.variant);
        // Never answered, so nothing was recorded.
        let Some(status) = run.status else { continue };
        let body = std::fs::read(dir.join(format!("{name}.json"))).unwrap();
        match (
            parse_response(
                status,
                &body,
                Crop::whole(run.sent_size.0, run.sent_size.1),
                &MarkdownStyle::default(),
            ),
            run.error,
        ) {
            (Ok(transcript), None) => {
                assert!(transcript.usage.output_tokens > 0, "{name}");
                assert_boxes_in_bounds(&transcript);
                for (u, span) in transcript
                    .uncertain
                    .iter()
                    .zip(transcript.uncertain_spans())
                {
                    words += 1;
                    match span_text(&transcript, span) {
                        Some(text) => assert_eq!(text, u.text, "{name}"),
                        None => unfound += 1,
                    }
                }
            }
            (Err(err), Some(recorded)) => assert_eq!(err.to_string(), recorded, "{name}"),
            (parsed, recorded) => panic!("{name}: parsed {parsed:?}, recorded {recorded:?}"),
        }
    }
    // Searching only near each word's claimed line missed 24: the index can be far off. See
    // `decisions/2026-10-23-bbox-accuracy.md`.
    assert_eq!((words, unfound), (123, 0));
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
        parse_response(
            529,
            &response("api_error"),
            Crop::whole(1, 1),
            &MarkdownStyle::default()
        ),
        Err(OcrError::Api {
            status: 529,
            kind: "overloaded_error".into(),
            message: "Overloaded".into(),
        })
    );
    let Err(OcrError::Api { status, kind, .. }) = parse_response(
        502,
        b"<html>Bad gateway",
        Crop::whole(1, 1),
        &MarkdownStyle::default(),
    ) else {
        panic!("expected an API error");
    };
    assert_eq!((status, kind.as_str()), (502, "unknown"));
}

#[test]
fn non_json_answer_is_a_parse_error() {
    let body = String::from_utf8(response("success"))
        .unwrap()
        .replace(r#""text": "{"#, r#""text": "not json {"#);
    assert!(matches!(
        parse_response(
            200,
            body.as_bytes(),
            Crop::whole(1, 1),
            &MarkdownStyle::default()
        ),
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
    let model = match std::env::var("OCR_LIVE_MODEL").as_deref() {
        Ok("sonnet") => Model::Sonnet,
        _ => Model::Opus,
    };
    let (page, crop) = image_to_send(&photo(&name), &CleanParams::default(), None);
    let body = request_body(&encode_jpeg(&page), model, EFFORT);
    let (status, body) = send(&credentials, &body).unwrap();

    let out_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("ocr-live");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out = out_dir.join(format!("{name}.json"));
    std::fs::write(&out, &body).unwrap();
    eprintln!("response saved to {}", out.display());

    let transcript = parse_response(status, &body, crop, &MarkdownStyle::default()).unwrap();
    eprintln!(
        "{} {:?}\n{}",
        transcript.model,
        transcript.usage,
        transcript.markdown()
    );
    assert!(transcript.lines.iter().any(|l| !l.text_md.is_empty()));
    assert_boxes_in_bounds(&transcript);
}
