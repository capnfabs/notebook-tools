//! Spike: how accurate are Claude's line and uncertain-word boxes, and what does effort cost?
//! See `tasks/bbox-spike.md`.
//!
//! ```sh
//! # Sends each photo at every effort, plus a smaller image at the default effort. Costs money.
//! scripts/with-claude-token.sh cargo run -p notebook_core --example bbox_spike -- \
//!     run sample_images/*.jpeg [--only <variant>] [--out target/tmp/bbox-spike]
//! # Redraws overlays and the summary from saved responses, without calling the API.
//! cargo run -p notebook_core --example bbox_spike -- \
//!     draw crates/core/tests/fixtures/ocr/bbox-spike --photos crates/core/tests/fixtures \
//!     --out target/tmp/bbox-spike-fixtures
//! ```
//!
//! `run` writes each raw response to `<out>/<photo>-<variant>.json` and what was sent to
//! `<out>/runs.json`, then draws. Runs that got an answer, even an error, are skipped, so running
//! again only retries requests that never got through. Overlays are `<out>/<photo>-<variant>.png`: line boxes in blue,
//! uncertain words in red.

use std::path::{Path, PathBuf};
use std::time::Instant;

use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_hollow_rect_mut;
use imageproc::rect::Rect;
use notebook_core::ocr::{self, Bbox, Credentials, Span, Transcript};
use notebook_core::{CleanParams, crop_to_page, decode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// we don't do max, it's too expensive
const EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];
/// The smaller image, sent at the default effort only.
const SMALL_SIDE: u32 = 1288;
/// Name of the variant sent the way the app sends it, with [`ocr::MAX_IMAGE_PIXELS`]. The other
/// variants predate that limit: at 2576px the API shrank them before Claude saw them, and now that
/// requests ask it not to resize, it would reject them. See
/// <https://platform.claude.com/docs/en/build-with-claude/vision-coordinates#how-claude-resizes-and-pads-images>.
const CAPPED: &str = "capped";
const LINE_COLOUR: Rgba<u8> = Rgba([0, 90, 255, 255]);
const UNCERTAIN_COLOUR: Rgba<u8> = Rgba([230, 0, 0, 255]);

/// One request, as recorded in `runs.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Run {
    /// File name of the photo, looked up in the photos directory when drawing.
    photo: String,
    variant: String,
    effort: String,
    max_side: u32,
    /// Size of the image sent, which the response's boxes are measured in.
    sent_size: (u32, u32),
    elapsed_s: f32,
    /// `None` if the request never got an answer.
    status: Option<u16>,
    /// Why the run failed, if it did.
    error: Option<String>,
}

impl Run {
    fn stem(&self) -> String {
        let photo = Path::new(&self.photo)
            .file_stem()
            .unwrap()
            .to_string_lossy();
        format!("{photo}-{}", self.variant)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        let at = args.iter().position(|a| a == name)?;
        Some(PathBuf::from(&args[at + 1]))
    };
    let positional: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--")))
        .map(|(_, a)| a)
        .collect();
    match positional.split_first() {
        Some((cmd, photos)) if cmd.as_str() == "run" && !photos.is_empty() => {
            let out = flag("--out").unwrap_or_else(|| "target/tmp/bbox-spike".into());
            let photos: Vec<PathBuf> = photos.iter().map(PathBuf::from).collect();
            let only = flag("--only").map(|v| v.to_string_lossy().into_owned());
            run(&photos, only.as_deref(), &out);
            let photos_dir = photos[0].parent().unwrap().to_owned();
            draw(&out, &photos_dir, &out);
        }
        Some((cmd, [dir])) if cmd.as_str() == "draw" => {
            let dir = PathBuf::from(dir);
            let photos = flag("--photos").unwrap_or_else(|| "sample_images".into());
            let out = flag("--out").unwrap_or_else(|| dir.clone());
            draw(&dir, &photos, &out);
        }
        _ => {
            eprintln!("usage: bbox_spike run <photos…> [--only <variant>] [--out <dir>]");
            eprintln!("       bbox_spike draw <dir> [--photos <dir>] [--out <dir>]");
            std::process::exit(2);
        }
    }
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("runs.json")
}

fn read_manifest(dir: &Path) -> Vec<Run> {
    std::fs::read(manifest_path(dir))
        .map(|bytes| serde_json::from_slice(&bytes).expect("runs.json is a list of runs"))
        .unwrap_or_default()
}

/// Sends every variant of every photo not already answered, or only the variant named `only`.
fn run(photos: &[PathBuf], only: Option<&str>, out: &Path) {
    let credentials =
        Credentials::from_env().expect("no credentials: run under scripts/with-claude-token.sh");
    std::fs::create_dir_all(out).unwrap();
    let mut done: Vec<Run> = read_manifest(out)
        .into_iter()
        .filter(|r| r.status.is_some())
        .collect();

    // Every variant of every photo, minus those already done.
    let mut jobs = Vec::new();
    for path in photos {
        let photo = path.file_name().unwrap().to_string_lossy().into_owned();
        let img = decode(&std::fs::read(path).unwrap()).unwrap();
        let default_side = CleanParams::default().max_output_side;
        let variants = EFFORTS
            .iter()
            .map(|&e| (e.to_owned(), e, default_side))
            .chain([
                (
                    format!("{}-{SMALL_SIDE}", ocr::EFFORT),
                    ocr::EFFORT,
                    SMALL_SIDE,
                ),
                (
                    format!("{}-{CAPPED}", ocr::EFFORT),
                    ocr::EFFORT,
                    default_side,
                ),
            ]);
        for (variant, effort, max_side) in variants {
            if only.is_some_and(|only| only != variant)
                || done
                    .iter()
                    .any(|r| r.photo == photo && r.variant == variant)
            {
                continue;
            }
            let params = CleanParams {
                max_output_side: max_side,
                ..CleanParams::default()
            };
            let page = if variant.ends_with(CAPPED) {
                ocr::image_to_send(&img, &params)
            } else {
                crop_to_page(&img, &params).0
            };
            jobs.push((photo.clone(), variant, effort, max_side, page));
        }
    }
    eprintln!("{} requests to send", jobs.len());

    // A few at a time: each takes a minute or more.
    let finished: Vec<Run> = std::thread::scope(|s| {
        let handles: Vec<_> = jobs
            .chunks(jobs.len().div_ceil(6).max(1))
            .map(|chunk| {
                let credentials = &credentials;
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|(photo, variant, effort, max_side, page)| {
                            let jpeg = ocr::encode_jpeg(page);
                            let started = Instant::now();
                            let sent = ocr::send(
                                credentials,
                                &ocr::request_body(&jpeg, ocr::Model::Opus, effort),
                            );
                            let mut run = Run {
                                photo: photo.clone(),
                                variant: variant.clone(),
                                effort: (*effort).to_owned(),
                                max_side: *max_side,
                                sent_size: page.dimensions(),
                                elapsed_s: started.elapsed().as_secs_f32(),
                                status: None,
                                error: None,
                            };
                            match sent {
                                Ok((status, body)) => {
                                    run.status = Some(status);
                                    std::fs::write(out.join(format!("{}.json", run.stem())), &body)
                                        .unwrap();
                                    let parsed = ocr::parse_response(status, &body, run.sent_size);
                                    run.error = parsed.err().map(|e| e.to_string());
                                }
                                Err(err) => run.error = Some(err.to_string()),
                            }
                            eprintln!(
                                "{} in {:.0}s{}",
                                run.stem(),
                                run.elapsed_s,
                                run.error
                                    .as_deref()
                                    .map(|e| format!(": {e}"))
                                    .unwrap_or_default()
                            );
                            run
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });

    done.extend(finished);
    done.sort_by(|a, b| (&a.photo, &a.variant).cmp(&(&b.photo, &b.variant)));
    let manifest = serde_json::to_string_pretty(&done).unwrap();
    std::fs::write(manifest_path(out), manifest + "\n").unwrap();
}

/// Draws an overlay per successful run and prints the summary table.
fn draw(dir: &Path, photos: &Path, out: &Path) {
    std::fs::create_dir_all(out).unwrap();
    println!(
        "{:<24} {:>11} {:>6} {:>6} {:>8} {:>5} {:>5} {:>4} {:>4} {:>6} {:>6}",
        "run", "sent", "in", "out", "thinking", "secs", "lines", "unc", "oob", "by idx", "by box"
    );
    let mut cropped: Option<(String, u32, RgbaImage)> = None;
    for run in read_manifest(dir) {
        if let Some(err) = &run.error {
            println!("{:<24} failed: {err}", run.stem());
            continue;
        }
        let body = std::fs::read(dir.join(format!("{}.json", run.stem()))).unwrap();
        let transcript = ocr::parse_response(run.status.unwrap(), &body, run.sent_size).unwrap();
        let raw: Value = serde_json::from_slice(&body).unwrap();
        let thinking = &raw["usage"]["output_tokens_details"]["thinking_tokens"];
        let stats = Stats::of(&transcript);
        println!(
            "{:<24} {:>11} {:>6} {:>6} {:>8} {:>5.0} {:>5} {:>4} {:>4} {:>6} {:>6}",
            run.stem(),
            format!("{}x{}", run.sent_size.0, run.sent_size.1),
            transcript.usage.input_tokens,
            transcript.usage.output_tokens,
            thinking.as_u64().map_or("?".into(), |t| t.to_string()),
            run.elapsed_s,
            stats.lines,
            transcript.uncertain.len(),
            stats.out_of_bounds,
            format!("{}/{}", stats.right_by_index, stats.located),
            format!("{}/{}", stats.right_by_box, stats.located),
        );

        // Variants share the photo's crop at a given size; don't redo it for each.
        let key = (run.photo.clone(), run.max_side);
        if cropped
            .as_ref()
            .is_none_or(|(p, s, _)| (p, s) != (&key.0, &key.1))
        {
            let img = decode(&std::fs::read(photos.join(&run.photo)).unwrap()).unwrap();
            let params = CleanParams {
                max_output_side: run.max_side,
                ..CleanParams::default()
            };
            cropped = Some((key.0, key.1, crop_to_page(&img, &params).0));
        }
        let mut overlay = cropped.as_ref().unwrap().2.clone();
        draw_boxes(&mut overlay, &transcript);
        overlay
            .save(out.join(format!("{}.png", run.stem())))
            .unwrap();
    }
    println!(
        "\noob: boxes outside the image. by idx / by box: uncertain words whose line is right going \
         by the line index Claude gave / by which line box holds the word's box, out of those \
         whose text was found."
    );
}

/// Draws `transcript`'s boxes on `img`, which may be the sent image at another scale.
fn draw_boxes(img: &mut RgbaImage, transcript: &Transcript) {
    let sx = img.width() as f32 / transcript.image_size.0 as f32;
    let sy = img.height() as f32 / transcript.image_size.1 as f32;
    let thickness = (img.width().max(img.height()) / 1000).max(1) as i32;
    let mut rect = |[x0, y0, x1, y1]: Bbox, colour, thickness: i32| {
        for t in 0..thickness {
            let (x, y) = ((x0 * sx) as i32 - t, (y0 * sy) as i32 - t);
            let w = (((x1 - x0) * sx) as i32 + 2 * t).max(1) as u32;
            let h = (((y1 - y0) * sy) as i32 + 2 * t).max(1) as u32;
            draw_hollow_rect_mut(img, Rect::at(x, y).of_size(w, h), colour);
        }
    };
    for bbox in transcript.lines.iter().filter_map(|l| l.bbox) {
        rect(bbox, LINE_COLOUR, thickness);
    }
    for bbox in transcript.uncertain.iter().filter_map(|u| u.bbox) {
        rect(bbox, UNCERTAIN_COLOUR, thickness * 2);
    }
}

struct Stats {
    /// Lines with text.
    lines: usize,
    out_of_bounds: usize,
    /// Uncertain words whose text was found, by box or near their claimed line.
    located: usize,
    right_by_index: usize,
    right_by_box: usize,
}

impl Stats {
    fn of(t: &Transcript) -> Self {
        let (w, h) = (t.image_size.0 as f32, t.image_size.1 as f32);
        let in_bounds = |[x0, y0, x1, y1]: Bbox| {
            0.0 <= x0 && x0 <= x1 && x1 <= w && 0.0 <= y0 && y0 <= y1 && y1 <= h
        };
        let boxes = t.lines.iter().map(|l| l.bbox);
        let out_of_bounds = boxes
            .chain(t.uncertain.iter().map(|u| u.bbox))
            .flatten()
            .filter(|&b| !in_bounds(b))
            .count();

        let (mut located, mut right_by_index, mut right_by_box) = (0, 0, 0);
        for (u, span) in t.uncertain.iter().zip(t.uncertain_spans()) {
            let Some(Span { line, .. }) = span else {
                continue;
            };
            located += 1;
            right_by_index += usize::from(u.line == line);
            right_by_box += usize::from(u.bbox.and_then(|b| t.line_by_box(b)) == Some(line));
        }
        Self {
            lines: t.lines.iter().filter(|l| !l.text_md.is_empty()).count(),
            out_of_bounds,
            located,
            right_by_index,
            right_by_box,
        }
    }
}
