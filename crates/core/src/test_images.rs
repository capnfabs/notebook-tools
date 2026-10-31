//! Made-up photos for unit tests.

use std::io::Cursor;

use image::{ImageFormat, Rgba, RgbaImage};

pub const BROWN: Rgba<u8> = Rgba([120, 80, 50, 255]);
pub const CREAM: Rgba<u8> = Rgba([235, 225, 200, 255]);

pub fn encode_png(img: &RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

/// A `w` x `h` PNG whose pixel at `(x, y)` is `[x, y, 0, 255]`.
pub fn png(w: u32, h: u32) -> Vec<u8> {
    encode_png(&RgbaImage::from_fn(w, h, |x, y| {
        Rgba([x as u8, y as u8, 0, 255])
    }))
}

/// A cream page on a brown table, square to the photo, which cleaning finds the corners of.
pub fn page_on_table(w: u32, h: u32) -> Vec<u8> {
    encode_png(&RgbaImage::from_fn(w, h, |x, y| {
        let inside = (w / 5..w * 4 / 5).contains(&x) && (h / 8..h * 7 / 8).contains(&y);
        if inside { CREAM } else { BROWN }
    }))
}
