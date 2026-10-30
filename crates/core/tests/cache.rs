//! The transcription cache, with recorded responses standing in for the API.

use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, SystemTime};

use notebook_core::Crop;
use notebook_core::cache::Cache;
use notebook_core::compare;
use notebook_core::llm_model::Model;
use notebook_core::markdown_style::MarkdownStyle;
use notebook_core::ocr::{CACHE_VERSION, Fetched, OcrError};
use notebook_core::transcript::Transcript;
use tempfile::TempDir;

const PHOTO: &[u8] = b"photo bytes";
/// Where the image sent was cut from [`PHOTO`].
const CROP: Crop = Crop {
    corners: [[10.0, 20.0], [1300.0, 30.0], [1290.0, 1000.0], [5.0, 990.0]],
    size: (1200, 900),
};

fn fixture(name: &str) -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr");
    std::fs::read(dir.join(format!("{name}.json"))).unwrap()
}

/// An empty cache of its own for each test, in a directory removed when dropped. Unique, so
/// overlapping runs of the suite, from two terminals say, don't share entries.
fn cache(test: &str) -> (TempDir, Cache) {
    let dir = tempfile::Builder::new()
        .prefix(&format!("cache-{test}-"))
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let cache = Cache::new(dir.path());
    (dir, cache)
}

/// Transcribes `photo` with Opus, answering with `fixture` if the cache asks; counts the asks in
/// `fetches`.
fn transcribe(
    cache: &Cache,
    photo: &[u8],
    status: u16,
    fixture_name: &str,
    fetches: &Cell<u32>,
) -> Result<Transcript, OcrError> {
    transcribe_with(cache, Model::Opus, photo, status, fixture_name, fetches)
}

fn transcribe_with(
    cache: &Cache,
    model: Model,
    photo: &[u8],
    status: u16,
    fixture_name: &str,
    fetches: &Cell<u32>,
) -> Result<Transcript, OcrError> {
    cache.transcript_or(
        photo,
        model,
        &MarkdownStyle::default(),
        || -> Result<Fetched, OcrError> {
            fetches.set(fetches.get() + 1);
            Ok(Fetched {
                status,
                body: fixture(fixture_name),
                crop: CROP,
            })
        },
    )
}

#[test]
fn second_transcription_comes_from_the_cache() {
    let (_dir, cache) = cache("hit");
    let fetches = Cell::new(0);
    let first = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1);
    assert!(!first.from_cache);
    assert!(cache.entry_path(PHOTO, Model::Opus).exists());

    let second = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1, "not fetched again");
    assert!(second.from_cache);
    assert_eq!(second.crop, CROP, "kept where the photo was cropped");
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
    let (_dir, cache) = cache("other_photo");
    let fetches = Cell::new(0);
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    transcribe(&cache, b"another photo", 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 2);
}

#[test]
fn failures_are_not_cached() {
    for (status, name) in [(200, "refusal"), (200, "max_tokens"), (529, "api_error")] {
        let (_dir, cache) = cache(&format!("failure_{name}"));
        let fetches = Cell::new(0);
        assert!(transcribe(&cache, PHOTO, status, name, &fetches).is_err());
        assert!(
            !cache.entry_path(PHOTO, Model::Opus).exists(),
            "{name} stored"
        );
        assert!(transcribe(&cache, PHOTO, status, name, &fetches).is_err());
        assert_eq!(fetches.get(), 2, "{name} fetched again");
    }
}

#[test]
fn corrupt_entry_is_fetched_again_and_replaced() {
    let (_dir, cache) = cache("corrupt");
    let path = cache.entry_path(PHOTO, Model::Opus);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not json").unwrap();

    let fetches = Cell::new(0);
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1);
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1, "replaced entry used");
}

#[test]
fn entries_keep_when_they_were_stored() {
    let (_dir, cache) = cache("stored_at");
    let fetches = Cell::new(0);
    let before = SystemTime::now() - Duration::from_secs(1);
    let fresh = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    let stored_at = fresh.stored_at.expect("stored");
    assert!(stored_at >= before && stored_at <= SystemTime::now());
    assert_eq!(
        cache
            .get(PHOTO, Model::Opus, &MarkdownStyle::default())
            .unwrap()
            .stored_at,
        Some(stored_at)
    );
}

#[test]
fn entries_are_read_in_the_style_asked_for() {
    let (_dir, cache) = cache("style");
    let underlined = String::from_utf8(fixture("success"))
        .unwrap()
        .replace("# Thurs 23 Oct", "<u>Thurs 23 Oct</u>")
        .replace("buy eggs", "buy <u>eggs</u>");
    let fetch = || -> Result<Fetched, OcrError> {
        Ok(Fetched {
            status: 200,
            body: underlined.into_bytes(),
            crop: CROP,
        })
    };
    let fresh = cache
        .transcript_or(PHOTO, Model::Opus, &MarkdownStyle::default(), fetch)
        .unwrap();
    assert_eq!(fresh.lines[0].text_md, "## Thurs 23 Oct");
    assert_eq!(fresh.lines[2].text_md, "- [x] buy _eggs_");

    let style = MarkdownStyle {
        underline: "*".to_owned(),
        underlined_line_heading: 3,
    };
    let restyled = cache.get(PHOTO, Model::Opus, &style).unwrap();
    assert_eq!(restyled.lines[0].text_md, "### Thurs 23 Oct");
    assert_eq!(restyled.lines[2].text_md, "- [x] buy *eggs*");
    assert_eq!(restyled.style, style);
}

#[test]
fn refresh_fetches_and_replaces_the_entry() {
    let (_dir, cache) = cache("refresh");
    let fetches = Cell::new(0);
    let first = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    let refreshed = cache
        .refresh(
            PHOTO,
            Model::Opus,
            &MarkdownStyle::default(),
            || -> Result<Fetched, OcrError> {
                Ok(Fetched {
                    status: 200,
                    body: fixture("fallback"),
                    crop: CROP,
                })
            },
        )
        .unwrap();
    assert!(!refreshed.from_cache);
    assert_ne!(refreshed.model, first.model);
    assert_eq!(
        cache
            .get(PHOTO, Model::Opus, &MarkdownStyle::default())
            .unwrap()
            .model,
        refreshed.model
    );
}

#[test]
fn failed_refresh_keeps_the_entry() {
    let (_dir, cache) = cache("failed_refresh");
    let fetches = Cell::new(0);
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    let before = std::fs::read(cache.entry_path(PHOTO, Model::Opus)).unwrap();
    let failed = cache.refresh(
        PHOTO,
        Model::Opus,
        &MarkdownStyle::default(),
        || -> Result<Fetched, OcrError> {
            Ok(Fetched {
                status: 529,
                body: fixture("api_error"),
                crop: CROP,
            })
        },
    );
    assert!(failed.is_err());
    assert_eq!(
        std::fs::read(cache.entry_path(PHOTO, Model::Opus)).unwrap(),
        before
    );
}

/// Answers `refresh` with the `fallback` fixture, whose model differs from `success`'s.
fn refresh_with(cache: &Cache, model: Model) -> Transcript {
    cache
        .refresh(
            PHOTO,
            model,
            &MarkdownStyle::default(),
            || -> Result<Fetched, OcrError> {
                Ok(Fetched {
                    status: 200,
                    body: fixture("fallback"),
                    crop: CROP,
                })
            },
        )
        .unwrap()
}

#[test]
fn any_models_entry_is_used() {
    let (_dir, cache) = cache("any_model");
    let fetches = Cell::new(0);
    let opus = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    let sonnet = transcribe_with(&cache, Model::Sonnet, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1, "not fetched for Sonnet");
    assert_eq!(sonnet.model, opus.model);
    assert!(!cache.entry_path(PHOTO, Model::Sonnet).exists());
}

#[test]
fn the_preferred_models_entry_wins() {
    let (_dir, cache) = cache("preferred");
    let fetches = Cell::new(0);
    let opus = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(
        cache
            .find(PHOTO, Model::Sonnet, &MarkdownStyle::default())
            .unwrap()
            .model,
        opus.model
    );

    let sonnet = refresh_with(&cache, Model::Sonnet);
    assert_ne!(sonnet.model, opus.model);
    assert_eq!(
        cache
            .find(PHOTO, Model::Sonnet, &MarkdownStyle::default())
            .unwrap()
            .model,
        sonnet.model
    );
    assert_eq!(
        cache
            .find(PHOTO, Model::Opus, &MarkdownStyle::default())
            .unwrap()
            .model,
        opus.model
    );
    assert!(
        cache
            .find(b"another photo", Model::Opus, &MarkdownStyle::default())
            .is_none()
    );
}

#[test]
fn refresh_writes_only_its_models_entry() {
    let (_dir, cache) = cache("refresh_one_model");
    let fetches = Cell::new(0);
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    let opus_entry = std::fs::read(cache.entry_path(PHOTO, Model::Opus)).unwrap();
    refresh_with(&cache, Model::Sonnet);
    assert!(cache.entry_path(PHOTO, Model::Sonnet).exists());
    assert_eq!(
        std::fs::read(cache.entry_path(PHOTO, Model::Opus)).unwrap(),
        opus_entry
    );
    let name = cache.entry_path(PHOTO, Model::Sonnet);
    assert!(
        name.to_string_lossy().ends_with("-v1-sonnet.json"),
        "{}",
        name.display()
    );
}

#[test]
fn compared_with_other_models_entries() {
    let (_dir, cache) = cache("compare");
    let fetches = Cell::new(0);
    let opus = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    let sonnet = Transcript {
        model: Model::Sonnet.id().to_owned(),
        ..opus.clone()
    };
    let comparisons = compare::with_cached(&cache, PHOTO, &sonnet);
    assert_eq!(comparisons.len(), 1);
    assert_eq!(comparisons[0].0, Model::Opus);
    assert_eq!(comparisons[0].1.similarity, 1.0);

    assert!(
        compare::with_cached(&cache, PHOTO, &opus).is_empty(),
        "not with its own model's entry"
    );
    assert!(compare::with_cached(&cache, b"another photo", &sonnet).is_empty());
}

/// Stores `fixture("success")` for [`PHOTO`] the way entries were before they kept corners.
fn write_entry_without_corners(cache: &Cache) {
    let path = cache.entry_path(PHOTO, Model::Opus);
    let response: serde_json::Value = serde_json::from_slice(&fixture("success")).unwrap();
    let entry = serde_json::json!({
        "image_size": [1200, 900],
        "response": response,
        "stored_at": 1,
    });
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, entry.to_string()).unwrap();
}

#[test]
fn entries_without_corners_are_fetched_again() {
    let (_dir, cache) = cache("no_corners");
    write_entry_without_corners(&cache);
    assert!(
        cache
            .get(PHOTO, Model::Opus, &MarkdownStyle::default())
            .is_none()
    );
    let fetches = Cell::new(0);
    let transcript = transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1);
    assert_eq!(transcript.crop, CROP);
}

#[test]
fn entries_are_formatted_json() {
    let (_dir, cache) = cache("formatted");
    transcribe(&cache, PHOTO, 200, "success", &Cell::new(0)).unwrap();
    let stored = std::fs::read_to_string(cache.entry_path(PHOTO, Model::Opus)).unwrap();
    assert!(
        stored.starts_with("{\n  \"image_size\": [\n    1200,"),
        "{stored}"
    );
}

#[test]
fn pruning_removes_only_older_versions_entries() {
    let (dir, cache) = cache("prune");
    let fetches = Cell::new(0);
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    let current = cache.entry_path(PHOTO, Model::Opus);
    let hash = current.file_name().unwrap().to_str().unwrap()[..64].to_owned();
    let file = |name: String| {
        let path = dir.path().join(name);
        std::fs::write(&path, "{}").unwrap();
        path
    };
    let older = [
        file(format!("{hash}-v{}-opus.json", CACHE_VERSION - 1)),
        file(format!("{hash}-v{}.json", CACHE_VERSION - 1)),
    ];
    let kept = [
        current,
        file(format!("{hash}-v{}-sonnet.json", CACHE_VERSION + 1)),
        file("notes.txt".to_owned()),
    ];

    assert_eq!(cache.prune().unwrap(), older.len());
    for path in &older {
        assert!(!path.exists(), "{} kept", path.display());
    }
    for path in &kept {
        assert!(path.exists(), "{} removed", path.display());
    }
    transcribe(&cache, PHOTO, 200, "success", &fetches).unwrap();
    assert_eq!(fetches.get(), 1, "current entry still used");
}

#[test]
fn pruning_a_missing_cache_does_nothing() {
    let (dir, _cache) = cache("prune_missing");
    assert_eq!(Cache::new(dir.path().join("missing")).prune().unwrap(), 0);
}
