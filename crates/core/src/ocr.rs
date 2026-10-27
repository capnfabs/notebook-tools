//! Transcribes a page with Claude through the Messages API. Building the request and parsing the
//! response are plain functions, testable without the network; only [`send`] and [`fetch_page`]
//! make requests.
//!
//! Response shape: `decisions/2026-10-23-ocr-response-shape.md`.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::buffer::ConvertBuffer as _;
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder as _, RgbImage, RgbaImage};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::llm_model::{Model, Usage};
use crate::page::max_side_within;
use crate::transcript::{Line, Transcript, Uncertain};

/// How hard Claude thinks: `low`, `medium`, `high`, `xhigh` or `max`.
pub const EFFORT: &str = "high";
pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";
/// Lets `fallbacks: "default"` re-run a refused request on another model.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Needed for OAuth access tokens sent as `Authorization: Bearer`.
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// Room for thinking plus a dense page. Non-streaming requests much bigger than this risk timeouts.
const MAX_TOKENS: u32 = 16_000;
/// The API's limit on one image, measured on the base64 text.
pub const MAX_IMAGE_BASE64_BYTES: usize = 5 * 1024 * 1024;
/// Most pixels sent in one image. The API shrinks an image until neither side is over 2576px and
/// it costs at most 4,784 visual tokens, one per 28×28px patch (about 3.75 MP). Claude's boxes are
/// then in the shrunk image's pixels, not ours. Partial patches count as whole ones, so this is a
/// little under, which keeps any image with sides up to 2576px within the token limit. The request
/// asks the API to reject a bigger image rather than resize it. See
/// <https://platform.claude.com/docs/en/build-with-claude/vision-coordinates#how-claude-resizes-and-pads-images>
/// and `decisions/2026-10-23-bbox-accuracy.md`.
pub const MAX_IMAGE_PIXELS: u32 = 3_600_000;

pub const PROMPT: &str = "\
Transcribe this photo of a handwritten notebook page.
- Write exactly what's on the page: keep the author's spelling, abbreviations and line breaks. Don't correct or complete anything.
- One `lines` entry per handwritten line, top to bottom, with its markdown and box. Use markdown only for structure the page shows (headings, bullets, numbered lists, checkboxes). An empty `text_md` with a null box marks a paragraph break.
- Crossed-out text: ~~strikethrough~~. Drawings: `[drawing: short description]`.
- Add an `uncertain` entry for each word or phrase you're unsure of: line index (0-based), the text exactly as it appears in that line, its box, and 1–3 other likely readings.
- Boxes are [x0, y0, x1, y1] in pixels of this image.";

/// Version of cached responses ([`crate::cache`]), which are also kept per [`Model`]. Bump it when
/// the schema changes, since old responses won't parse; a test checks for that. After a prompt,
/// model version or send-image change, bump it only if the cached transcriptions should be redone.
pub const CACHE_VERSION: u32 = 1;

#[derive(Debug, PartialEq)]
pub enum OcrError {
    /// The page's image couldn't be decoded.
    Decode(String),
    /// The API couldn't be reached.
    Network(String),
    /// The API answered with an error status.
    Api {
        status: u16,
        kind: String,
        message: String,
    },
    /// Claude (and any fallback) declined.
    Refusal {
        category: Option<String>,
        explanation: Option<String>,
    },
    /// The answer was cut off at [`MAX_TOKENS`].
    MaxTokens,
    /// The answer wasn't in the expected shape.
    Parse(String),
}

impl fmt::Display for OcrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(err) => write!(f, "couldn't decode the page: {err}"),
            Self::Network(err) => write!(f, "couldn't reach Claude: {err}"),
            Self::Api {
                status,
                kind,
                message,
            } => write!(f, "Claude API error {status} ({kind}): {message}"),
            Self::Refusal {
                category,
                explanation,
            } => {
                write!(f, "Claude declined to transcribe this page")?;
                if let Some(category) = category {
                    write!(f, " ({category})")?;
                }
                if let Some(explanation) = explanation {
                    write!(f, ": {explanation}")?;
                }
                Ok(())
            }
            Self::MaxTokens => write!(f, "the transcription was cut off (too long)"),
            Self::Parse(err) => write!(f, "unexpected response from Claude: {err}"),
        }
    }
}

impl std::error::Error for OcrError {}

/// How to authenticate with the API.
#[derive(Clone, PartialEq)]
pub enum Credentials {
    ApiKey(String),
    /// An OAuth access token, e.g. from `ant auth print-credentials --access-token`.
    OAuth(String),
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey(_) => f.write_str("ApiKey(..)"),
            Self::OAuth(_) => f.write_str("OAuth(..)"),
        }
    }
}

impl Credentials {
    /// `ANTHROPIC_API_KEY`, else `ANTHROPIC_AUTH_TOKEN`.
    pub fn from_env() -> Option<Self> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// [`Self::from_env`] with the variables looked up by `get`. Blank values count as unset.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let get = |name| {
            get(name)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        get("ANTHROPIC_API_KEY")
            .map(Self::ApiKey)
            .or_else(|| get("ANTHROPIC_AUTH_TOKEN").map(Self::OAuth))
    }
}

/// Request headers besides `content-type`.
pub fn headers(credentials: &Credentials) -> Vec<(&'static str, String)> {
    let mut headers = vec![("anthropic-version", API_VERSION.to_owned())];
    match credentials {
        Credentials::ApiKey(key) => {
            headers.push(("x-api-key", key.clone()));
            headers.push(("anthropic-beta", FALLBACK_BETA.to_owned()));
        }
        Credentials::OAuth(token) => {
            headers.push(("authorization", format!("Bearer {token}")));
            headers.push(("anthropic-beta", format!("{FALLBACK_BETA},{OAUTH_BETA}")));
        }
    }
    headers
}

/// `img` as a JPEG small enough for the API.
pub fn encode_jpeg(img: &RgbaImage) -> Vec<u8> {
    encode_jpeg_within(img, MAX_IMAGE_BASE64_BYTES)
}

/// `img` as a JPEG whose base64 is at most `limit` bytes, lowering quality as needed. At the lowest
/// quality tried it's returned even if still too big, and the API says so.
fn encode_jpeg_within(img: &RgbaImage, limit: usize) -> Vec<u8> {
    let rgb: RgbImage = img.convert();
    let mut quality = 85;
    loop {
        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(&mut bytes, quality)
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                ExtendedColorType::Rgb8,
            )
            .expect("encoding a JPEG in memory can't fail");
        if bytes.len().div_ceil(3) * 4 <= limit || quality <= 25 {
            return bytes;
        }
        quality -= 15;
    }
}

/// The Messages API request for transcribing `jpeg` with `model`, thinking at `effort` (see
/// [`EFFORT`]).
pub fn request_body(jpeg: &[u8], model: Model, effort: &str) -> Value {
    json!({
        "model": model.id(),
        "max_tokens": MAX_TOKENS,
        // Thinking is always on for this model; effort sets how much.
        "output_config": {
            "effort": effort,
            "format": { "type": "json_schema", "schema": schema() },
        },
        "fallbacks": "default",
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/jpeg",
                        "data": BASE64.encode(jpeg),
                    },
                    // Fail rather than resize, which would put Claude's boxes in another image's
                    // pixels. Default fallback routing skips models that would resize it.
                    "transformations": { "oversized_image": "error" },
                },
                { "type": "text", "text": PROMPT },
            ],
        }],
    })
}

fn schema() -> Value {
    let bbox = json!({ "type": "array", "items": { "type": "number" } });
    json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "text_md": { "type": "string" },
                        "bbox": { "anyOf": [bbox, { "type": "null" }] },
                    },
                    "required": ["text_md", "bbox"],
                    "additionalProperties": false,
                },
            },
            "uncertain": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "line": { "type": "integer" },
                        "text": { "type": "string" },
                        "bbox": bbox,
                        "alternatives": { "type": "array", "items": { "type": "string" } },
                    },
                    "required": ["line", "text", "bbox", "alternatives"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["lines", "uncertain"],
        "additionalProperties": false,
    })
}

#[derive(Deserialize)]
struct Response {
    model: String,
    stop_reason: Option<String>,
    stop_details: Option<StopDetails>,
    content: Vec<Block>,
    usage: Usage,
}

#[derive(Deserialize)]
struct StopDetails {
    category: Option<String>,
    explanation: Option<String>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(rename = "type")]
    kind: String,
    message: String,
}

#[derive(Deserialize)]
struct Answer {
    lines: Vec<Line>,
    uncertain: Vec<Uncertain>,
}

/// Turns an API response into a [`Transcript`]. `image_size` is the size of the image sent.
pub fn parse_response(
    status: u16,
    body: &[u8],
    image_size: (u32, u32),
) -> Result<Transcript, OcrError> {
    if !(200..300).contains(&status) {
        return Err(match serde_json::from_slice::<ErrorResponse>(body) {
            Ok(err) => OcrError::Api {
                status,
                kind: err.error.kind,
                message: err.error.message,
            },
            Err(_) => OcrError::Api {
                status,
                kind: "unknown".to_owned(),
                message: String::from_utf8_lossy(body).chars().take(200).collect(),
            },
        });
    }
    let response: Response =
        serde_json::from_slice(body).map_err(|e| OcrError::Parse(e.to_string()))?;
    match response.stop_reason.as_deref() {
        Some("refusal") => {
            let details = response.stop_details;
            return Err(OcrError::Refusal {
                category: details.as_ref().and_then(|d| d.category.clone()),
                explanation: details.and_then(|d| d.explanation),
            });
        }
        Some("max_tokens") => return Err(OcrError::MaxTokens),
        _ => {}
    }
    // Thinking and fallback blocks come first; the structured answer is the last text block.
    let text = response
        .content
        .iter()
        .rev()
        .find(|b| b.kind == "text")
        .and_then(|b| b.text.as_deref())
        .ok_or_else(|| OcrError::Parse("no text in the response".to_owned()))?;
    let answer: Answer = serde_json::from_str(text).map_err(|e| OcrError::Parse(e.to_string()))?;
    Ok(Transcript {
        lines: answer.lines,
        uncertain: answer.uncertain,
        image_size,
        model: response.model,
        usage: response.usage,
        from_cache: false,
        stored_at: None,
        edited: false,
    })
}

/// Posts `body` to the Messages API, returning the status and response body.
pub fn send(credentials: &Credentials, body: &Value) -> Result<(u16, Vec<u8>), OcrError> {
    let network = |e: reqwest::Error| OcrError::Network(e.to_string());
    let client = reqwest::blocking::Client::builder()
        // Thinking at high effort can take minutes.
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(network)?;
    let mut request = client
        .post(API_URL)
        .header("content-type", "application/json")
        .body(body.to_string());
    for (name, value) in headers(credentials) {
        request = request.header(name, value);
    }
    let response = request.send().map_err(network)?;
    let status = response.status().as_u16();
    Ok((status, response.bytes().map_err(network)?.to_vec()))
}

/// What's sent for `photo`: straightened and cropped to the page, with no lighting fix, and no
/// more than [`MAX_IMAGE_PIXELS`], so the API doesn't resize it and Claude's boxes are in its
/// pixels. See <https://platform.claude.com/docs/en/build-with-claude/vision-coordinates#how-claude-resizes-and-pads-images>.
pub fn image_to_send(photo: &RgbaImage, params: &crate::CleanParams) -> RgbaImage {
    let (page, _) = crate::crop_to_page(photo, params);
    let (w, h) = page.dimensions();
    crate::shrink_to_fit(page, max_side_within(w, h, MAX_IMAGE_PIXELS))
}

/// A response as it came back, for [`parse_response`] or the [`crate::cache`].
#[derive(Clone, Debug, PartialEq)]
pub struct Fetched {
    pub status: u16,
    pub body: Vec<u8>,
    /// Size of the image sent, which the response's boxes are measured in.
    pub image_size: (u32, u32),
}

/// Sends a page to `model` from its file's bytes (see [`image_to_send`]).
pub fn fetch_page(
    credentials: &Credentials,
    page_bytes: &[u8],
    params: &crate::CleanParams,
    model: Model,
) -> Result<Fetched, OcrError> {
    let img = crate::decode(page_bytes).map_err(|e| OcrError::Decode(e.to_string()))?;
    let page = image_to_send(&img, params);
    let body = request_body(&encode_jpeg(&page), model, EFFORT);
    let (status, body) = send(credentials, &body)?;
    Ok(Fetched {
        status,
        body,
        image_size: page.dimensions(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn vars<'a>(set: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |name| {
            set.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn api_key_wins_over_auth_token() {
        let both = [
            ("ANTHROPIC_API_KEY", "key"),
            ("ANTHROPIC_AUTH_TOKEN", "tok"),
        ];
        assert_eq!(
            Credentials::from_vars(vars(&both)),
            Some(Credentials::ApiKey("key".into()))
        );
        let token = [
            ("ANTHROPIC_API_KEY", "  "),
            ("ANTHROPIC_AUTH_TOKEN", "tok\n"),
        ];
        assert_eq!(
            Credentials::from_vars(vars(&token)),
            Some(Credentials::OAuth("tok".into()))
        );
        assert_eq!(Credentials::from_vars(vars(&[])), None);
    }

    #[test]
    fn credentials_debug_hides_secret() {
        assert_eq!(
            format!("{:?}", Credentials::ApiKey("sk-secret".into())),
            "ApiKey(..)"
        );
    }

    #[test]
    fn headers_per_credential_type() {
        assert_eq!(
            headers(&Credentials::ApiKey("key".into())),
            [
                ("anthropic-version", "2026-10-21".to_owned()),
                ("x-api-key", "key".to_owned()),
                (
                    "anthropic-beta",
                    "server-side-fallback-2026-07-01".to_owned()
                ),
            ]
        );
        assert_eq!(
            headers(&Credentials::OAuth("tok".into())),
            [
                ("anthropic-version", "2026-10-21".to_owned()),
                ("authorization", "Bearer tok".to_owned()),
                (
                    "anthropic-beta",
                    "server-side-fallback-2026-07-01,oauth-2025-04-20".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn jpeg_shrinks_to_fit_limit() {
        // Noise compresses badly, so quality makes a big difference to size.
        let mut seed = 1u32;
        let noise = RgbaImage::from_fn(256, 256, |_, _| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let [r, g, b, _] = seed.to_le_bytes();
            Rgba([r, g, b, 255])
        });
        let full = encode_jpeg_within(&noise, usize::MAX);
        let limit = full.len().div_ceil(3) * 4 / 2;
        let small = encode_jpeg_within(&noise, limit);
        assert!(small.len().div_ceil(3) * 4 <= limit, "{}", small.len());
        assert_eq!(
            image::load_from_memory(&small)
                .unwrap()
                .into_rgba8()
                .dimensions(),
            (256, 256)
        );
    }

    #[test]
    fn image_to_send_fits_pixel_limit() {
        let params = crate::CleanParams::default();
        // Uniform grey has no page outline, so it's sent whole, scaled to the long-side limit.
        let photo = RgbaImage::from_pixel(4000, 3000, Rgba([128, 128, 128, 255]));
        let (w, h) = image_to_send(&photo, &params).dimensions();
        assert!(w * h <= MAX_IMAGE_PIXELS, "{w}x{h}");
        assert!(
            w * h > MAX_IMAGE_PIXELS * 99 / 100,
            "{w}x{h} shrunk more than needed"
        );
        assert_eq!(
            (w as f32 / h as f32 * 100.0).round(),
            133.0,
            "aspect ratio kept"
        );

        let small = RgbaImage::from_pixel(1200, 900, Rgba([128, 128, 128, 255]));
        assert_eq!(image_to_send(&small, &params).dimensions(), (1200, 900));
    }

    /// Cached responses in the old shape wouldn't parse, so a schema change needs a new version.
    #[test]
    fn schema_change_bumps_cache_version() {
        const SCHEMA_HASH: &str =
            "3d79ef41f0f5e1d8a5bc5c4c162e9fc3010795e16a2d0bfe0461680963474f35";
        assert_eq!(
            (
                CACHE_VERSION,
                crate::cache::sha256_hex(schema().to_string().as_bytes()).as_str()
            ),
            (1, SCHEMA_HASH),
            "schema changed: bump CACHE_VERSION, then update this test"
        );
    }
}
