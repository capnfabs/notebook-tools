//! Turns image files into upright pixels. Each format has a [`Decoder`]; [`decode`] picks the one
//! that recognizes the file. Nothing past here knows which format a page came from.
//!
//! See `decisions/2026-10-31-image-decoding.md`.

use std::fmt;
use std::io::Cursor;

use image::{DynamicImage, ImageDecoder as _, ImageFormat, ImageReader, RgbaImage};
use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

/// Decodes one image format.
trait Decoder: Sync {
    /// Whether `bytes` look like this decoder's format, from their first few bytes.
    fn recognizes(&self, bytes: &[u8]) -> bool;
    /// `bytes` decoded and turned upright.
    fn decode(&self, bytes: &[u8]) -> Result<RgbaImage, DecodeError>;
}

const DECODERS: &[&dyn Decoder] = &[&ImageCrate, &Heif];

#[derive(Debug)]
pub enum DecodeError {
    /// No decoder recognizes the file.
    Unrecognized,
    Image(image::ImageError),
    Heif(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unrecognized => write!(f, "not a JPEG, PNG or HEIC image"),
            Self::Image(err) => err.fmt(f),
            Self::Heif(err) => write!(f, "couldn't decode HEIC: {err}"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<image::ImageError> for DecodeError {
    fn from(err: image::ImageError) -> Self {
        Self::Image(err)
    }
}

/// Decodes a JPEG, PNG or HEIC, turned upright.
pub fn decode(bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
    DECODERS
        .iter()
        .find(|d| d.recognizes(bytes))
        .ok_or(DecodeError::Unrecognized)?
        .decode(bytes)
}

/// JPEG and PNG, through the `image` crate, applying the EXIF orientation.
struct ImageCrate;

impl Decoder for ImageCrate {
    fn recognizes(&self, bytes: &[u8]) -> bool {
        matches!(
            image::guess_format(bytes),
            Ok(ImageFormat::Jpeg | ImageFormat::Png)
        )
    }

    fn decode(&self, bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
        let mut decoder = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(image::ImageError::IoError)?
            .into_decoder()?;
        let orientation = decoder.orientation()?;
        let mut img = DynamicImage::from_decoder(decoder)?;
        img.apply_orientation(orientation);
        Ok(img.into_rgba8())
    }
}

/// HEIC, as iPhones take, through libheif.
struct Heif;

impl Decoder for Heif {
    fn recognizes(&self, bytes: &[u8]) -> bool {
        // An ISO box `ftyp` first, then the major brand.
        const BRANDS: [&[u8]; 4] = [b"heic", b"heix", b"heim", b"mif1"];
        bytes.get(4..8) == Some(b"ftyp") && bytes.get(8..12).is_some_and(|b| BRANDS.contains(&b))
    }

    fn decode(&self, bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
        let heif = |err: libheif_rs::HeifError| DecodeError::Heif(err.to_string());
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
            .ok_or_else(|| DecodeError::Heif("no RGBA plane".into()))?;
        let (w, h) = (plane.width, plane.height);
        let row = w as usize * 4;
        let pixels = (0..h as usize)
            .flat_map(|y| &plane.data[y * plane.stride..][..row])
            .copied()
            .collect();
        RgbaImage::from_raw(w, h, pixels)
            .ok_or_else(|| DecodeError::Heif(format!("short {w}x{h} RGBA plane")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_images::png;
    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageEncoder as _, Rgb, RgbImage, Rgba};

    /// A JPEG whose EXIF says "rotate 90° clockwise to display", as iPhones write for portrait shots.
    fn rotated_jpeg(w: u32, h: u32) -> Vec<u8> {
        #[rustfmt::skip]
        let exif = vec![
            b'I', b'I', 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, // little-endian TIFF header, IFD at 8
            0x01, 0x00,                                     // one entry
            0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, // Orientation, SHORT, count 1
            0x06, 0x00, 0x00, 0x00,                         // value 6 (Rotate90)
            0x00, 0x00, 0x00, 0x00,                         // no next IFD
        ];
        let img = RgbImage::from_pixel(w, h, Rgb([200, 100, 50]));
        let mut bytes = Vec::new();
        let mut encoder = JpegEncoder::new(&mut bytes);
        encoder.set_exif_metadata(exif).unwrap();
        encoder
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
            .unwrap();
        bytes
    }

    #[test]
    fn decodes_png() {
        let img = decode(&png(3, 2)).unwrap();
        assert_eq!(img.dimensions(), (3, 2));
        assert_eq!(img.get_pixel(2, 1), &Rgba([2, 1, 0, 255]));
    }

    #[test]
    fn applies_exif_orientation() {
        let img = decode(&rotated_jpeg(16, 8)).unwrap();
        assert_eq!(img.dimensions(), (8, 16));
    }

    #[test]
    fn rejects_non_images() {
        assert!(matches!(
            decode(b"just some notes"),
            Err(DecodeError::Unrecognized)
        ));
    }

    #[test]
    fn each_decoder_recognizes_only_its_formats() {
        let heic = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic";
        assert!(ImageCrate.recognizes(&png(1, 1)));
        assert!(ImageCrate.recognizes(&rotated_jpeg(1, 1)));
        assert!(!ImageCrate.recognizes(heic));
        assert!(Heif.recognizes(heic));
        assert!(!Heif.recognizes(&png(1, 1)));
        assert!(!Heif.recognizes(b"\0\0\0\x18ftypisom"), "an MP4");
        assert!(!Heif.recognizes(b"ftyp"));
    }

    #[test]
    fn broken_heic_is_an_error() {
        assert!(matches!(
            decode(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic"),
            Err(DecodeError::Heif(_))
        ));
    }
}
