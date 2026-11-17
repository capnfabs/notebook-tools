//! Giving a page whose photo is missing its photo again, from a file the user picked or dropped:
//! whether the file is the same photo ([`Tier`]), whether it's the same shape ([`Shape`]), and
//! scaling the page's corners and transcript onto a resized copy.
//!
//! A file of another shape is never relinked: corners and boxes can't be mapped onto a crop or a
//! turn, and a missing page still exports to Markdown from its saved transcript. No two pages
//! have the same photo. See `decisions/2026-11-17-relink-photos.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::geom::Quad;
use crate::page::{Page, PageId, Pages};

/// How sure it is that a file is a page's photo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// The same file: the same SHA-256.
    Hash,
    /// Another file, but every EXIF ID agrees ([`crate::photo_meta::PhotoIds::same_photo`]):
    /// re-encoded, resized or edited.
    Exif,
    /// Nothing says it's the same photo.
    Unmatched,
}

/// A file's photo's size beside a page's photo's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Same,
    /// The same shape at another size, `from` the page's `to` the file's: corners and the
    /// transcript's crop are scaled.
    Scaled {
        from: (u32, u32),
        to: (u32, u32),
    },
    /// Another aspect ratio, or turned: never relinked.
    Different {
        from: (u32, u32),
        to: (u32, u32),
    },
}

impl Shape {
    /// The file's photo's shape beside the page's, `from` its size `to` the file's.
    pub fn of(from: (u32, u32), to: (u32, u32)) -> Self {
        let ((w0, h0), (w1, h1)) = (from, to);
        if from == to {
            return Self::Same;
        }
        if w0 == 0 || h0 == 0 || w1 == 0 || h1 == 0 {
            return Self::Different { from, to };
        }
        // Each side within a pixel of the other scaled, for rounding.
        let near = |side: u32, scaled: f64| (f64::from(side) - scaled).abs() <= 1.0;
        let (w0, h0, w1f, h1f) = (f64::from(w0), f64::from(h0), f64::from(w1), f64::from(h1));
        if near(h1, h0 * w1f / w0) && near(w1, w0 * h1f / h0) {
            Self::Scaled { from, to }
        } else {
            Self::Different { from, to }
        }
    }

    /// How much to scale the page's corners across and down, if at all.
    pub fn scale(&self) -> Option<(f32, f32)> {
        match *self {
            Self::Scaled {
                from: (w0, h0),
                to: (w1, h1),
            } => Some((w1 as f32 / w0 as f32, h1 as f32 / h0 as f32)),
            _ => None,
        }
    }

    pub fn is_different(&self) -> bool {
        matches!(self, Self::Different { .. })
    }
}

/// How a file would relink a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub tier: Tier,
    /// Never [`Shape::Different`].
    pub shape: Shape,
}

/// Why a file can't relink a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The file is the photo of another page, named this.
    AlreadyAPage(String),
    /// The file's photo is another shape: `from` the page's size `to` the file's.
    DifferentShape { from: (u32, u32), to: (u32, u32) },
}

/// Whether `photo`, a file read as a page, is page `page`'s photo.
pub fn tier(page: &Page, photo: &Page) -> Tier {
    if page.sha256 == photo.sha256 {
        Tier::Hash
    } else if page.meta.exif.same_photo(&photo.meta.exif) {
        Tier::Exif
    } else {
        Tier::Unmatched
    }
}

/// Whether, and how, `photo`, a file read as a page, can relink page `id` among `pages`.
pub fn check(pages: &Pages, id: PageId, photo: &Page) -> Result<Plan, Refusal> {
    let page = pages
        .find(id)
        .expect("relinking a page that's among the pages");
    if let Some(other) = pages.with_photo(&photo.sha256).find(|p| p.id != id) {
        return Err(Refusal::AlreadyAPage(other.name.clone()));
    }
    let shape = Shape::of(page.meta.size, photo.meta.size);
    if let Shape::Different { from, to } = shape {
        return Err(Refusal::DifferentShape { from, to });
    }
    Ok(Plan {
        tier: tier(page, photo),
        shape,
    })
}

/// What a dropped file, not the photo of any page, has to do with the pages missing theirs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// No missing page looks like it.
    Unrelated,
    /// These missing pages look like it by EXIF and are its shape: ask which, if any, it is.
    Ask(Vec<PageId>),
    /// The missing pages that look like it by EXIF, named, are all another shape.
    OtherShape(Vec<String>),
}

/// What `photo`, a dropped file read as a page, has to do with the pages missing theirs.
pub fn dropped(pages: &Pages, photo: &Page) -> Dropped {
    let alike: Vec<&Page> = pages
        .iter()
        .filter(|p| p.missing_photo().is_some() && tier(p, photo) == Tier::Exif)
        .collect();
    let (same_shape, other_shape): (Vec<&Page>, Vec<&Page>) = alike
        .into_iter()
        .partition(|p| !Shape::of(p.meta.size, photo.meta.size).is_different());
    if !same_shape.is_empty() {
        Dropped::Ask(same_shape.iter().map(|p| p.id).collect())
    } else if !other_shape.is_empty() {
        Dropped::OtherShape(other_shape.iter().map(|p| p.name.clone()).collect())
    } else {
        Dropped::Unrelated
    }
}

/// Scales page's corners, and its transcript's crop, `x` across and `y` down, onto its photo
/// resized. The transcript's boxes are in the pixels of the image sent, so they stay.
pub(crate) fn scale_page(page: &mut Page, x: f32, y: f32) {
    let scale = |quad: &mut Quad| {
        for point in quad {
            point[0] *= x;
            point[1] *= y;
        }
    };
    if let Some(corners) = &mut page.corners {
        scale(corners);
    }
    if let Some(transcript) = &mut page.transcript {
        scale(&mut transcript.crop.corners);
        if let Some(source) = &mut transcript.source {
            scale(&mut Arc::make_mut(source).corners);
        }
    }
}

/// The folder to look for a photo that was at `path` in: its own, or the nearest one above it
/// that's still there.
pub fn nearest_existing_dir(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|dir| dir.is_dir())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clean::Crop;
    use crate::markdown_style::MarkdownStyle;
    use crate::page::Why;
    use crate::photo_meta::{PhotoIds, PhotoMeta};
    use crate::project::Photo;
    use crate::test_images::png;
    use crate::transcript::{Source, Transcript};

    fn taken(at: &str) -> PhotoIds {
        PhotoIds {
            date_time_original: Some(at.into()),
            ..PhotoIds::default()
        }
    }

    /// A page missing its photo, `size` with `exif`.
    fn missing(name: &str, size: (u32, u32), exif: PhotoIds) -> Page {
        let photo = Photo {
            path: PathBuf::from(format!("/gone/{name}")),
            sha256: format!("sha of {name}"),
            len: 1,
            size,
            exif,
        };
        Page::missing(name.into(), photo, Why::NotFound)
    }

    /// A file read as a page, with `bytes` and saying it's `size` with `exif`.
    fn file(bytes: Vec<u8>, size: (u32, u32), exif: PhotoIds) -> Page {
        Page::new("new.jpg".into(), bytes.into(), PhotoMeta { size, exif })
    }

    #[test]
    fn shapes() {
        assert_eq!(Shape::of((4032, 3024), (4032, 3024)), Shape::Same);
        let scaled = Shape::of((4032, 3024), (1024, 768));
        assert_eq!(
            scaled,
            Shape::Scaled {
                from: (4032, 3024),
                to: (1024, 768)
            }
        );
        let (x, y) = scaled.scale().unwrap();
        assert!((x - 1024.0 / 4032.0).abs() < 1e-6 && (y - 768.0 / 3024.0).abs() < 1e-6);
        // Rounded to the pixel.
        assert!(!Shape::of((4032, 3024), (1000, 750)).is_different());
        assert!(!Shape::of((4032, 3024), (1001, 751)).is_different());
        assert!(
            Shape::of((4032, 3024), (1000, 760)).is_different(),
            "cropped"
        );
        assert!(
            Shape::of((4032, 3024), (3024, 4032)).is_different(),
            "turned"
        );
        assert!(Shape::of((4032, 3024), (0, 0)).is_different());
        assert_eq!(Shape::Same.scale(), None);
    }

    #[test]
    fn the_same_file_matches_by_hash() {
        let bytes = png(2, 2);
        let mut page = missing("a.png", (2, 2), PhotoIds::default());
        page.sha256 = crate::cache::sha256_hex(&bytes);
        let photo = file(bytes, (2, 2), PhotoIds::default());
        assert_eq!(tier(&page, &photo), Tier::Hash);
    }

    #[test]
    fn another_file_matches_by_exif() {
        let at = taken("2026:05:01 12:34:56");
        let page = missing("a.jpg", (4, 3), at.clone());
        assert_eq!(tier(&page, &file(png(1, 1), (4, 3), at)), Tier::Exif);
        let later = taken("2026:05:01 12:34:57");
        assert_eq!(
            tier(&page, &file(png(1, 1), (4, 3), later)),
            Tier::Unmatched
        );
        let none = PhotoIds::default();
        assert_eq!(
            tier(
                &missing("b.jpg", (4, 3), none.clone()),
                &file(png(1, 1), (4, 3), none)
            ),
            Tier::Unmatched,
            "no EXIF on either"
        );
    }

    #[test]
    fn a_differing_photo_identifier_beats_an_equal_time() {
        let with_id = |id: &str| PhotoIds {
            photo_identifier: Some(id.into()),
            ..taken("2026:05:01 12:34:56")
        };
        let page = missing("a.heic", (4, 3), with_id("A"));
        assert_eq!(
            tier(&page, &file(png(1, 1), (4, 3), with_id("B"))),
            Tier::Unmatched
        );
        assert_eq!(
            tier(&page, &file(png(1, 1), (4, 3), with_id("A"))),
            Tier::Exif
        );
    }

    fn pages_of(pages: Vec<Page>) -> (Pages, Vec<PageId>) {
        let ids = pages.iter().map(|p| p.id).collect();
        (Pages::in_order(pages), ids)
    }

    #[test]
    fn checks_shape_and_other_pages() {
        let at = taken("2026:05:01 12:34:56");
        let other_bytes = png(3, 3);
        let other = file(other_bytes.clone(), (3, 3), PhotoIds::default());
        let (pages, ids) = pages_of(vec![missing("a.jpg", (400, 300), at.clone()), other]);
        let a = ids[0];

        let resized = file(png(1, 1), (200, 150), at.clone());
        assert_eq!(
            check(&pages, a, &resized),
            Ok(Plan {
                tier: Tier::Exif,
                shape: Shape::Scaled {
                    from: (400, 300),
                    to: (200, 150)
                }
            })
        );
        let unmatched = file(png(1, 2), (400, 300), PhotoIds::default());
        assert_eq!(
            check(&pages, a, &unmatched),
            Ok(Plan {
                tier: Tier::Unmatched,
                shape: Shape::Same
            })
        );
        let turned = file(png(2, 1), (300, 400), at);
        assert_eq!(
            check(&pages, a, &turned),
            Err(Refusal::DifferentShape {
                from: (400, 300),
                to: (300, 400)
            })
        );
        let duplicate = file(other_bytes, (400, 300), PhotoIds::default());
        assert_eq!(
            check(&pages, a, &duplicate),
            Err(Refusal::AlreadyAPage("new.jpg".into()))
        );
    }

    #[test]
    fn dropped_files_are_asked_about_when_exif_matches() {
        let at = taken("2026:05:01 12:34:56");
        let (pages, ids) = pages_of(vec![
            missing("a.jpg", (400, 300), at.clone()),
            missing("b.jpg", (400, 300), at.clone()),
            missing("c.jpg", (300, 400), at.clone()),
            missing("d.jpg", (400, 300), taken("2020:01:01 00:00:00")),
        ]);
        let resized = file(png(1, 1), (200, 150), at.clone());
        assert_eq!(
            dropped(&pages, &resized),
            Dropped::Ask(vec![ids[0], ids[1]])
        );
        let turned = file(png(1, 1), (150, 200), at.clone());
        assert_eq!(dropped(&pages, &turned), Dropped::Ask(vec![ids[2]]));
        let square = file(png(1, 1), (100, 100), at);
        assert_eq!(
            dropped(&pages, &square),
            Dropped::OtherShape(vec!["a.jpg".into(), "b.jpg".into(), "c.jpg".into()])
        );
        let unrelated = file(png(1, 1), (400, 300), PhotoIds::default());
        assert_eq!(dropped(&pages, &unrelated), Dropped::Unrelated);
    }

    #[test]
    fn only_missing_pages_are_asked_about() {
        let at = taken("2026:05:01 12:34:56");
        let found = file(png(2, 2), (400, 300), at.clone());
        let (pages, _) = pages_of(vec![found]);
        assert_eq!(
            dropped(&pages, &file(png(1, 1), (400, 300), at)),
            Dropped::Unrelated
        );
    }

    #[test]
    fn scaling_moves_corners_and_the_transcripts_crop() {
        let mut page = missing("a.jpg", (400, 300), PhotoIds::default());
        page.corners = Some([[40.0, 30.0], [360.0, 30.0], [360.0, 270.0], [40.0, 270.0]]);
        let crop = Crop {
            corners: [[40.0, 30.0], [360.0, 30.0], [360.0, 270.0], [40.0, 270.0]],
            size: (320, 240),
        };
        let mut transcript = Transcript::new(
            Vec::new(),
            Vec::new(),
            crop,
            MarkdownStyle::default(),
            "model".into(),
            Default::default(),
        );
        transcript.source = Some(Arc::new(Source {
            image_size: crop.size,
            corners: crop.corners,
            response: serde_json::Value::Null,
        }));
        page.transcript = Some(transcript);

        scale_page(&mut page, 0.5, 0.5);
        let half = [[20.0, 15.0], [180.0, 15.0], [180.0, 135.0], [20.0, 135.0]];
        assert_eq!(page.corners, Some(half));
        let transcript = page.transcript.unwrap();
        assert_eq!(transcript.crop.corners, half);
        assert_eq!(
            transcript.crop.size,
            (320, 240),
            "the image sent is the same"
        );
        assert_eq!(transcript.source.unwrap().corners, half);
    }

    #[test]
    fn looks_in_the_nearest_folder_still_there() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        assert_eq!(
            nearest_existing_dir(&photos.join("a.jpg")),
            Some(photos.clone())
        );
        assert_eq!(
            nearest_existing_dir(&photos.join("gone/deeper/a.jpg")),
            Some(photos)
        );
    }
}
