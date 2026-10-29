//! Opened image files and the ordered set of them.

use std::sync::Arc;

use image::buffer::ConvertBuffer as _;
use image::{RgbImage, RgbaImage, imageops};

use crate::clean::{CleanParams, CleanReport, Quad, clean};
use crate::decode::{DecodeError, decode};
use crate::transcript::Transcript;

/// Longest side of the thumbnails in [`Loaded`], in pixels. Big enough for the viewer's thumbnails
/// on a 2x display.
pub const THUMBNAIL_SIDE_PX: u32 = 256;

/// Quality of the JPEGs in [`Loaded`]. The cleaned one goes into PDFs as it is.
pub const PAGE_JPEG_QUALITY: u8 = 85;

/// An image file the user opened.
///
/// Holds the file's bytes, which transcription works from, and once [`Loaded`], small copies for
/// everything else. A 12 MP photo decodes to ~48 MB, too much to keep around for every page. The
/// bytes are shared, so handing them to a transcription thread doesn't copy them. See
/// `decisions/2026-10-28-pages-cleaned-on-load.md`.
pub struct Page {
    pub name: String,
    pub bytes: Arc<[u8]>,
    /// Filled in off the UI thread, as loading decodes and cleans the whole photo.
    pub loaded: Option<Loaded>,
    /// Claude's transcription, once there is one.
    pub transcript: Option<Transcript>,
    /// Where the user put the page's corners, in the photo's pixels, if they did. Otherwise
    /// cleaning finds them.
    pub corners: Option<Quad>,
    /// [`Self::corners`] when the page was sent to be transcribed. When they differ, the
    /// transcript's boxes may not line up with the cleaned page.
    pub transcribed_with: Option<Quad>,
}

impl Page {
    /// A page that isn't [`Loaded`] yet.
    pub fn new(name: String, bytes: Arc<[u8]>) -> Self {
        Self {
            name,
            bytes,
            loaded: None,
            transcript: None,
            corners: None,
            transcribed_with: None,
        }
    }
}

/// A page's file decoded and cleaned once, kept as JPEGs no longer than
/// [`CleanParams::max_output_side`] so the viewer and the PDF never decode the full photo.
pub struct Loaded {
    /// The photo as it was.
    pub original: Arc<[u8]>,
    /// The photo [`clean`]ed.
    pub cleaned: Arc<[u8]>,
    pub thumbnail: RgbaImage,
    pub cleaned_thumbnail: RgbaImage,
    /// What cleaning found. Its `output_size` is `cleaned`'s size.
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
        let original = shrunk_copy(&photo, params.max_output_side);
        let cleaned = clean(&photo, params, corners);
        let jpeg = |img: &RgbaImage| -> Arc<[u8]> {
            let rgb: RgbImage = img.convert();
            crate::ocr::jpeg(&rgb, PAGE_JPEG_QUALITY).into()
        };
        Ok(Self {
            thumbnail: shrunk_copy(&original, THUMBNAIL_SIDE_PX),
            cleaned_thumbnail: shrunk_copy(&cleaned.image, THUMBNAIL_SIDE_PX),
            original: jpeg(&original),
            cleaned: jpeg(&cleaned.image),
            report: cleaned.report,
            corners,
        })
    }
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
    use std::io::Cursor;

    use super::*;
    use image::{ImageFormat, Rgba};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn page(name: &str) -> Page {
        Page::new(name.to_owned(), png(2, 2).into())
    }

    fn names(pages: &Pages) -> Vec<&str> {
        pages.iter().map(|p| p.name.as_str()).collect()
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

    /// A cream page on a brown table, which cleanup finds an outline for.
    fn page_photo(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, y| {
            let inside = (w / 5..w * 4 / 5).contains(&x) && (h / 8..h * 7 / 8).contains(&y);
            if inside {
                Rgba([235, 225, 200, 255])
            } else {
                Rgba([120, 80, 50, 255])
            }
        });
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn loading_cleans_and_shrinks_the_page() {
        let params = CleanParams {
            max_output_side: 400,
            ..CleanParams::default()
        };
        let loaded = Loaded::new(&page_photo(1000, 500), &params, None).unwrap();
        assert!(
            loaded.report.outline.is_some(),
            "{:?}",
            loaded.report.detected
        );

        let original = decode(&loaded.original).unwrap();
        assert_eq!(original.dimensions(), (400, 200));
        assert_eq!(
            loaded.thumbnail.dimensions(),
            (THUMBNAIL_SIDE_PX, THUMBNAIL_SIDE_PX / 2)
        );

        let cleaned = decode(&loaded.cleaned).unwrap();
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
            &page_photo(1000, 500),
            &CleanParams::default(),
            Some(corners),
        );
        let loaded = loaded.unwrap();
        assert_eq!(loaded.corners, Some(corners));
        assert_eq!(loaded.report.outline, Some(corners));
        assert!(
            loaded.report.detected.is_ok(),
            "{:?}",
            loaded.report.detected
        );
        assert_eq!(loaded.report.output_size, (500, 500));
        assert_eq!(decode(&loaded.original).unwrap().dimensions(), (1000, 500));
    }

    #[test]
    fn loading_rejects_non_images() {
        assert!(Loaded::new(b"hi", &CleanParams::default(), None).is_err());
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
