//! The pages the user opened, in order. A page whose photo wasn't found when its project opened
//! is still a page, without its photo: see `decisions/2026-11-15-missing-pages-shown.md`. Pages
//! are told apart by their [`PageId`], not their names, which can repeat: see
//! `decisions/2026-11-15-page-ids.md`.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cache::sha256_hex;
use crate::geom::Quad;
use crate::loaded::Loaded;
use crate::photo_meta::PhotoMeta;
use crate::project::Photo;
use crate::relink::Shape;
use crate::transcript::Transcript;

/// An image file the user opened.
///
/// Holds the file's bytes, which transcription works from, and once [`Loaded`], small copies for
/// everything else. A 12 MP photo decodes to ~48 MB, too much to keep around for every page. The
/// bytes are shared, so handing them to a transcription thread doesn't copy them. See
/// `decisions/2026-10-28-pages-cleaned-on-load.md`.
pub struct Page {
    /// Tells the page from every other while the app runs. Not saved.
    pub id: PageId,
    /// The photo's file name. Pages' names can repeat.
    pub name: String,
    /// Where the file is, if known: a project saves it. Pages dropped as bytes alone have none.
    pub path: Option<PathBuf>,
    /// The file's bytes, or why they couldn't be read when the page's project opened.
    pub bytes: Result<Arc<[u8]>, Why>,
    /// [`sha256_hex`] of the file, which a project checks it against when it opens.
    pub sha256: String,
    /// The file's size in bytes.
    pub len: u64,
    /// The photo's size and EXIF, which tell it again if it's moved: see `crate::relink`.
    pub meta: PhotoMeta,
    /// Filled in off the UI thread, as loading decodes and cleans the whole photo.
    pub loaded: Option<Loaded>,
    /// Claude's transcription, once there is one.
    pub transcript: Option<Transcript>,
    /// Where the user put the page's corners, in the photo's pixels, if they did. Otherwise
    /// cleaning finds them. The transcript keeps the corners it was made with.
    pub corners: Option<Quad>,
}

impl Page {
    /// A page that isn't [`Loaded`] yet, of a photo that says `meta` about itself
    /// ([`crate::photo_meta::read`]).
    pub fn new(name: String, bytes: Arc<[u8]>, meta: PhotoMeta) -> Self {
        Self {
            id: PageId::next(),
            name,
            path: None,
            sha256: sha256_hex(&bytes),
            len: bytes.len() as u64,
            meta,
            bytes: Ok(bytes),
            loaded: None,
            transcript: None,
            corners: None,
        }
    }

    /// A page whose `photo` couldn't be read, and `why`. It never loads, and can't be
    /// transcribed.
    pub fn missing(name: String, photo: Photo, why: Why) -> Self {
        Self {
            id: PageId::next(),
            name,
            path: Some(photo.path),
            bytes: Err(why),
            sha256: photo.sha256,
            len: photo.len,
            meta: PhotoMeta {
                size: photo.size,
                exif: photo.exif,
            },
            loaded: None,
            transcript: None,
            corners: None,
        }
    }

    /// The photo's bytes, unless it's missing.
    pub fn bytes(&self) -> Option<&Arc<[u8]>> {
        self.bytes.as_ref().ok()
    }

    /// Why the photo is missing, if it is.
    pub fn missing_photo(&self) -> Option<&Why> {
        self.bytes.as_ref().err()
    }

    /// The page, read from the file at `path`.
    pub fn at(self, path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..self
        }
    }

    /// Gives a page missing its photo `photo`'s, whose [`Shape`] beside the old one is `shape`.
    /// The page takes the file's name, path (if known), hash, size and EXIF; its ID, corners and
    /// transcript stay, scaled to a resized photo. See `decisions/2026-11-17-relink-photos.md`.
    pub fn relink(&mut self, photo: &Page, shape: Shape) {
        let Some(bytes) = photo.bytes() else {
            return;
        };
        if let Some((x, y)) = shape.scale() {
            crate::relink::scale_page(self, x, y);
        }
        self.name = photo.name.clone();
        if photo.path.is_some() {
            self.path = photo.path.clone();
        }
        self.bytes = Ok(bytes.clone());
        self.sha256 = photo.sha256.clone();
        self.len = photo.len;
        self.meta = photo.meta.clone();
    }
}

/// Tells pages apart while the app runs, whatever their names. Never reused, even by another
/// project, so work still running for a page that's gone can't land on another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageId(u64);

impl PageId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for PageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Why a page's photo wasn't found when its project opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// There's no file at its path.
    NotFound,
    /// The file at its path isn't the photo the project was saved with.
    Changed,
    /// The file couldn't be read.
    Unreadable(String),
}

/// Pages in filename order until the user rearranges them. Pages with the same name keep their
/// order among themselves. See `decisions/2026-10-24-page-order.md`.
#[derive(Default)]
pub struct Pages(Vec<Page>);

impl Pages {
    /// `pages` in the order given, as a project saved them.
    pub fn in_order(pages: Vec<Page>) -> Self {
        Self(pages)
    }

    /// Inserts `page` in filename order, after any with the same name, or at the end once the
    /// pages have been rearranged, and returns its index.
    pub fn add(&mut self, page: Page) -> usize {
        let index = if self.is_sorted() {
            self.0.partition_point(|p| p.name <= page.name)
        } else {
            self.0.len()
        };
        self.0.insert(index, page);
        index
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

    /// Where page `id` is.
    pub fn position(&self, id: PageId) -> Option<usize> {
        self.0.iter().position(|p| p.id == id)
    }

    /// Page `id`.
    pub fn find(&self, id: PageId) -> Option<&Page> {
        self.0.iter().find(|p| p.id == id)
    }

    pub fn find_mut(&mut self, id: PageId) -> Option<&mut Page> {
        self.0.iter_mut().find(|p| p.id == id)
    }

    /// The pages whose photo is the file with this [`sha256_hex`], found or missing.
    pub fn with_photo<'a>(&'a self, sha256: &'a str) -> impl Iterator<Item = &'a Page> {
        self.0.iter().filter(move |p| p.sha256 == sha256)
    }

    /// Keeps the order of pages with the same name.
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
    use crate::test_images::png;

    fn page(name: &str) -> Page {
        photo_page(name, png(2, 2))
    }

    fn photo_page(name: &str, bytes: Vec<u8>) -> Page {
        let meta = crate::photo_meta::read(&bytes).unwrap();
        Page::new(name.to_owned(), bytes.into(), meta)
    }

    fn photo(path: &str, sha256: &str, len: u64) -> Photo {
        Photo {
            path: PathBuf::from(path),
            sha256: sha256.into(),
            len,
            size: (2, 2),
            exif: Default::default(),
        }
    }

    fn names(pages: &Pages) -> Vec<&str> {
        pages.iter().map(|p| p.name.as_str()).collect()
    }

    fn ids(pages: &Pages) -> Vec<PageId> {
        pages.iter().map(|p| p.id).collect()
    }

    #[test]
    fn pages_stay_sorted() {
        let mut pages = Pages::default();
        assert_eq!(pages.add(page("IMG_0002.png")), 0);
        assert_eq!(pages.add(page("IMG_0001.png")), 0);
        assert_eq!(pages.add(page("IMG_0003.png")), 2);
        assert_eq!(
            names(&pages),
            ["IMG_0001.png", "IMG_0002.png", "IMG_0003.png"]
        );
    }

    #[test]
    fn names_can_repeat() {
        let mut pages = pages(&["a.png", "b.png", "c.png"]);
        let first_b = pages.get(1).unwrap().id;
        let second_b = page("b.png");
        let second_b_id = second_b.id;
        assert_ne!(first_b, second_b_id);
        // After the page already called that.
        assert_eq!(pages.add(second_b), 2);
        assert_eq!(names(&pages), ["a.png", "b.png", "b.png", "c.png"]);
        assert_eq!(pages.position(first_b), Some(1));
        assert_eq!(pages.position(second_b_id), Some(2));
        assert!(pages.is_sorted());
    }

    #[test]
    fn every_page_has_its_own_id() {
        let pages = pages(&["a.png", "a.png", "b.png"]);
        let mut ids = ids(&pages);
        ids.dedup();
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn removes_pages() {
        let mut pages = Pages::default();
        pages.add(page("a.png"));
        pages.add(page("b.png"));
        assert_eq!(pages.remove(2).map(|p| p.name), None);
        assert_eq!(pages.remove(0).map(|p| p.name).as_deref(), Some("a.png"));
        assert_eq!(names(&pages), ["b.png"]);
        assert_eq!(pages.add(page("a.png")), 0);
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

        assert_eq!(pages.add(page("a.png")), 2);
        assert_eq!(pages.add(page("b.png")), 3);
        assert_eq!(names(&pages), ["d.png", "b.png", "a.png", "b.png"]);
    }

    #[test]
    fn finds_pages_by_id() {
        let mut pages = pages(&["a.png", "b.png"]);
        let (a, b) = (pages.get(0).unwrap().id, pages.get(1).unwrap().id);
        pages.move_page(1, 0);
        assert_eq!(pages.position(a), Some(1));
        assert_eq!(pages.find(b).map(|p| p.name.as_str()), Some("b.png"));

        pages.find_mut(a).unwrap().corners = Some([[1.0; 2]; 4]);
        assert_eq!(pages.get(1).unwrap().corners, Some([[1.0; 2]; 4]));

        let gone = pages.remove(0).unwrap().id;
        assert_eq!(pages.position(gone), None);
        assert!(pages.find(gone).is_none());
        assert!(pages.find_mut(gone).is_none());
    }

    #[test]
    fn finds_pages_by_photo() {
        let mut pages = pages(&["a.png"]);
        pages.add(photo_page("b.png", png(3, 3)));
        let sha256 = sha256_hex(&png(2, 2));
        let found: Vec<_> = pages.with_photo(&sha256).map(|p| p.name.as_str()).collect();
        assert_eq!(found, ["a.png"]);
        assert_eq!(pages.with_photo("nope").count(), 0);
    }

    #[test]
    fn keeps_the_order_given() {
        let pages = Pages::in_order(vec![page("b.png"), page("a.png"), page("b.png")]);
        assert_eq!(names(&pages), ["b.png", "a.png", "b.png"]);
        assert!(!pages.is_sorted());
    }

    #[test]
    fn pages_know_their_hash_and_size() {
        let page = page("a.png");
        assert_eq!(page.sha256, sha256_hex(&png(2, 2)));
        assert_eq!(page.len, png(2, 2).len() as u64);
        assert!(page.missing_photo().is_none());
    }

    #[test]
    fn missing_pages_have_no_photo() {
        let page = Page::missing(
            "a.png".into(),
            photo("/photos/a.png", "ab", 3),
            Why::NotFound,
        );
        assert!(page.bytes().is_none());
        assert_eq!(page.missing_photo(), Some(&Why::NotFound));
        assert_eq!(
            (page.path, page.sha256, page.len, page.meta.size),
            (Some(PathBuf::from("/photos/a.png")), "ab".into(), 3, (2, 2))
        );
    }

    #[test]
    fn found_photos_fill_in_missing_pages() {
        let bytes = png(2, 2);
        let mut page = Page::missing(
            "b.png".into(),
            photo("/a/b.png", &sha256_hex(&bytes), bytes.len() as u64),
            Why::NotFound,
        );
        page.corners = Some([[1.0; 2]; 4]);
        let id = page.id;

        let new = PathBuf::from("/c/d.png");
        let found = photo_page("d.png", bytes).at(new.clone());
        page.relink(&found, Shape::Same);
        assert_eq!(page.id, id);
        assert_eq!(page.name, "d.png");
        assert_eq!(page.path, Some(new));
        assert_eq!(page.bytes(), found.bytes());
        assert_eq!(page.corners, Some([[1.0; 2]; 4]));
    }

    #[test]
    fn relinked_pages_take_the_new_photo() {
        let mut page = Page::missing("b.png".into(), photo("/a/b.png", "ab", 3), Why::Changed);
        page.corners = Some([[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]);
        let found = photo_page("c.png", png(4, 4));
        page.relink(
            &found,
            Shape::Scaled {
                from: (2, 2),
                to: (4, 4),
            },
        );
        assert_eq!(
            (page.sha256.as_str(), page.len),
            (found.sha256.as_str(), found.len)
        );
        assert_eq!(page.meta, found.meta);
        assert!(page.missing_photo().is_none());
        assert_eq!(
            page.corners,
            Some([[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]])
        );
    }

    #[test]
    fn sorts_by_name_keeping_ties_in_order() {
        let mut pages = pages(&["a.png", "b.png", "b.png", "c.png"]);
        let (first_b, second_b) = (pages.get(1).unwrap().id, pages.get(2).unwrap().id);
        pages.move_page(2, 1);
        pages.move_page(0, 3);
        assert_eq!(names(&pages), ["b.png", "b.png", "c.png", "a.png"]);
        pages.sort_by_name();
        assert_eq!(names(&pages), ["a.png", "b.png", "b.png", "c.png"]);
        assert_eq!(&ids(&pages)[1..3], &[second_b, first_b]);
    }
}
