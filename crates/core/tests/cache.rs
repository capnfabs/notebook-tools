//! The transcription cache, with recorded responses standing in for the API.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use notebook_core::cache::{Cache, Fetched};
use notebook_core::ocr::{OcrError, Transcript};

const PHOTO: &[u8] = b"photo bytes";
const SIZE: (u32, u32) = (1200, 900);

fn fixture(name: &str) -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr");
    std::fs::read(dir.join(format!("{name}.json"))).unwrap()
}

/// An empty cache of its own for each test.
fn cache(test: &str) -> Cache {
    let dir: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("cache")
        .join(test);
    std::fs::remove_dir_all(&dir).ok();
    Cache::new(dir)
}

/// Transcribes `photo`, answering with `fixture` if the cache asks; counts the asks in `fetches`.
fn transcribe(
    cache: &Cache,
    photo: &[u8],
    status: u16,
    fixture_name: &str,
    fetches: &Cell<u32>,
) -> Result<Transcript, OcrError> {
    cache.transcript_or(photo, || -> Result<Fetched, OcrError> {
        fetches.set(fetches.get() + 1);
        Ok((status, fixture(fixture_name), SIZE))
    })
}

#[test]
fn second_transcription_comes_from_the_cache() {
    let cache = cache("hit");
    let fetches = Cell::new(0);
    let first = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1);
    assert!(!first.from_cache);
    assert!(cache.entry_path(PHOTO).exists());

    let second = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1, "not fetched again");
    assert!(second.from_cache);
    assert_eq!(
        second,
        Transcript {
            from_cache: true,
            ..first
        }
    );
}

#[test]
fn another_photo_is_fetched() {
    let cache = cache("other_photo");
    let fetches = Cell::new(0);
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    transcribe(&cache, b"another photo", 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 2);
}

#[test]
fn failures_are_not_cached() {
    for (status, name) in [(200, "refusal"), (200, "max_tokens"), (529, "api_error")] {
        let cache = cache(&format!("failure_{name}"));
        let fetches = Cell::new(0);
        assert!(transcribe(&cache, PHOTO, status, name, &fetches).is_err());
        assert!(!cache.entry_path(PHOTO).exists(), "{name} stored");
        assert!(transcribe(&cache, PHOTO, status, name, &fetches).is_err());
        assert_eq!(fetches.get(), 2, "{name} fetched again");
    }
}

#[test]
fn corrupt_entry_is_fetched_again_and_replaced() {
    let cache = cache("corrupt");
    let path = cache.entry_path(PHOTO);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not json").unwrap();

    let fetches = Cell::new(0);
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1);
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1, "replaced entry used");
}
