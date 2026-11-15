//! Turns image files into upright pixels. Each format has a [`Decoder`]; [`decode`] picks the one
//! that recognizes the file. Nothing past here knows which format a page came from.
//!
//! See `decisions/2026-10-31-image-decoding.md`; HEIC goes through ImageIO on macOS and libheif
//! elsewhere, `decisions/2026-11-01-heic-through-imageio.md`.

use std::fmt;
use std::io::Cursor;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder as _, ImageFormat, ImageReader, RgbaImage};

#[cfg(target_os = "macos")]
mod image_io;
#[cfg(not(target_os = "macos"))]
mod libheif;

/// The extensions of the photos the app opens, lowercase: the open dialog's filter, and the photo
/// document type in the macOS bundle's `crates/app/macos/Info.plist`. Decoding goes by the bytes,
/// not the extension.
pub const EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "heic", "heif"];

/// Decodes one image format.
trait Decoder: Sync {
    /// Whether `bytes` look like this decoder's format, from their first few bytes.
    fn recognizes(&self, bytes: &[u8]) -> bool;
    /// `bytes` decoded and turned upright.
    fn decode(&self, bytes: &[u8]) -> Result<RgbaImage, DecodeError>;
}

/// This platform's HEIC decoder.
#[cfg(target_os = "macos")]
const HEIC: &dyn Decoder = &image_io::ImageIo;
#[cfg(not(target_os = "macos"))]
const HEIC: &dyn Decoder = &libheif::Heif;

const DECODERS: &[&dyn Decoder] = &[&ImageCrate, HEIC];

#[derive(Debug)]
pub enum DecodeError {
    /// No decoder recognizes the file.
    Unrecognized,
    Image(image::ImageError),
    /// The HEIC decoder failed, saying why.
    Heic(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unrecognized => write!(f, "not a JPEG, PNG or HEIC image"),
            Self::Image(err) => err.fmt(f),
            Self::Heic(err) => write!(f, "couldn't decode HEIC: {err}"),
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

/// Whether `bytes` start like a HEIC: an ISO box `ftyp` first, then the major brand.
fn is_heic(bytes: &[u8]) -> bool {
    const BRANDS: [&[u8]; 4] = [b"heic", b"heix", b"heim", b"mif1"];
    bytes.get(4..8) == Some(b"ftyp") && bytes.get(8..12).is_some_and(|b| BRANDS.contains(&b))
}

/// `img` turned upright, given the EXIF-style `orientation` it was stored with.
fn upright(mut img: DynamicImage, orientation: Orientation) -> RgbaImage {
    img.apply_orientation(orientation);
    img.into_rgba8()
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
        Ok(upright(DynamicImage::from_decoder(decoder)?, orientation))
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

    const HEIC_HEADER: &[u8] = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic";

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
    fn upright_rotates_clockwise() {
        // The top-left pixel ends up top-right after a quarter turn clockwise.
        let mut img = RgbaImage::new(3, 2);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        let turned = upright(DynamicImage::ImageRgba8(img), Orientation::Rotate90);
        assert_eq!(turned.dimensions(), (2, 3));
        assert_eq!(turned.get_pixel(1, 0), &Rgba([255, 0, 0, 255]));
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
        assert!(ImageCrate.recognizes(&png(1, 1)));
        assert!(ImageCrate.recognizes(&rotated_jpeg(1, 1)));
        assert!(!ImageCrate.recognizes(HEIC_HEADER));
        assert!(HEIC.recognizes(HEIC_HEADER));
        assert!(!HEIC.recognizes(&png(1, 1)));
        assert!(!HEIC.recognizes(b"\0\0\0\x18ftypisom"), "an MP4");
        assert!(!HEIC.recognizes(b"ftyp"));
    }

    #[test]
    fn broken_heic_is_an_error() {
        assert!(matches!(decode(HEIC_HEADER), Err(DecodeError::Heic(_))));
    }
}
