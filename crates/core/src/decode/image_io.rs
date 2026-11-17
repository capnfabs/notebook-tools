//! HEIC through Apple's ImageIO, so the macOS binary links nothing outside the OS. See
//! `decisions/2026-11-01-heic-through-imageio.md`.

use image::metadata::Orientation;
use image::{DynamicImage, RgbaImage};
use objc2_core_foundation::{
    CFData, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGColorSpaceModel, CGContext, CGImage, CGImageAlphaInfo,
    kCGColorSpaceSRGB,
};
use objc2_image_io::{
    CGImageSource, kCGImagePropertyOrientation, kCGImagePropertyPixelHeight,
    kCGImagePropertyPixelWidth,
};

use super::{DecodeError, Decoder, is_heic, upright, upright_size};

/// HEIC, as iPhones take, through ImageIO.
pub(super) struct ImageIo;

impl Decoder for ImageIo {
    fn recognizes(&self, bytes: &[u8]) -> bool {
        is_heic(bytes)
    }

    fn decode(&self, bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
        let source = source(bytes)?;
        // SAFETY: no options; the index comes from the source itself.
        let (image, orientation) = unsafe {
            let index = source.primary_image_index();
            let image = source.image_at_index(index, None).ok_or_else(|| {
                DecodeError::Heic(format!("ImageIO status {}", source.status().0))
            })?;
            (image, orientation(&source, index))
        };
        // ImageIO leaves the pixels as stored; iPhones record which way is up in the HEIC's
        // `irot`/`imir`, which ImageIO reports as an EXIF orientation.
        Ok(upright(
            DynamicImage::ImageRgba8(pixels(&image)?),
            orientation,
        ))
    }

    fn size(&self, bytes: &[u8]) -> Result<(u32, u32), DecodeError> {
        let source = source(bytes)?;
        // SAFETY: no options; the index comes from the source itself, and ImageIO's property
        // dictionaries have CFString keys.
        let (size, orientation) = unsafe {
            let index = source.primary_image_index();
            let properties = source.properties_at_index(index, None).ok_or_else(|| {
                DecodeError::Heic(format!("ImageIO status {}", source.status().0))
            })?;
            let properties = properties.cast_unchecked::<CFString, CFType>();
            let number = |key: &CFString| {
                let value = properties.get(key)?;
                u32::try_from(value.downcast_ref::<CFNumber>()?.as_i64()?).ok()
            };
            let width = number(kCGImagePropertyPixelWidth);
            let height = number(kCGImagePropertyPixelHeight);
            (width.zip(height), orientation(&source, index))
        };
        let size = size.ok_or_else(|| DecodeError::Heic("no pixel size".into()))?;
        Ok(upright_size(size, orientation))
    }
}

/// ImageIO's reader for `bytes`.
fn source(bytes: &[u8]) -> Result<CFRetained<CGImageSource>, DecodeError> {
    let data = CFData::from_bytes(bytes);
    // SAFETY: no options.
    unsafe { CGImageSource::with_data(&data, None) }
        .ok_or_else(|| DecodeError::Heic("ImageIO can't read it".into()))
}

/// The EXIF-style orientation of image `index` in `source`, or none if it doesn't say.
///
/// # Safety
/// `index` is an image in `source`.
unsafe fn orientation(source: &CGImageSource, index: usize) -> Orientation {
    // SAFETY: no options; ImageIO's property dictionaries have CFString keys.
    let exif = unsafe {
        let Some(properties) = source.properties_at_index(index, None) else {
            return Orientation::NoTransforms;
        };
        properties
            .cast_unchecked::<CFString, CFType>()
            .get(kCGImagePropertyOrientation)
            .and_then(|value| value.downcast_ref::<CFNumber>()?.as_i32())
    };
    exif.and_then(|o| u8::try_from(o).ok())
        .and_then(Orientation::from_exif)
        .unwrap_or(Orientation::NoTransforms)
}

/// `image`'s pixels as stored: its own colour space's values, not converted to sRGB. Images that
/// aren't RGB (grey, say) are converted to sRGB. Alpha is dropped, as photos have none.
fn pixels(image: &CGImage) -> Result<RgbaImage, DecodeError> {
    let (w, h) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    let space = CGImage::color_space(Some(image))
        .filter(|space| CGColorSpace::model(Some(space)) == CGColorSpaceModel::RGB)
        .or_else(srgb)
        .ok_or_else(|| DecodeError::Heic("no colour space".into()))?;
    let row = w * 4;
    let mut buffer = vec![0u8; row * h];
    // SAFETY: `buffer` holds `h` rows of `row` bytes and outlives the context, which only
    // draws into it below.
    let context: CFRetained<CGContext> = unsafe {
        CGBitmapContextCreate(
            buffer.as_mut_ptr().cast(),
            w,
            h,
            8,
            row,
            Some(&space),
            CGImageAlphaInfo::NoneSkipLast.0,
        )
    }
    .ok_or_else(|| DecodeError::Heic(format!("no {w}x{h} bitmap")))?;
    let rect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: w as f64,
            height: h as f64,
        },
    };
    CGContext::draw_image(Some(&context), rect, Some(image));
    drop(context);
    for alpha in buffer.iter_mut().skip(3).step_by(4) {
        *alpha = u8::MAX;
    }
    let (w, h) = (w as u32, h as u32);
    RgbaImage::from_raw(w, h, buffer)
        .ok_or_else(|| DecodeError::Heic(format!("short {w}x{h} bitmap")))
}

fn srgb() -> Option<CFRetained<CGColorSpace>> {
    // SAFETY: a constant from CoreGraphics.
    CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
}
