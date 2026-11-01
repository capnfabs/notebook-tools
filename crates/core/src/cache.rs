//! Caches Claude's responses on disk, so a page is only transcribed again when its photo changes
//! or [`CACHE_VERSION`] is bumped. An entry is the raw response, parsed on every load just like a
//! live one. Each [`Model`] has its own entry for a photo, and either will do for a page. Entries
//! keep the corners the photo was cropped to, so boxes can follow the page if its corners move.
//! Entries from older versions stay until the user clears the cache, in Settings or with
//! `notebook-tools --clear-caches-only`. See `decisions/2026-10-31-transcription-cache.md`,
//! `decisions/2026-11-01-manual-cache-clearing.md` and `decisions/2026-10-30-boxes-follow-corners.md`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::claude_transcribe::{Fetched, OcrError, parse_source};
use crate::json_file;
use crate::llm_model::Model;
use crate::markdown_style::MarkdownStyle;
use crate::transcript::{Source, Transcript};

/// Version of cached responses, which are also kept per [`Model`]. Bump it when the schema in
/// [`crate::claude_transcribe`] changes, since old responses won't parse; a test there checks for
/// that. After a prompt, model version or send-image change, bump it only if the cached
/// transcriptions should be redone.
pub const CACHE_VERSION: u32 = 1;

/// A directory of cached responses, one file per photo and model.
#[derive(Clone)]
pub struct Cache {
    dir: PathBuf,
}

/// What's stored: the response, where the image sent was cut from the photo and its size, which
/// the response's boxes are measured in, and when.
#[derive(Serialize, Deserialize)]
struct Entry {
    #[serde(flatten)]
    source: Source,
    /// When it was stored, in seconds since the Unix epoch.
    /// See `decisions/2026-10-24-cache-status.md`.
    stored_at: u64,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The user's cache directory. `notebook-tools --clear-caches-only` (which
    /// `scripts/clear-cache.sh` runs) removes it with [`Self::clear`].
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

    /// How many entries there are, of any version: other files don't count. A missing directory
    /// has none.
    pub fn count(&self) -> io::Result<usize> {
        let dir = match std::fs::read_dir(&self.dir) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
            dir => dir?,
        };
        let mut count = 0;
        for entry in dir {
            let entry = entry?;
            let is_entry = entry.file_name().to_str().and_then(key_version).is_some();
            if is_entry && entry.file_type()?.is_file() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Removes the cache directory and everything in it, returning how many entries went, as
    /// [`Self::count`] counts them. A missing directory has nothing to clear.
    pub fn clear(&self) -> io::Result<usize> {
        let count = self.count()?;
        match std::fs::remove_dir_all(&self.dir) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(0),
            removed => removed.map(|()| count),
        }
    }

    /// The photo's transcript by `model` in `style`, if there's a readable entry.
    pub fn get(
        &self,
        page_bytes: &[u8],
        model: Model,
        style: &MarkdownStyle,
    ) -> Option<Transcript> {
        load(&self.entry_path(page_bytes, model), style)
    }

    /// The photo's transcript by `preferred`, else by any other model, in `style`.
    pub fn find(
        &self,
        page_bytes: &[u8],
        preferred: Model,
        style: &MarkdownStyle,
    ) -> Option<Transcript> {
        let others = Model::ALL.into_iter().filter(|&m| m != preferred);
        std::iter::once(preferred)
            .chain(others)
            .find_map(|model| self.get(page_bytes, model, style))
    }

    /// The photo's cached transcript by any model, preferring `model`, else [`Self::refresh`]'s;
    /// in `style` either way.
    pub fn transcript_or(
        &self,
        page_bytes: &[u8],
        model: Model,
        style: &MarkdownStyle,
        fetch: impl FnOnce() -> Result<Fetched, OcrError>,
    ) -> Result<Transcript, OcrError> {
        match self.find(page_bytes, model, style) {
            Some(transcript) => Ok(transcript),
            None => self.refresh(page_bytes, model, style, fetch),
        }
    }

    /// `fetch`'s transcript by `model`, which replaces the photo's entry for `model` if it parses:
    /// refusals, truncated answers and errors leave the entry as it was. A failure to write one
    /// only logs.
    pub fn refresh(
        &self,
        page_bytes: &[u8],
        model: Model,
        style: &MarkdownStyle,
        fetch: impl FnOnce() -> Result<Fetched, OcrError>,
    ) -> Result<Transcript, OcrError> {
        let path = self.entry_path(page_bytes, model);
        let fetched = fetch()?;
        let transcript = fetched.parse(style)?;
        let stored_at = unix_secs(SystemTime::now());
        let entry = transcript.source.as_ref().map(|source| Entry {
            source: Source::clone(source),
            stored_at,
        });
        let written = match entry {
            Some(entry) => json_file::write(&path, &entry),
            None => Err(io::Error::other("no response to store")),
        };
        match written {
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

/// The transcript in the entry at `path`, in `style`.
fn load(path: &Path, style: &MarkdownStyle) -> Option<Transcript> {
    let bytes = std::fs::read(path).ok()?;
    let loaded = serde_json::from_slice::<Entry>(&bytes)
        .map_err(|e| OcrError::Parse(e.to_string()))
        .and_then(|entry| {
            let transcript = parse_source(Arc::new(entry.source), style)?;
            Ok((entry.stored_at, transcript))
        });
    match loaded {
        Ok((stored_at, transcript)) => {
            log::debug!("using cached transcript {}", path.display());
            Some(Transcript {
                from_cache: true,
                stored_at: Some(from_unix_secs(stored_at)),
                ..transcript
            })
        }
        Err(err) => {
            log::warn!("ignoring cached transcript {}: {err}", path.display());
            None
        }
    }
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

/// The cache version in an entry's file name, if it's one's: `<sha256>-v<version>-<model>.json`,
/// or without `-<model>` from before entries were per model.
fn key_version(file_name: &str) -> Option<u32> {
    let (hash, rest) = file_name.strip_suffix(".json")?.split_at_checked(64)?;
    if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let rest = rest.strip_prefix("-v")?;
    let version = rest
        .split_once('-')
        .map_or(rest, |(version, _model)| version);
    version.parse().ok()
}

/// The SHA-256 of `bytes` in lowercase hex, as `shasum -a 256` prints it.
pub fn sha256_hex(bytes: &[u8]) -> String {
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

    #[test]
    fn key_version_reads_entry_names() {
        let name = format!("{}.json", key(b"page", 7, Model::Sonnet));
        assert_eq!(key_version(&name), Some(7));
        let hash = sha256_hex(b"page");
        assert_eq!(
            key_version(&format!("{hash}-v3.json")),
            Some(3),
            "before models"
        );
        for other in [
            format!("{hash}-v3.json.tmp"),
            format!("{hash}-3-opus.json"),
            format!("{hash}-vx-opus.json"),
            format!("{}-v3-opus.json", "z".repeat(64)),
            "notes.json".to_owned(),
            ".DS_Store".to_owned(),
        ] {
            assert_eq!(key_version(&other), None, "{other}");
        }
    }
}
