//! The viewer's textures: every page's thumbnails, and the selected page's full-size photo and
//! cleaned image, decoded off the UI thread. Only those two full-size images are kept: see
//! `decisions/2026-10-29-two-viewer-textures.md`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use eframe::egui;
use egui::{TextureHandle, TextureOptions};
use image::RgbaImage;
use notebook_core::decode::decode;
use notebook_core::imaging::shrink_to_fit;
use notebook_core::loaded::Loaded;
use notebook_core::page::{Page, PageId};

use crate::jobs::{Jobs, Waker};
use crate::timing::timed;

#[derive(Default)]
pub(super) struct Textures {
    /// One pair per loaded page.
    thumbs: HashMap<PageId, Thumbs>,
    /// The selected page's photo, then its cleaned image, at full size.
    full: [Option<FullSize>; 2],
    decodes: Decodes,
}

/// What the viewer shows of the selected page.
pub(super) enum Showing {
    Full(TextureHandle),
    /// Its thumbnail until the full image is decoded, or nothing while the page loads.
    Placeholder(Option<TextureHandle>),
    /// Its photo is missing, for this reason: it never loads.
    Missing(String),
}

impl Showing {
    pub(super) fn texture(&self) -> Option<&TextureHandle> {
        match self {
            Self::Full(texture) => Some(texture),
            Self::Placeholder(texture) => texture.as_ref(),
            Self::Missing(_) => None,
        }
    }
}

/// One of a page's full-size images: its page, whether it's the cleaned one, and its JPEG, which
/// changes when the page is cleaned again.
#[derive(Clone)]
struct ImageKey {
    id: PageId,
    /// For the logs.
    name: String,
    cleaned: bool,
    jpeg: Arc<[u8]>,
}

impl PartialEq for ImageKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.cleaned == other.cleaned && Arc::ptr_eq(&self.jpeg, &other.jpeg)
    }
}

/// A full-size image on screen, and which it is.
struct FullSize {
    key: ImageKey,
    texture: TextureHandle,
}

/// A page's thumbnails, as loaded.
struct Thumbs {
    photo: TextureHandle,
    cleaned: TextureHandle,
}

impl Thumbs {
    fn get(&self, cleaned: bool) -> &TextureHandle {
        if cleaned { &self.cleaned } else { &self.photo }
    }
}

/// Decodes the selected page's images for the viewer, on a thread of its own so they don't wait
/// behind pages that are loading.
struct Decodes {
    jobs: Jobs<ImageKey, Result<egui::ColorImage, String>>,
    /// The photo and the cleaned image the viewer is waiting for. Any other that comes back is
    /// stale.
    wanted: [Option<ImageKey>; 2],
}

impl Default for Decodes {
    fn default() -> Self {
        Self {
            jobs: Jobs::on_own_threads(),
            wanted: [None, None],
        }
    }
}

impl Textures {
    pub(super) fn set_waker(&mut self, waker: Waker) {
        self.decodes.jobs.set_waker(waker);
    }

    /// Whether any image is being decoded.
    pub(super) fn decoding(&self) -> bool {
        self.decodes.jobs.running() > 0
    }

    /// Makes textures of `loaded`'s thumbnails for `page`.
    pub(super) fn add_thumbnails(&mut self, ctx: &egui::Context, page: &Page, loaded: &Loaded) {
        let texture = |suffix: &str, img: &RgbaImage| {
            let name = format!("{}{suffix}", page.name);
            ctx.load_texture(name, color_image(img), TextureOptions::LINEAR)
        };
        let thumbs = Thumbs {
            photo: texture("", &loaded.photo_thumbnail),
            cleaned: texture(" (cleaned)", &loaded.cleaned_thumbnail),
        };
        self.thumbs.insert(page.id, thumbs);
    }

    /// The thumbnail of the photo of page `id`, once it's loaded.
    pub(super) fn thumbnail(&self, id: PageId) -> Option<&TextureHandle> {
        self.thumbs.get(&id).map(|t| &t.photo)
    }

    /// Drops deleted page `id`'s textures.
    pub(super) fn forget(&mut self, id: PageId) {
        self.thumbs.remove(&id);
        for full in &mut self.full {
            if full.as_ref().is_some_and(|f| f.key.id == id) {
                *full = None;
            }
        }
    }

    /// `page`'s photo, or its `cleaned` image, at full size once it's decoded. Until then, the
    /// image it replaces or its thumbnail, while the full image is decoded off the UI thread. While
    /// the page is cleaned again with new corners, the cleaned image is a placeholder too.
    pub(super) fn get(&mut self, ctx: &egui::Context, page: &Page, cleaned: bool) -> Showing {
        if let Some(why) = page.missing_photo() {
            let path = page.path.as_deref().unwrap_or(Path::new(""));
            return Showing::Missing(crate::i18n::missing_photo(why, path));
        }
        let Some(loaded) = &page.loaded else {
            return Showing::Placeholder(None);
        };
        let jpeg = if cleaned {
            &loaded.cleaned_jpeg
        } else {
            &loaded.photo_jpeg
        };
        let key = ImageKey {
            id: page.id,
            name: page.name.clone(),
            cleaned,
            jpeg: jpeg.clone(),
        };
        let slot = usize::from(cleaned);
        let full = self.full[slot].as_ref().filter(|f| f.key.id == page.id);
        let recleaning = cleaned && loaded.corners != page.corners;
        if let Some(full) = full.filter(|f| f.key == key) {
            let texture = full.texture.clone();
            return if recleaning {
                Showing::Placeholder(Some(texture))
            } else {
                Showing::Full(texture)
            };
        }
        if self.decodes.wanted[slot].as_ref() != Some(&key) {
            let (name, jpeg) = (page.name.clone(), jpeg.clone());
            let max_side = ctx.input(|i| i.max_texture_side) as u32;
            self.decodes.jobs.spawn(key.clone(), move || {
                let img = timed(format_args!("{name}: decode"), || decode(&jpeg));
                let img = img.map_err(|e| e.to_string())?;
                Ok(color_image(&shrink_to_fit(img, max_side)))
            });
            self.decodes.wanted[slot] = Some(key);
        }
        let thumb = self.thumbs.get(&page.id).map(|t| t.get(cleaned));
        let replaced = full.map(|f| &f.texture);
        Showing::Placeholder(replaced.or(thumb).cloned())
    }

    /// Puts the images the viewer is waiting for on screen once they're decoded. Others are
    /// stale.
    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        while let Some((key, image)) = self.decodes.jobs.next() {
            let slot = usize::from(key.cleaned);
            if self.decodes.wanted[slot].as_ref() != Some(&key) {
                continue;
            }
            self.decodes.wanted[slot] = None;
            let name = &key.name;
            let texture = match image {
                Ok(image) => timed(format_args!("{name}: load texture"), || {
                    ctx.load_texture(name, image, TextureOptions::LINEAR)
                }),
                // It decoded when the page loaded, so this shouldn't happen.
                Err(err) => {
                    log::error!("Couldn't decode {name}: {err}");
                    let Some(thumbs) = self.thumbs.get(&key.id) else {
                        continue;
                    };
                    thumbs.get(key.cleaned).clone()
                }
            };
            self.full[slot] = Some(FullSize { key, texture });
        }
    }
}

fn color_image(img: &RgbaImage) -> egui::ColorImage {
    let size = [img.width() as usize, img.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw())
}
