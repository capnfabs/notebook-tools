//! Caches Claude's responses on disk, so a page is only transcribed again when its photo changes
//! or [`CACHE_VERSION`] is bumped. An entry is the raw response, parsed on every load just like a
//! live one. Each [`Model`] has its own entry for a photo, and either will do for a page. Entries
//! keep the corners the photo was cropped to, so boxes can follow the page if its corners move.
//! See `decisions/2026-10-24-transcription-cache.md`, `decisions/2026-10-24-model-selector.md` and
//! `decisions/2026-10-30-boxes-follow-corners.md`.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::clean::{CleanParams, Crop, Quad, detect_outline, page_quad, photo_corners};
use crate::llm_model::Model;
use crate::ocr::{CACHE_VERSION, Fetched, OcrError, parse_response};
use crate::transcript::Transcript;

/// A directory of cached responses, one file per photo and model.
#[derive(Clone)]
pub struct Cache {
    dir: PathBuf,
}

/// What's stored: the response, plus where the image sent was cut from the photo and its size,
/// which the response's boxes are measured in.
#[derive(Serialize, Deserialize)]
struct Entry {
    image_size: (u32, u32),
    /// `None` only in entries stored before corners were; see [`backfill_corners`].
    #[serde(default)]
    corners: Option<Quad>,
    response: Value,
    /// When it was stored, in seconds since the Unix epoch.
    /// See `decisions/2026-10-24-cache-status.md`.
    stored_at: u64,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The user's cache directory. `scripts/clear-cache.sh` removes it; keep the two in step.
    pub fn default_dir() -> Option<PathBuf> {
        Some(dirs::cache_dir()?.join("notebook-tools/transcriptions"))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where `model`'s entry for the photo `page_bytes` is kept.
    pub fn entry_path(&self, page_bytes: &[u8], model: Model) -> PathBuf {
        self.dir
            .join(format!("{}.json", key(page_bytes, CACHE_VERSION, model)))
    }

    /// The photo's transcript by `model`, if there's a readable entry.
    pub fn get(&self, page_bytes: &[u8], model: Model) -> Option<Transcript> {
        load(&self.entry_path(page_bytes, model), page_bytes)
    }

    /// The photo's transcript by `preferred`, else by any other model.
    pub fn find(&self, page_bytes: &[u8], preferred: Model) -> Option<Transcript> {
        let others = Model::ALL.into_iter().filter(|&m| m != preferred);
        std::iter::once(preferred)
            .chain(others)
            .find_map(|model| self.get(page_bytes, model))
    }

    /// The photo's cached transcript by any model, preferring `model`, else [`Self::refresh`]'s.
    pub fn transcript_or(
        &self,
        page_bytes: &[u8],
        model: Model,
        fetch: impl FnOnce() -> Result<Fetched, OcrError>,
    ) -> Result<Transcript, OcrError> {
        match self.find(page_bytes, model) {
            Some(transcript) => Ok(transcript),
            None => self.refresh(page_bytes, model, fetch),
        }
    }

    /// `fetch`'s transcript by `model`, which replaces the photo's entry for `model` if it parses:
    /// refusals, truncated answers and errors leave the entry as it was. A failure to write one
    /// only logs.
    pub fn refresh(
        &self,
        page_bytes: &[u8],
        model: Model,
        fetch: impl FnOnce() -> Result<Fetched, OcrError>,
    ) -> Result<Transcript, OcrError> {
        let path = self.entry_path(page_bytes, model);
        let Fetched { status, body, crop } = fetch()?;
        let transcript = parse_response(status, &body, crop)?;
        let stored_at = unix_secs(SystemTime::now());
        let entry = serde_json::from_slice(&body).map(|response| Entry {
            image_size: crop.size,
            corners: Some(crop.corners),
            response,
            stored_at,
        });
        match entry
            .map_err(io::Error::from)
            .and_then(|e| write(&path, &e))
        {
            Ok(()) => Ok(Transcript {
                stored_at: Some(from_unix_secs(stored_at)),
                ..transcript
            }),
            Err(err) => {
                log::warn!("couldn't cache {}: {err}", path.display());
                Ok(transcript)
            }
        }
    }
}

/// The transcript in the entry at `path`, for the photo `page_bytes`.
fn load(path: &Path, page_bytes: &[u8]) -> Option<Transcript> {
    let bytes = std::fs::read(path).ok()?;
    let loaded = serde_json::from_slice::<Entry>(&bytes)
        .map_err(|e| OcrError::Parse(e.to_string()))
        .and_then(|mut entry| {
            let corners = match entry.corners {
                Some(corners) => corners,
                None => backfill_corners(path, page_bytes, &mut entry)?,
            };
            let crop = Crop {
                corners,
                size: entry.image_size,
            };
            let response = entry.response.to_string();
            let transcript = parse_response(200, response.as_bytes(), crop)?;
            Ok((entry, transcript))
        });
    match loaded {
        Ok((entry, transcript)) => {
            log::debug!("using cached transcript {}", path.display());
            Some(Transcript {
                from_cache: true,
                stored_at: Some(from_unix_secs(entry.stored_at)),
                ..transcript
            })
        }
        Err(err) => {
            log::warn!("ignoring cached transcript {}: {err}", path.display());
            None
        }
    }
}

/// Corners for `entry`, stored at `path` before entries kept them: those cleaning finds with the
/// default parameters, which is where the photo was cropped unless the user placed corners. They're
/// written back to the entry, so this happens once. Remove with `tasks/remove-corner-backfill.md`.
fn backfill_corners(path: &Path, page_bytes: &[u8], entry: &mut Entry) -> Result<Quad, OcrError> {
    let photo = crate::decode(page_bytes).map_err(|e| OcrError::Decode(e.to_string()))?;
    let detected = detect_outline(&photo, &CleanParams::default());
    let corners =
        page_quad(None, &detected).unwrap_or_else(|| photo_corners(photo.width(), photo.height()));
    entry.corners = Some(corners);
    match write(path, entry) {
        Ok(()) => log::info!("backfilled corners in {}", path.display()),
        Err(err) => log::warn!("couldn't backfill corners in {}: {err}", path.display()),
    }
    Ok(corners)
}

fn write(path: &Path, entry: &Entry) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec(entry)?)
}

fn unix_secs(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn from_unix_secs(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

/// Names `model`'s entry for `page_bytes` at cache version `version`.
fn key(page_bytes: &[u8], version: u32, model: Model) -> String {
    format!("{}-v{version}-{}", sha256_hex(page_bytes), model.name())
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_depends_on_bytes_version_and_model() {
        let key = |bytes, version| key(bytes, version, Model::Opus);
        assert_eq!(key(b"page", 1), key(b"page", 1));
        assert_ne!(key(b"page", 1), key(b"other page", 1));
        assert_ne!(key(b"page", 1), key(b"page", 2));
        assert_ne!(key(b"page", 1), super::key(b"page", 1, Model::Sonnet));
        assert_eq!(
            super::key(b"abc", 3, Model::Sonnet),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad-v3-sonnet",
            "sha256, so `shasum -a 256` finds a photo's entry"
        );
    }
}
