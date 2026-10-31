//! A page's photo decoded and cleaned once, kept small. See
//! `decisions/2026-10-28-pages-cleaned-on-load.md`.

use std::sync::Arc;

use image::buffer::ConvertBuffer as _;
use image::{RgbImage, RgbaImage};

use crate::clean::{CleanParams, CleanReport, clean};
use crate::decode::{DecodeError, decode};
use crate::geom::Quad;
use crate::imaging::{jpeg, shrunk_copy};

/// Longest side of the thumbnails in [`Loaded`], in pixels. Big enough for the viewer's thumbnails
/// on a 2x display.
pub const THUMBNAIL_SIDE_PX: u32 = 256;

/// Quality of the JPEGs in [`Loaded`]. The cleaned one goes into PDFs as it is.
const PAGE_JPEG_QUALITY: u8 = 85;

/// A page's file decoded and cleaned once, kept as JPEGs no longer than
/// [`CleanParams::max_output_side`] so the viewer and the PDF never decode the full photo.
pub struct Loaded {
    /// The photo as it was, as a JPEG.
    pub photo_jpeg: Arc<[u8]>,
    /// The photo [`clean`]ed, as a JPEG.
    pub cleaned_jpeg: Arc<[u8]>,
    pub photo_thumbnail: RgbaImage,
    pub cleaned_thumbnail: RgbaImage,
    /// What cleaning found. Its `output_size` is `cleaned_jpeg`'s size.
    pub report: CleanReport,
    /// The user's corners it was cleaned with, if any. Once they differ from the page's, it's out
    /// of date.
    pub corners: Option<Quad>,
}

impl Loaded {
    /// Decodes and cleans a page's file, cropping to `corners` if given. Takes a while for a big
    /// photo.
    pub fn new(
        bytes: &[u8],
        params: &CleanParams,
        corners: Option<Quad>,
    ) -> Result<Self, DecodeError> {
        let photo = decode(bytes)?;
        let shrunk = shrunk_copy(&photo, params.max_output_side);
        let cleaned = clean(&photo, params, corners);
        let to_jpeg = |img: &RgbaImage| -> Arc<[u8]> {
            let rgb: RgbImage = img.convert();
            jpeg(&rgb, PAGE_JPEG_QUALITY).into()
        };
        Ok(Self {
            photo_thumbnail: shrunk_copy(&shrunk, THUMBNAIL_SIDE_PX),
            cleaned_thumbnail: shrunk_copy(&cleaned.image, THUMBNAIL_SIDE_PX),
            photo_jpeg: to_jpeg(&shrunk),
            cleaned_jpeg: to_jpeg(&cleaned.image),
            report: cleaned.report,
            corners,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_images::page_on_table;

    #[test]
    fn loading_cleans_and_shrinks_the_page() {
        let params = CleanParams {
            max_output_side: 400,
            ..CleanParams::default()
        };
        let loaded = Loaded::new(&page_on_table(1000, 500), &params, None).unwrap();
        assert!(
            loaded.report.corners.is_some(),
            "{:?}",
            loaded.report.detected
        );

        let photo = decode(&loaded.photo_jpeg).unwrap();
        assert_eq!(photo.dimensions(), (400, 200));
        assert_eq!(
            loaded.photo_thumbnail.dimensions(),
            (THUMBNAIL_SIDE_PX, THUMBNAIL_SIDE_PX / 2)
        );

        let cleaned = decode(&loaded.cleaned_jpeg).unwrap();
        assert_eq!(cleaned.dimensions(), loaded.report.output_size);
        let (w, h) = cleaned.dimensions();
        // The page is 600x375, so cropped to it the image is taller than the photo's 2:1.
        assert_eq!(w, 400);
        assert!((240..=260).contains(&h), "cropped to the page: {w}x{h}");
        let (tw, th) = loaded.cleaned_thumbnail.dimensions();
        assert_eq!(tw.max(th), THUMBNAIL_SIDE_PX.min(w.max(h)));
    }

    #[test]
    fn loading_crops_to_the_users_corners() {
        let corners = [[0.0, 0.0], [500.0, 0.0], [500.0, 500.0], [0.0, 500.0]];
        let loaded = Loaded::new(
            &page_on_table(1000, 500),
            &CleanParams::default(),
            Some(corners),
        );
        let loaded = loaded.unwrap();
        assert_eq!(loaded.corners, Some(corners));
        assert_eq!(loaded.report.corners, Some(corners));
        assert!(
            loaded.report.detected.is_ok(),
            "{:?}",
            loaded.report.detected
        );
        assert_eq!(loaded.report.output_size, (500, 500));
        assert_eq!(
            decode(&loaded.photo_jpeg).unwrap().dimensions(),
            (1000, 500)
        );
    }

    #[test]
    fn loading_rejects_non_images() {
        assert!(Loaded::new(b"hi", &CleanParams::default(), None).is_err());
    }
}
