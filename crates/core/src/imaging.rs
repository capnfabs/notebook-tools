//! Scaling images down and encoding JPEGs, shared by cleaning, loading and transcription.

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder as _, RgbImage, RgbaImage, imageops};

/// Scales `img` down so neither side exceeds `max_side`, keeping the aspect ratio.
/// Images that already fit are returned unchanged.
pub fn shrink_to_fit(img: RgbaImage, max_side: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    if w <= max_side && h <= max_side {
        return img;
    }
    shrunk_copy(&img, max_side)
}

/// [`shrink_to_fit`] for a borrowed image: a copy, scaled down only if it doesn't fit.
pub(crate) fn shrunk_copy(img: &RgbaImage, max_side: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    match fitted_size(w, h, max_side) {
        (fw, fh) if (fw, fh) == (w, h) => img.clone(),
        (fw, fh) => imageops::thumbnail(img, fw, fh),
    }
}

/// The longest side a `w` x `h` image can be scaled to, keeping the aspect ratio, and have at most
/// `max_pixels`. Never more than its longest side now.
pub(crate) fn max_side_within(w: u32, h: u32, max_pixels: u32) -> u32 {
    let pixels = u64::from(w) * u64::from(h);
    if pixels <= u64::from(max_pixels) {
        return w.max(h);
    }
    let scale = (f64::from(max_pixels) / pixels as f64).sqrt();
    (f64::from(w.max(h)) * scale) as u32
}

/// The size `w` x `h` scales to so neither side exceeds `max_side`, keeping the aspect ratio.
/// Never scales up.
pub(crate) fn fitted_size(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    if w <= max_side && h <= max_side {
        return (w, h);
    }
    let scale = f64::from(max_side) / f64::from(w.max(h));
    let scaled = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
    (scaled(w), scaled(h))
}

/// `rgb` as a JPEG at `quality` (1-100).
pub(crate) fn jpeg(rgb: &RgbImage, quality: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .expect("encoding a JPEG in memory can't fail");
    bytes
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[test]
    fn shrinks_keeping_aspect_ratio() {
        let img = RgbaImage::new(400, 100);
        assert_eq!(shrink_to_fit(img.clone(), 128).dimensions(), (128, 32));
        assert_eq!(shrink_to_fit(img, 1000).dimensions(), (400, 100));
    }

    #[test]
    fn copies_shrink_the_same_way() {
        let img = RgbaImage::from_fn(400, 100, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        assert_eq!(shrunk_copy(&img, 128), shrink_to_fit(img.clone(), 128));
        assert_eq!(shrunk_copy(&img, 1000), img);
    }

    #[test]
    fn max_side_within_a_pixel_budget() {
        assert_eq!(max_side_within(400, 100, 40_000), 400, "already fits");
        assert_eq!(max_side_within(400, 100, 10_000), 200);
        assert_eq!(max_side_within(100, 400, 10_000), 200);
    }
}
