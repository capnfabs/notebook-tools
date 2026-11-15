//! Projects: the pages the user opened, in order, with their corners and transcripts, saved to a
//! `.notebook` file, gzipped JSON. Photos aren't copied in: each page names its file by a path relative to the
//! project, and its hash, so a moved or changed photo is noticed. Transcripts are saved whole,
//! corrections and Claude's answer included, so a project doesn't need the cache. A photo that
//! can't be found when the project opens leaves its page [missing](Page::missing) its photo, still
//! in its place, so that saving again loses nothing.
//!
//! See `decisions/2026-10-31-projects.md`, `decisions/2026-10-31-gzip-projects.md` and
//! `decisions/2026-11-15-missing-pages-shown.md`.

use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::cache::sha256_hex;
use crate::geom::Quad;
use crate::json_file;
use crate::page::{Page, Pages, Why};
use crate::transcript::Transcript;

/// Version of the project file. Bump it when the format changes in a way an older build would
/// misread, and keep reading the older versions. A test snapshots the format. It's the JSON's
/// version: the gzip around it isn't versioned.
pub const PROJECT_VERSION: u32 = 1;

/// A project file's extension.
pub const EXTENSION: &str = "notebook";

/// A page as a project saves it: everything about it but the photo itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPage {
    pub name: String,
    pub photo: Photo,
    #[serde(default)]
    pub corners: Option<Quad>,
    #[serde(default)]
    pub transcript: Option<Transcript>,
}

/// Which file a page's photo is. Its path is relative to the project in the file, and absolute in
/// memory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Photo {
    pub path: PathBuf,
    /// [`sha256_hex`] of the file.
    pub sha256: String,
    /// The file's size in bytes.
    pub len: u64,
}

#[derive(Debug)]
pub enum OpenError {
    Io(io::Error),
    /// It isn't a project file, or it's damaged.
    Format(String),
    /// A newer build wrote it, in a format this one doesn't know.
    NewerVersion(u32),
}

#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    /// These pages were dropped as bytes alone, so the project can't say where their photos are.
    NoPath(Vec<String>),
}

/// What's in the file.
#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    pages: Vec<SavedPage>,
}

impl SavedPage {
    /// `page` as it would be saved, with its photo's absolute path. `None` if it has no path.
    pub fn of(page: &Page) -> Option<Self> {
        Some(Self {
            name: page.name.clone(),
            photo: Photo {
                path: page.path.clone()?,
                sha256: page.sha256.clone(),
                len: page.len,
            },
            corners: page.corners,
            transcript: page.transcript.clone(),
        })
    }

    /// Whether saving `page` would save this, without copying it.
    pub fn matches(&self, page: &Page) -> bool {
        self.name == page.name
            && page.path.as_ref() == Some(&self.photo.path)
            && self.photo.sha256 == page.sha256
            && self.corners == page.corners
            && self.transcript == page.transcript
    }
}

/// Saves `pages` to the project file at `path`. Photo paths are written relative to the project's
/// directory.
pub fn save(path: &Path, pages: &Pages) -> Result<(), SaveError> {
    let no_path: Vec<String> = pages
        .iter()
        .filter(|p| p.path.is_none())
        .map(|p| p.name.clone())
        .collect();
    if !no_path.is_empty() {
        return Err(SaveError::NoPath(no_path));
    }
    let mut saved: Vec<SavedPage> = pages.iter().filter_map(SavedPage::of).collect();
    let dir = project_dir(path);
    for page in &mut saved {
        page.photo.path = relative(&page.photo.path, &dir);
    }
    let file = File {
        version: PROJECT_VERSION,
        pages: saved,
    };
    json_file::write_gzipped(path, &file).map_err(SaveError::Io)
}

/// Opens the project file at `path`, reading each page's photo. Photos that aren't there, or
/// aren't the same file, leave their pages [missing](Page::missing) them, in their place.
pub fn open(path: &Path) -> Result<Pages, OpenError> {
    let bytes = json(path)?;
    let version = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|e| OpenError::Format(e.to_string()))?
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| OpenError::Format("no version".to_owned()))?;
    if version > u64::from(PROJECT_VERSION) {
        return Err(OpenError::NewerVersion(
            u32::try_from(version).unwrap_or(u32::MAX),
        ));
    }
    let file: File =
        serde_json::from_slice(&bytes).map_err(|e| OpenError::Format(e.to_string()))?;
    let dir = project_dir(path);
    let pages = file.pages.into_iter().map(|saved| {
        let photo = Photo {
            path: joined(&dir, &saved.photo.path),
            ..saved.photo
        };
        let page = match read_photo(&photo) {
            Ok(bytes) => Page::new(saved.name, bytes).at(photo.path),
            Err(why) => Page::missing(saved.name, photo.path, photo.sha256, photo.len, why),
        };
        Page {
            corners: saved.corners,
            transcript: saved.transcript,
            ..page
        }
    });
    Ok(Pages::in_order(pages.collect()))
}

/// The JSON in the project file at `path`, ungzipped.
pub fn json(path: &Path) -> Result<Vec<u8>, OpenError> {
    let gzipped = std::fs::read(path).map_err(OpenError::Io)?;
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(&gzipped[..])
        .read_to_end(&mut json)
        .map_err(|e| OpenError::Format(e.to_string()))?;
    Ok(json)
}

/// The photo's file, if it's still the one saved.
fn read_photo(photo: &Photo) -> Result<Arc<[u8]>, Why> {
    let bytes = std::fs::read(&photo.path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Why::NotFound,
        _ => Why::Unreadable(e.to_string()),
    })?;
    if sha256_hex(&bytes) != photo.sha256 {
        return Err(Why::Changed);
    }
    Ok(bytes.into())
}

/// The directory the project file at `path` is in.
fn project_dir(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// `path`, relative to the directory `base` or absolute, as an absolute path without `..`s.
/// Lexically, as symlinks don't matter here: the hash says whether it's the right file.
fn joined(base: &Path, path: &Path) -> PathBuf {
    let mut joined = PathBuf::new();
    for part in base.join(path).components() {
        match part {
            Component::ParentDir if joined.file_name().is_some() => {
                joined.pop();
            }
            // Above the root is the root.
            Component::ParentDir if joined.has_root() => {}
            Component::CurDir => {}
            part => joined.push(part),
        }
    }
    joined
}

/// `path` relative to the directory `base`, both absolute. Left absolute if they share nothing but
/// the root, as for a photo on another volume, so moving the project doesn't lose it.
fn relative(path: &Path, base: &Path) -> PathBuf {
    let (path_parts, base_parts): (Vec<_>, Vec<_>) =
        (path.components().collect(), base.components().collect());
    let common = path_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let shares_a_dir = path_parts[..common]
        .iter()
        .any(|c| matches!(c, Component::Normal(_)));
    if !shares_a_dir || !path.is_absolute() || !base.is_absolute() {
        return path.to_path_buf();
    }
    let up = base_parts.len() - common;
    std::iter::repeat_n(Component::ParentDir, up)
        .chain(path_parts[common..].iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_joined_without_dots() {
        let base = Path::new("/Users/a/notes");
        let join = |path: &str| joined(base, Path::new(path));
        assert_eq!(
            join("../photos/./1.jpg"),
            Path::new("/Users/a/photos/1.jpg")
        );
        assert_eq!(join("1.jpg"), Path::new("/Users/a/notes/1.jpg"));
        assert_eq!(join("/Volumes/SD/1.jpg"), Path::new("/Volumes/SD/1.jpg"));
        assert_eq!(join("../../../../1.jpg"), Path::new("/1.jpg"));
    }

    #[test]
    fn paths_are_relative_to_the_project() {
        let rel = |path: &str, base: &str| relative(Path::new(path), Path::new(base));
        assert_eq!(
            rel("/Users/a/notes/IMG_1.jpg", "/Users/a/notes"),
            Path::new("IMG_1.jpg")
        );
        assert_eq!(
            rel("/Users/a/photos/IMG_1.jpg", "/Users/a/notes"),
            Path::new("../photos/IMG_1.jpg")
        );
        assert_eq!(
            rel("/Users/a/notes/hike/IMG_1.jpg", "/Users/a/notes"),
            Path::new("hike/IMG_1.jpg")
        );
        assert_eq!(
            rel("/Volumes/SD/IMG_1.jpg", "/Users/a/notes"),
            Path::new("/Volumes/SD/IMG_1.jpg"),
            "another volume"
        );
    }
}
