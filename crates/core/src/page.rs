//! Opened image files and the ordered set of them.

use std::fmt;
use std::io::Cursor;
use std::sync::Arc;

use image::{DynamicImage, ImageDecoder as _, ImageReader, RgbaImage, imageops};

use crate::transcript::Transcript;

/// Longest side of a [`Page::thumbnail`], in pixels. Big enough for the viewer's thumbnails on a
/// 2x display.
pub const THUMBNAIL_SIDE_PX: u32 = 256;

/// An image file the user opened.
///
/// Holds the encoded bytes rather than decoded pixels: a 12 MP photo decodes to ~48 MB, too much
/// to keep around for every page. Call [`decode`] on `bytes` when the full image is needed. They're
/// shared, so handing them to a transcription thread doesn't copy them.
pub struct Page {
    pub name: String,
    pub bytes: Arc<[u8]>,
    pub thumbnail: RgbaImage,
    /// Claude's transcription, once there is one.
    pub transcript: Option<Transcript>,
}

impl Page {
    pub fn from_file(name: String, bytes: Vec<u8>) -> Result<Self, DecodeError> {
        let thumbnail = shrink_to_fit(decode(&bytes)?, THUMBNAIL_SIDE_PX);
        Ok(Self {
            name,
            bytes: bytes.into(),
            thumbnail,
            transcript: None,
        })
    }
}

#[derive(Debug)]
pub struct DecodeError(image::ImageError);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for DecodeError {}

impl From<image::ImageError> for DecodeError {
    fn from(err: image::ImageError) -> Self {
        Self(err)
    }
}

/// Decodes a JPEG or PNG, applying its EXIF orientation so the result is upright.
pub fn decode(bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(image::ImageError::IoError)?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    Ok(img.into_rgba8())
}

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

/// Pages with no duplicate names, in filename order until the user rearranges them.
#[derive(Default)]
pub struct Pages(Vec<Page>);

impl Pages {
    /// Inserts `page` in filename order, or at the end once the pages have been rearranged, and
    /// returns its index. Returns `None` if a page with that name is already present.
    pub fn add(&mut self, page: Page) -> Option<usize> {
        let index = if self.is_sorted() {
            self.0
                .binary_search_by(|p| p.name.as_str().cmp(&page.name))
                .err()?
        } else if self.0.iter().any(|p| p.name == page.name) {
            return None;
        } else {
            self.0.len()
        };
        self.0.insert(index, page);
        Some(index)
    }

    /// Whether the pages are in filename order.
    pub fn is_sorted(&self) -> bool {
        self.0.is_sorted_by(|a, b| a.name <= b.name)
    }

    /// Moves page `from` so it ends up at index `to`. Returns false if either is out of range.
    pub fn move_page(&mut self, from: usize, to: usize) -> bool {
        if from >= self.0.len() || to >= self.0.len() {
            return false;
        }
        let page = self.0.remove(from);
        self.0.insert(to, page);
        true
    }

    /// Where the page named `name` is.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.0.iter().position(|p| p.name == name)
    }

    /// The page named `name`.
    pub fn find(&self, name: &str) -> Option<&Page> {
        self.0.iter().find(|p| p.name == name)
    }

    pub fn sort_by_name(&mut self) {
        self.0.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn remove(&mut self, index: usize) -> Option<Page> {
        (index < self.0.len()).then(|| self.0.remove(index))
    }

    pub fn get(&self, index: usize) -> Option<&Page> {
        self.0.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Page> {
        self.0.get_mut(index)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Page> {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageEncoder as _, ImageFormat, Rgb, RgbImage, Rgba};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

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

    fn page(name: &str) -> Page {
        Page::from_file(name.to_owned(), png(2, 2)).unwrap()
    }

    fn names(pages: &Pages) -> Vec<&str> {
        pages.iter().map(|p| p.name.as_str()).collect()
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
        assert!(decode(b"just some notes").is_err());
        assert!(Page::from_file("notes.txt".into(), b"hi".to_vec()).is_err());
    }

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

    #[test]
    fn page_has_small_thumbnail() {
        let page = Page::from_file("big.png".into(), png(512, 256)).unwrap();
        assert_eq!(
            page.thumbnail.dimensions(),
            (THUMBNAIL_SIDE_PX, THUMBNAIL_SIDE_PX / 2)
        );
    }

    #[test]
    fn pages_stay_sorted_and_unique() {
        let mut pages = Pages::default();
        assert_eq!(pages.add(page("IMG_0002.png")), Some(0));
        assert_eq!(pages.add(page("IMG_0001.png")), Some(0));
        assert_eq!(pages.add(page("IMG_0003.png")), Some(2));
        assert_eq!(pages.add(page("IMG_0002.png")), None);
        assert_eq!(
            names(&pages),
            ["IMG_0001.png", "IMG_0002.png", "IMG_0003.png"]
        );
    }

    #[test]
    fn removes_pages() {
        let mut pages = Pages::default();
        pages.add(page("a.png"));
        pages.add(page("b.png"));
        assert_eq!(pages.remove(2).map(|p| p.name), None);
        assert_eq!(pages.remove(0).map(|p| p.name).as_deref(), Some("a.png"));
        assert_eq!(names(&pages), ["b.png"]);
        // The name can be added again.
        assert_eq!(pages.add(page("a.png")), Some(0));
    }

    fn pages(names: &[&str]) -> Pages {
        let mut pages = Pages::default();
        for name in names {
            pages.add(page(name));
        }
        pages
    }

    #[test]
    fn moves_pages() {
        let mut pages = pages(&["a.png", "b.png", "c.png"]);
        assert!(pages.move_page(0, 2));
        assert_eq!(names(&pages), ["b.png", "c.png", "a.png"]);
        assert!(pages.move_page(2, 0));
        assert_eq!(names(&pages), ["a.png", "b.png", "c.png"]);
        assert!(!pages.move_page(3, 0));
        assert!(!pages.move_page(0, 3));
        assert_eq!(names(&pages), ["a.png", "b.png", "c.png"]);
    }

    #[test]
    fn adds_to_the_end_once_rearranged() {
        let mut pages = pages(&["b.png", "d.png"]);
        assert!(pages.is_sorted());
        pages.move_page(1, 0);
        assert!(!pages.is_sorted());

        assert_eq!(pages.add(page("a.png")), Some(2));
        assert_eq!(pages.add(page("b.png")), None);
        assert_eq!(names(&pages), ["d.png", "b.png", "a.png"]);
    }

    #[test]
    fn finds_pages_by_name() {
        let mut pages = pages(&["a.png", "b.png"]);
        pages.move_page(1, 0);
        assert_eq!(pages.position("a.png"), Some(1));
        assert_eq!(pages.find("b.png").map(|p| p.name.as_str()), Some("b.png"));
        assert_eq!(pages.position("c.png"), None);
        assert!(pages.find("c.png").is_none());
    }

    #[test]
    fn sorts_by_name() {
        let mut pages = pages(&["a.png", "b.png", "c.png"]);
        pages.move_page(0, 2);
        pages.sort_by_name();
        assert!(pages.is_sorted());
        assert_eq!(names(&pages), ["a.png", "b.png", "c.png"]);
    }
}
