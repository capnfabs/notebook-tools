//! Photos far smaller than a page should be, and transcribed pages relinked to resized copies of
//! theirs: nothing panics. Photos under `photo_meta::MIN_SIDE` can't be added or relinked any more,
//! but pages saved before that can still be this small. See
//! `decisions/2026-11-18-aspect-names-and-min-size.md`.
//! It still seems pretty sensible to have unit tests for tiny photos don't panic despite

mod common;

use std::io::Cursor;
use std::sync::Arc;

use common::recorded;
use image::{ImageFormat, Rgba, RgbaImage};
use notebook_core::claude_transcribe::image_to_send;
use notebook_core::clean::CleanParams;
use notebook_core::decode::decode;
use notebook_core::export::{self, PdfPage};
use notebook_core::geom::Quad;
use notebook_core::loaded::Loaded;
use notebook_core::page::Page;
use notebook_core::photo_meta::PhotoMeta;
use notebook_core::relink::Shape;
use notebook_core::transcript::Transcript;

/// A `w` x `h` cream page on a brown table, as a PNG.
fn png(w: u32, h: u32) -> Vec<u8> {
    let img = RgbaImage::from_fn(w, h, |x, y| {
        let inside = (w / 5..w * 4 / 5).contains(&x) && (h / 5..h * 4 / 5).contains(&y);
        Rgba(if inside {
            [235, 225, 200, 255]
        } else {
            [120, 80, 50, 255]
        })
    });
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

/// A page shaped quad inside a `w` x `h` photo, a tenth in from each side.
fn inset((w, h): (u32, u32)) -> Quad {
    let (w, h) = (w as f32, h as f32);
    let (x0, y0, x1, y1) = (w * 0.1, h * 0.1, w * 0.9, h * 0.9);
    [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

/// Every box in `transcript`, which must all be finite.
fn assert_finite(transcript: &Transcript) {
    let boxes = transcript.lines.iter().map(|l| l.bbox);
    let boxes = boxes.chain(transcript.uncertain.iter().map(|u| u.bbox));
    for bbox in boxes.flatten() {
        assert!(bbox.iter().all(|v| v.is_finite()), "{bbox:?}");
    }
}

/// Loads `bytes` cropped to `corners`, cuts the image Claude would be sent, maps `transcript`'s
/// boxes onto it and exports it as a PDF page: everything a page's photo goes through.
fn load_send_export(bytes: &[u8], corners: Option<Quad>, transcript: &Transcript) {
    let params = CleanParams::default();
    let loaded = Loaded::new(bytes, &params, corners).unwrap();
    let (sent, crop) = image_to_send(&decode(bytes).unwrap(), &params, corners);
    assert!(sent.width() > 0 && sent.height() > 0);
    let fit = transcript.fit(crop);
    assert!(!fit.untranscribed.is_nan());
    let reframed = transcript.reframed(crop);
    assert_finite(&reframed);
    let page = PdfPage {
        name: "tiny.png",
        jpeg: &loaded.cleaned_jpeg,
        size: loaded.report.output_size,
        transcript: &reframed,
    };
    export::pdf(&[page], "notes.md").unwrap();
}

#[test]
fn tiny_photos_load_send_and_export() {
    let transcript = recorded("success");
    for size in [(1, 1), (2, 2), (3, 2), (1, 300), (300, 1)] {
        let (w, h) = (size.0 as f32, size.1 as f32);
        let corners = [
            None,
            Some([[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]),
            Some(inset(size)),
            Some([[0.5, 0.5]; 4]),
        ];
        for corners in corners {
            load_send_export(&png(size.0, size.1), corners, &transcript);
        }
    }
}

#[test]
fn a_transcribed_page_relinked_to_a_smaller_copy_still_loads() {
    // A square photo, transcribed from inside its corners.
    let big = (2000, 2000);
    let corners = inset(big);
    let mut transcript = recorded("success");
    transcript.crop.corners = corners;

    // Down to the smallest photo that can be added now, and to a single pixel, as before.
    for (to, scale) in [((200, 200), 0.1), ((1, 1), 1.0 / 2000.0)] {
        let mut page = Page::new("big.png".into(), Arc::from(png(big.0, big.1)), meta(big));
        page.corners = Some(corners);
        page.transcript = Some(transcript.clone());
        let bytes = png(to.0, to.1);
        let file = Page::new("small.png".into(), Arc::from(bytes.clone()), meta(to));
        let shape = Shape::of(big, to);
        assert!(!shape.is_different(), "{to:?}");
        page.relink(&file, shape);

        let scaled = corners.map(|[x, y]| [x * scale, y * scale]);
        let near = |a: Quad, b: Quad| {
            a.iter()
                .flatten()
                .zip(b.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 1e-3)
        };
        assert!(near(page.corners.unwrap(), scaled), "{:?}", page.corners);
        let transcript = page.transcript.unwrap();
        assert!(near(transcript.crop.corners, scaled));
        load_send_export(&bytes, page.corners, &transcript);
    }
}

fn meta(size: (u32, u32)) -> PhotoMeta {
    PhotoMeta {
        size,
        exif: Default::default(),
    }
}
