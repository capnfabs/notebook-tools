//! The pages the user opened, in order.

use std::path::PathBuf;
use std::sync::Arc;

use crate::cache::sha256_hex;
use crate::geom::Quad;
use crate::loaded::Loaded;
use crate::transcript::Transcript;

/// An image file the user opened.
///
/// Holds the file's bytes, which transcription works from, and once [`Loaded`], small copies for
/// everything else. A 12 MP photo decodes to ~48 MB, too much to keep around for every page. The
/// bytes are shared, so handing them to a transcription thread doesn't copy them. See
/// `decisions/2026-10-28-pages-cleaned-on-load.md`.
pub struct Page {
    pub name: String,
    /// Where the file is, if known: a project saves it. Pages dropped as bytes alone have none.
    pub path: Option<PathBuf>,
    pub bytes: Arc<[u8]>,
    /// [`sha256_hex`] of `bytes`, which a project checks the file against when it opens.
    pub sha256: String,
    /// Filled in off the UI thread, as loading decodes and cleans the whole photo.
    pub loaded: Option<Loaded>,
    /// Claude's transcription, once there is one.
    pub transcript: Option<Transcript>,
    /// Where the user put the page's corners, in the photo's pixels, if they did. Otherwise
    /// cleaning finds them. The transcript keeps the corners it was made with.
    pub corners: Option<Quad>,
}

impl Page {
    /// A page that isn't [`Loaded`] yet.
    pub fn new(name: String, bytes: Arc<[u8]>) -> Self {
        Self {
            name,
            path: None,
            sha256: sha256_hex(&bytes),
            bytes,
            loaded: None,
            transcript: None,
            corners: None,
        }
    }

    /// The page, read from the file at `path`.
    pub fn at(self, path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..self
        }
    }
}

/// Pages with no duplicate names, in filename order until the user rearranges them.
/// See `decisions/2026-10-24-page-order.md`.
#[derive(Default)]
pub struct Pages(Vec<Page>);

impl Pages {
    /// `pages` in the order given, as a project saved them. Later pages with a name already used
    /// are dropped.
    pub fn in_order(pages: Vec<Page>) -> Self {
        let mut kept: Vec<Page> = Vec::with_capacity(pages.len());
        for page in pages {
            if !kept.iter().any(|p| p.name == page.name) {
                kept.push(page);
            }
        }
        Self(kept)
    }

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

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Page> {
        self.0.iter_mut().find(|p| p.name == name)
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
    use crate::test_images::png;

    fn page(name: &str) -> Page {
        Page::new(name.to_owned(), png(2, 2).into())
    }

    fn names(pages: &Pages) -> Vec<&str> {
        pages.iter().map(|p| p.name.as_str()).collect()
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

        pages.find_mut("a.png").unwrap().corners = Some([[1.0; 2]; 4]);
        assert_eq!(pages.get(1).unwrap().corners, Some([[1.0; 2]; 4]));
        assert!(pages.find_mut("c.png").is_none());
    }

    #[test]
    fn keeps_the_order_given() {
        let pages = Pages::in_order(vec![page("b.png"), page("a.png"), page("b.png")]);
        assert_eq!(names(&pages), ["b.png", "a.png"]);
        assert!(!pages.is_sorted());
    }

    #[test]
    fn pages_know_their_hash() {
        assert_eq!(page("a.png").sha256, sha256_hex(&png(2, 2)));
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
