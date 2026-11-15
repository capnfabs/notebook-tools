//! HEIC through libheif, off macOS. See `decisions/2026-11-01-heic-through-imageio.md`.

use image::RgbaImage;
use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

use super::{DecodeError, Decoder, is_heic};

/// HEIC, as iPhones take, through libheif.
pub(super) struct Heif;

impl Decoder for Heif {
    fn recognizes(&self, bytes: &[u8]) -> bool {
        is_heic(bytes)
    }

    fn decode(&self, bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
        let heif = |err: libheif_rs::HeifError| DecodeError::Heic(err.to_string());
        let context = HeifContext::read_from_bytes(bytes).map_err(heif)?;
        let handle = context.primary_image_handle().map_err(heif)?;
        // Applies the file's rotation and mirroring, which is how iPhones record orientation in
        // HEIC; its EXIF orientation is only informational, so it isn't applied again.
        let image = LibHeif::new()
            .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
            .map_err(heif)?;
        let plane = image
            .planes()
            .interleaved
            .ok_or_else(|| DecodeError::Heic("no RGBA plane".into()))?;
        let (w, h) = (plane.width, plane.height);
        let row = w as usize * 4;
        let pixels = (0..h as usize)
            .flat_map(|y| &plane.data[y * plane.stride..][..row])
            .copied()
            .collect();
        RgbaImage::from_raw(w, h, pixels)
            .ok_or_else(|| DecodeError::Heic(format!("short {w}x{h} RGBA plane")))
    }
}
