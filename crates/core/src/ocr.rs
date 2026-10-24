//! Transcribes a page with Claude through the Messages API. Building the request and parsing the
//! response are plain functions, testable without the network; only [`send`] and [`fetch_page`]
//! make requests.
//!
//! Response shape: `decisions/2026-10-23-ocr-response-shape.md`.

use std::fmt;
use std::ops::Range;
use std::time::SystemTime;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::buffer::ConvertBuffer as _;
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder as _, RgbImage, RgbaImage};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

/// Which Claude transcribes. Both see images at the same resolution, so [`MAX_IMAGE_PIXELS`] suits
/// either.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Model {
    #[default]
    Opus,
    Sonnet,
}

impl Model {
    pub const ALL: [Self; 2] = [Self::Opus, Self::Sonnet];

    /// The API's name for it.
    pub fn id(self) -> &'static str {
        match self {
            Self::Opus => "claude-opus-5-5",
            Self::Sonnet => "claude-sonnet-5-5",
        }
    }

    /// Its family, without version or effort: names its [`crate::cache`] entries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Sonnet => "sonnet",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Opus => "Opus",
            Self::Sonnet => "Sonnet",
        }
    }

    /// The family of the model the API names `id`, which may be a fallback's.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|m| id.starts_with(&format!("claude-{}", m.name())))
    }

    /// Dollars per million input and output tokens.
    pub fn prices(self) -> (f64, f64) {
        match self {
            Self::Opus => (4.0, 20.0),
            Self::Sonnet => (2.0, 10.0),
        }
    }
}

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

/// How many lines either side of its claimed line an uncertain word is looked for in.
pub const LINE_SEARCH_RADIUS: usize = 2;

/// `[x0, y0, x1, y1]` in pixels of the image sent.
pub type Bbox = [f32; 4];

/// A page's transcription, as Claude returned it.
#[derive(Clone, Debug, PartialEq)]
pub struct Transcript {
    pub lines: Vec<Line>,
    pub uncertain: Vec<Uncertain>,
    /// Size of the image sent, which the boxes are measured in.
    pub image_size: (u32, u32),
    /// The model that answered; differs from the one asked for if a fallback ran.
    pub model: String,
    /// What the request cost; for a cached transcript, what it cost when it was made.
    pub usage: Usage,
    /// Loaded from the [`crate::cache`] rather than just transcribed.
    pub from_cache: bool,
    /// When the [`crate::cache`] stored it. `None` if it wasn't stored.
    pub stored_at: Option<SystemTime>,
    /// The user edited a line by hand. Never part of Claude's answer.
    pub edited: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Line {
    pub text_md: String,
    /// `None` for paragraph breaks, or if Claude's box wasn't four numbers.
    #[serde(deserialize_with = "lenient_bbox")]
    pub bbox: Option<Bbox>,
}

/// A word or phrase Claude wasn't sure of.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Uncertain {
    /// Index into [`Transcript::lines`]. Claude sometimes miscounts this by one or so.
    pub line: usize,
    /// As it appears in that line's `text_md`.
    pub text: String,
    #[serde(deserialize_with = "lenient_bbox")]
    pub bbox: Option<Bbox>,
    pub alternatives: Vec<String>,
    /// The user corrected or accepted this word. Never part of Claude's answer.
    #[serde(skip)]
    pub resolved: bool,
}

/// Where an [`Uncertain`] word is: a byte range in one line's `text_md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub range: Range<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(from = "RawUsage")]
pub struct Usage {
    pub input_tokens: u64,
    /// Including thinking.
    pub output_tokens: u64,
    pub thinking_tokens: u64,
}

impl Usage {
    /// What it cost in dollars, at `model`'s prices.
    pub fn cost(&self, model: Model) -> f64 {
        let (input, output) = model.prices();
        (self.input_tokens as f64 * input + self.output_tokens as f64 * output) / 1e6
    }
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    output_tokens_details: Option<OutputTokensDetails>,
}

#[derive(Deserialize)]
struct OutputTokensDetails {
    #[serde(default)]
    thinking_tokens: u64,
}

impl From<RawUsage> for Usage {
    fn from(raw: RawUsage) -> Self {
        Self {
            input_tokens: raw.input_tokens,
            output_tokens: raw.output_tokens,
            thinking_tokens: raw.output_tokens_details.map_or(0, |d| d.thinking_tokens),
        }
    }
}

impl Transcript {
    /// The page as markdown: one line per handwritten line.
    pub fn markdown(&self) -> String {
        let lines: Vec<&str> = self.lines.iter().map(|l| l.text_md.as_str()).collect();
        lines.join("\n")
    }

    /// Where each [`Uncertain`] is, in the same order. Its text is looked for in the lines whose
    /// box holds its box's centre, then in its claimed line and the nearest lines within
    /// [`LINE_SEARCH_RADIUS`]: Claude's line index can be far off, its boxes rarely are. The first
    /// match in a line wins. `None` if it isn't found.
    pub fn uncertain_spans(&self) -> Vec<Option<Span>> {
        self.uncertain.iter().map(|u| self.locate(u)).collect()
    }

    fn locate(&self, u: &Uncertain) -> Option<Span> {
        if u.text.is_empty() {
            return None;
        }
        let by_box = u.bbox.map(|b| self.lines_by_box(b)).unwrap_or_default();
        let nearest_lines = (0..=LINE_SEARCH_RADIUS)
            .flat_map(|d| [u.line.checked_sub(d), u.line.checked_add(d)])
            .flatten();
        by_box.into_iter().chain(nearest_lines).find_map(|line| {
            let start = self.lines.get(line)?.text_md.find(&u.text)?;
            Some(Span {
                line,
                range: start..start + u.text.len(),
            })
        })
    }

    /// The line whose box holds the centre of `bbox`; the vertically nearest if several do.
    pub fn line_by_box(&self, bbox: Bbox) -> Option<usize> {
        self.lines_by_box(bbox).first().copied()
    }

    /// The lines whose boxes hold the centre of `bbox`, vertically nearest first.
    fn lines_by_box(&self, [x0, y0, x1, y1]: Bbox) -> Vec<usize> {
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let mut lines: Vec<(usize, f32)> = self
            .lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| Some((i, l.bbox?)))
            .filter(|(_, [lx0, ly0, lx1, ly1])| {
                (*lx0..=*lx1).contains(&cx) && (*ly0..=*ly1).contains(&cy)
            })
            .map(|(i, [_, ly0, _, ly1])| (i, ((ly0 + ly1) / 2.0 - cy).abs()))
            .collect();
        lines.sort_by(|(_, a), (_, b)| a.total_cmp(b));
        lines.into_iter().map(|(i, _)| i).collect()
    }

    /// Replaces uncertain word `index` with `replacement` and marks it resolved, keeping the old
    /// reading among its alternatives. Returns false, changing nothing, if the word isn't found.
    pub fn resolve(&mut self, index: usize, replacement: &str) -> bool {
        let Some(u) = self.uncertain.get(index) else {
            return false;
        };
        let Some(Span { line, range }) = self.locate(u) else {
            return false;
        };
        self.lines[line].text_md.replace_range(range, replacement);
        let u = &mut self.uncertain[index];
        let old = std::mem::replace(&mut u.text, replacement.to_owned());
        if old != replacement && !u.alternatives.contains(&old) {
            u.alternatives.insert(0, old);
        }
        u.alternatives.retain(|a| a != replacement);
        u.line = line;
        u.resolved = true;
        true
    }

    /// Replaces line `index`'s markdown. Uncertain words no longer in it stop being found.
    pub fn set_line(&mut self, index: usize, text_md: String) {
        if let Some(line) = self.lines.get_mut(index)
            && line.text_md != text_md
        {
            line.text_md = text_md;
            self.edited = true;
        }
    }

    /// Uncertain words still to correct or keep: found in the text and not yet resolved.
    pub fn open_uncertain(&self) -> usize {
        self.uncertain
            .iter()
            .filter(|u| !u.resolved && self.locate(u).is_some())
            .count()
    }

    /// The user has corrected or kept an uncertain word, or edited a line: work that
    /// transcribing the page again would lose.
    pub fn corrected(&self) -> bool {
        self.edited || self.uncertain.iter().any(|u| u.resolved)
    }
}

/// A box that isn't exactly four numbers becomes `None` rather than failing the whole page.
fn lenient_bbox<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Bbox>, D::Error> {
    let bbox = Option::<Vec<f32>>::deserialize(d)?;
    Ok(bbox.and_then(|b| b.try_into().ok()))
}

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
    let pixels = u64::from(w) * u64::from(h);
    if pixels <= u64::from(MAX_IMAGE_PIXELS) {
        return page;
    }
    let scale = (f64::from(MAX_IMAGE_PIXELS) / pixels as f64).sqrt();
    crate::shrink_to_fit(page, (f64::from(w.max(h)) * scale) as u32)
}

/// Sends a page to `model` from its file's bytes (see [`image_to_send`]), returning the response
/// for [`parse_response`] or the [`crate::cache`].
pub fn fetch_page(
    credentials: &Credentials,
    page_bytes: &[u8],
    params: &crate::CleanParams,
    model: Model,
) -> Result<crate::cache::Fetched, OcrError> {
    let img = crate::decode(page_bytes).map_err(|e| OcrError::Decode(e.to_string()))?;
    let page = image_to_send(&img, params);
    let body = request_body(&encode_jpeg(&page), model, EFFORT);
    let (status, body) = send(credentials, &body)?;
    Ok((status, body, page.dimensions()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn line(text_md: &str) -> Line {
        Line {
            text_md: text_md.to_owned(),
            bbox: (!text_md.is_empty()).then_some([0.0, 0.0, 10.0, 10.0]),
        }
    }

    fn uncertain(line: usize, text: &str) -> Uncertain {
        Uncertain {
            line,
            text: text.to_owned(),
            // Found by its line index alone.
            bbox: None,
            alternatives: vec!["other".to_owned()],
            resolved: false,
        }
    }

    /// A line with a box at row `row`, 100px tall.
    fn line_at(text_md: &str, row: f32) -> Line {
        Line {
            text_md: text_md.to_owned(),
            bbox: Some([0.0, row * 100.0, 1000.0, row * 100.0 + 100.0]),
        }
    }

    /// Text of `t`'s uncertain word `index` as found, and its line.
    fn found(t: &Transcript, index: usize) -> Option<(usize, &str)> {
        let Span { line, range } = t.uncertain_spans()[index].clone()?;
        Some((line, &t.lines[line].text_md[range]))
    }

    fn transcript(lines: &[&str], uncertain: Vec<Uncertain>) -> Transcript {
        Transcript {
            lines: lines.iter().copied().map(line).collect(),
            uncertain,
            image_size: (100, 100),
            model: Model::Opus.id().to_owned(),
            usage: Usage::default(),
            from_cache: false,
            stored_at: None,
            edited: false,
        }
    }

    fn vars<'a>(set: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |name| {
            set.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn markdown_joins_lines_with_paragraph_breaks() {
        let t = transcript(&["# Monday", "- eggs", "", "Call Sam"], vec![]);
        assert_eq!(t.markdown(), "# Monday\n- eggs\n\nCall Sam");
    }

    #[test]
    fn uncertain_spans_point_into_lines() {
        let t = transcript(
            &["# Monday", "", "buy eggs and eggs", "café visit"],
            vec![
                uncertain(2, "eggs"),
                uncertain(3, "visit"),
                uncertain(3, "café"),
                uncertain(0, "Tuesday"),
                uncertain(9, "eggs"),
                uncertain(2, ""),
            ],
        );
        let spans = t.uncertain_spans();
        assert_eq!(
            spans[0],
            Some(Span {
                line: 2,
                range: 4..8
            }),
            "first match in the line"
        );
        assert_eq!(found(&t, 1), Some((3, "visit")));
        assert_eq!(found(&t, 2), Some((3, "café")));
        assert_eq!(spans[3..], [None, None, None]);
    }

    /// Seen in a live response: line indices one off in both directions.
    #[test]
    fn uncertain_spans_tolerate_miscounted_lines() {
        let lines = ["a", "b", "eggs", "c", "d", "e", "eggs too"];
        let line_of = |claimed: usize, text: &str| {
            let t = transcript(&lines, vec![uncertain(claimed, text)]);
            Some(found(&t, 0)?.0)
        };
        assert_eq!(line_of(3, "eggs"), Some(2), "one too high");
        assert_eq!(line_of(5, "eggs"), Some(6), "one too low");
        assert_eq!(line_of(4, "eggs"), Some(2), "earlier line wins a tie");
        assert_eq!(line_of(0, "too"), None, "too far away");
    }

    /// Claude's line index can be ~15 off; its boxes are better.
    #[test]
    fn uncertain_spans_go_by_box_first() {
        let lines: Vec<Line> = ["eggs", "a", "b", "c", "d", "e", "eggs"]
            .iter()
            .zip(0..)
            .map(|(text, row)| line_at(text, row as f32))
            .collect();
        let word = |claimed, bbox| Uncertain {
            bbox,
            ..uncertain(claimed, "eggs")
        };
        let t = Transcript {
            lines,
            uncertain: vec![
                // Claims line 0, but the box is on line 6, and a little low.
                word(0, Some([10.0, 640.0, 60.0, 720.0])),
                // A box between lines falls back to the index.
                word(1, Some([10.0, 1000.0, 60.0, 1100.0])),
                // The box's line doesn't have the text.
                word(6, Some([10.0, 300.0, 60.0, 350.0])),
                word(0, None),
            ],
            ..transcript(&[], vec![])
        };
        let lines: Vec<_> = (0..4).map(|i| found(&t, i).map(|(line, _)| line)).collect();
        assert_eq!(lines, [Some(6), Some(0), Some(6), Some(0)]);
        assert_eq!(t.line_by_box([10.0, 640.0, 60.0, 720.0]), Some(6));
        assert_eq!(t.line_by_box([10.0, 1000.0, 60.0, 1100.0]), None);
    }

    #[test]
    fn resolving_replaces_the_word_and_keeps_the_old_reading() {
        let mut t = transcript(
            &["ring the plumber re: tap"],
            vec![Uncertain {
                alternatives: vec!["plumbers".into(), "plunber".into()],
                ..uncertain(0, "plumber")
            }],
        );
        assert!(t.resolve(0, "plumbers"));
        assert_eq!(t.lines[0].text_md, "ring the plumbers re: tap");
        let u = &t.uncertain[0];
        assert!(u.resolved);
        assert_eq!(u.text, "plumbers");
        assert_eq!(u.alternatives, ["plumber", "plunber"]);
        assert_eq!(found(&t, 0), Some((0, "plumbers")));

        // Back again, then a reading of the user's own.
        assert!(t.resolve(0, "plumber"));
        assert_eq!(t.uncertain[0].alternatives, ["plumbers", "plunber"]);
        assert!(t.resolve(0, "Plumbing Co"));
        assert_eq!(t.lines[0].text_md, "ring the Plumbing Co re: tap");
        assert_eq!(
            t.uncertain[0].alternatives,
            ["plumber", "plumbers", "plunber"]
        );
    }

    #[test]
    fn keeping_a_reading_resolves_it_unchanged() {
        let mut t = transcript(&["buy eggs"], vec![uncertain(0, "eggs")]);
        assert!(t.resolve(0, "eggs"));
        assert_eq!(t.lines[0].text_md, "buy eggs");
        assert!(t.uncertain[0].resolved);
        assert_eq!(t.uncertain[0].alternatives, ["other"]);
    }

    #[test]
    fn unfound_words_cant_be_resolved() {
        let mut t = transcript(&["buy eggs"], vec![uncertain(0, "milk")]);
        let before = t.clone();
        assert!(!t.resolve(0, "cheese"));
        assert!(!t.resolve(5, "cheese"));
        assert_eq!(t, before);
    }

    #[test]
    fn editing_a_line_replaces_its_markdown() {
        let mut t = transcript(&["buy eggs", "call Sam"], vec![uncertain(0, "eggs")]);
        t.set_line(0, "buy milk".into());
        assert_eq!(t.markdown(), "buy milk\ncall Sam");
        assert_eq!(t.uncertain_spans(), [None]);
        t.set_line(9, "ignored".into());
        assert_eq!(t.markdown(), "buy milk\ncall Sam");
    }

    #[test]
    fn corrections_are_noticed() {
        let mut t = transcript(&["buy eggs", "call Sam"], vec![uncertain(0, "eggs")]);
        assert!(!t.corrected());
        t.set_line(1, "call Sam".into());
        assert!(!t.corrected(), "unchanged line");
        t.set_line(1, "call Sam!".into());
        assert!(t.corrected());

        let mut t = transcript(&["buy eggs"], vec![uncertain(0, "eggs")]);
        t.resolve(0, "eggs");
        assert!(t.corrected(), "kept a reading");
    }

    #[test]
    fn open_uncertain_counts_found_unresolved_words() {
        let mut t = transcript(
            &["buy eggs and milk"],
            vec![
                uncertain(0, "eggs"),
                uncertain(0, "milk"),
                uncertain(0, "cheese"),
            ],
        );
        assert_eq!(t.open_uncertain(), 2, "cheese isn't in the text");
        t.resolve(0, "eggs");
        assert_eq!(t.open_uncertain(), 1);
        t.resolve(1, "mint");
        assert_eq!(t.open_uncertain(), 0);
    }

    #[test]
    fn models_by_id_and_cost() {
        assert_eq!(Model::from_id("claude-opus-5-5"), Some(Model::Opus));
        assert_eq!(
            Model::from_id("claude-sonnet-5"),
            Some(Model::Sonnet),
            "a fallback"
        );
        assert_eq!(Model::from_id("claude-haiku-5-5"), None);
        let usage = Usage {
            input_tokens: 5_000,
            output_tokens: 2_000,
            thinking_tokens: 800,
        };
        assert!((usage.cost(Model::Opus) - 0.06).abs() < 1e-9);
        assert!((usage.cost(Model::Sonnet) - 0.03).abs() < 1e-9);
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

    #[test]
    fn malformed_box_becomes_none() {
        let line: Line = serde_json::from_str(r#"{"text_md": "hi", "bbox": [1, 2, 3]}"#).unwrap();
        assert_eq!(line.bbox, None);
    }
}
