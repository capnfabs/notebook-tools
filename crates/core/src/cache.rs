//! Caches Claude's responses on disk, so a page is only transcribed again when its photo changes
//! or [`CACHE_VERSION`] is bumped. An entry is the raw response, parsed on every load just like a
//! live one. See `decisions/2026-10-24-transcription-cache.md`.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::ocr::{CACHE_VERSION, OcrError, Transcript, parse_response};

/// A response as it came back: status, body, and the size of the image sent.
pub type Fetched = (u16, Vec<u8>, (u32, u32));

/// A directory of cached responses, one file per photo.
pub struct Cache {
    dir: PathBuf,
}

/// What's stored: the response, plus the size of the image sent, which its boxes are measured in.
#[derive(Serialize, Deserialize)]
struct Entry {
    image_size: (u32, u32),
    response: Value,
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

    /// Where the entry for the photo `page_bytes` is kept.
    pub fn entry_path(&self, page_bytes: &[u8]) -> PathBuf {
        self.dir
            .join(format!("{}.json", key(page_bytes, CACHE_VERSION)))
    }

    /// The photo's cached transcript, else `fetch`'s, which is cached if it parses: refusals,
    /// truncated answers and errors aren't. A missing or unreadable entry is fetched again, and a
    /// failure to write one only logs.
    pub fn transcript_or(
        &self,
        page_bytes: &[u8],
        fetch: impl FnOnce() -> Result<Fetched, OcrError>,
    ) -> Result<Transcript, OcrError> {
        let path = self.entry_path(page_bytes);
        if let Some(transcript) = load(&path) {
            return Ok(transcript);
        }
        let (status, body, image_size) = fetch()?;
        let transcript = parse_response(status, &body, image_size)?;
        if let Err(err) = store(&path, image_size, &body) {
            log::warn!("couldn't cache {}: {err}", path.display());
        }
        Ok(transcript)
    }
}

fn load(path: &Path) -> Option<Transcript> {
    let bytes = std::fs::read(path).ok()?;
    let transcript = serde_json::from_slice::<Entry>(&bytes)
        .map_err(|e| OcrError::Parse(e.to_string()))
        .and_then(|entry| {
            parse_response(200, entry.response.to_string().as_bytes(), entry.image_size)
        });
    match transcript {
        Ok(transcript) => {
            log::debug!("using cached transcript {}", path.display());
            Some(Transcript {
                from_cache: true,
                ..transcript
            })
        }
        Err(err) => {
            log::warn!("ignoring cached transcript {}: {err}", path.display());
            None
        }
    }
}

fn store(path: &Path, image_size: (u32, u32), body: &[u8]) -> io::Result<()> {
    let entry = Entry {
        image_size,
        response: serde_json::from_slice(body)?,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec(&entry)?)
}

/// Names the entry for `page_bytes` at cache version `version`.
fn key(page_bytes: &[u8], version: u32) -> String {
    format!("{}-v{version}", sha256_hex(page_bytes))
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
    fn key_depends_on_bytes_and_version() {
        assert_eq!(key(b"page", 1), key(b"page", 1));
        assert_ne!(key(b"page", 1), key(b"other page", 1));
        assert_ne!(key(b"page", 1), key(b"page", 2));
        assert_eq!(
            key(b"abc", 3),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad-v3",
            "sha256, so `shasum -a 256` finds a photo's entry"
        );
    }
}
