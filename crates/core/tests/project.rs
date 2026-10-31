//! Saving and opening projects, with photos in temporary directories.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod common;

use common::recorded;
use notebook_core::clean::Crop;
use notebook_core::geom::Quad;
use notebook_core::llm_model::Usage;
use notebook_core::markdown_style::MarkdownStyle;
use notebook_core::page::{Page, Pages};
use notebook_core::project::{self, Missing, OpenError, PROJECT_VERSION, SaveError, Why};
use notebook_core::transcript::{Deleted, Line, Side, Transcript, Uncertain};
use serde_json::Value;
use tempfile::TempDir;

/// A directory of its own for each test, removed when dropped.
fn dir(test: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(&format!("project-{test}-"))
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
}

/// A page read from a new file at `dir/name` holding `contents`.
fn photo(dir: &Path, name: &str, contents: &str) -> Page {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
    Page::new(file_name, Arc::from(contents.as_bytes())).at(path)
}

/// The recorded success, corrected: a word resolved, a line edited, a word deleted.
fn corrected() -> Transcript {
    let mut t = recorded("success");
    assert!(t.resolve(0, "plumbers"));
    t.set_line(2, "- [x] buy six eggs".to_owned());
    t.uncertain[1].deleted = Some(Deleted {
        line: 4,
        at: 3,
        space: Some(Side::After),
    });
    assert!(t.corrected());
    t
}

/// Each page's name, path and corners.
type Summary = Vec<(String, Option<PathBuf>, Option<Quad>)>;

fn summary(pages: &Pages) -> Summary {
    pages
        .iter()
        .map(|p| (p.name.clone(), p.path.clone(), p.corners))
        .collect()
}

fn file(path: &Path) -> Value {
    serde_json::from_slice(&project::json(path).unwrap()).unwrap()
}

/// Writes `json` to `path`, gzipped as a project is.
fn write_gzipped(path: &Path, json: &str) {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(json.as_bytes()).unwrap();
    std::fs::write(path, gz.finish().unwrap()).unwrap();
}

#[test]
fn pages_corners_and_transcripts_come_back() {
    let dir = dir("round_trip");
    let photos = dir.path().join("photos");
    let corners = [[1.0, 2.0], [30.0, 2.0], [30.0, 40.0], [1.0, 40.0]];
    let a = Page {
        corners: Some(corners),
        transcript: Some(recorded("success")),
        ..photo(&photos, "a.jpg", "photo a")
    };
    let b = Page {
        transcript: Some(corrected()),
        ..photo(&photos, "b.jpg", "photo b")
    };
    // Rearranged: b first.
    let pages = Pages::in_order(vec![b, a]);
    let path = dir.path().join("notes/hike.notebook");
    project::save(&path, &pages, &[]).unwrap();

    let opened = project::open(&path).unwrap();
    assert!(opened.missing.is_empty());
    assert_eq!(summary(&opened.pages), summary(&pages));
    for (opened, saved) in opened.pages.iter().zip(pages.iter()) {
        assert_eq!(opened.transcript, saved.transcript, "{}", saved.name);
        assert_eq!(&*opened.bytes, &*saved.bytes);
        assert_eq!(opened.sha256, saved.sha256);
    }
    let b = opened.pages.get(0).unwrap().transcript.as_ref().unwrap();
    assert!(b.corrected() && b.uncertain[0].resolved);
    assert!(b.source.is_some(), "Claude's answer is kept for restyling");

    assert!(
        std::fs::read(&path).unwrap().starts_with(&[0x1f, 0x8b]),
        "gzipped"
    );
    let saved = file(&path);
    assert_eq!(saved["version"], PROJECT_VERSION);
    assert_eq!(saved["pages"][0]["photo"]["path"], "../photos/b.jpg");
    assert_eq!(saved["pages"][0]["photo"]["len"], 7);
}

#[test]
fn saving_elsewhere_keeps_photos_found() {
    let dir = dir("save_as");
    let pages = Pages::in_order(vec![photo(dir.path(), "a.jpg", "photo a")]);
    let first = dir.path().join("first.notebook");
    project::save(&first, &pages, &[]).unwrap();
    assert_eq!(file(&first)["pages"][0]["photo"]["path"], "a.jpg");

    let opened = project::open(&first).unwrap();
    let second = dir.path().join("deep/er/second.notebook");
    project::save(&second, &opened.pages, &opened.missing).unwrap();
    assert_eq!(file(&second)["pages"][0]["photo"]["path"], "../../a.jpg");
    let opened = project::open(&second).unwrap();
    assert_eq!(summary(&opened.pages), summary(&pages));
}

#[test]
fn missing_photos_are_kept_in_place() {
    let dir = dir("missing");
    let pages = Pages::in_order(vec![
        photo(dir.path(), "a.jpg", "photo a"),
        Page {
            transcript: Some(corrected()),
            ..photo(dir.path(), "b.jpg", "photo b")
        },
        photo(dir.path(), "c.jpg", "photo c"),
        photo(dir.path(), "d.jpg", "photo d"),
    ]);
    let path = dir.path().join("p.notebook");
    project::save(&path, &pages, &[]).unwrap();
    let before = file(&path);

    std::fs::remove_file(dir.path().join("b.jpg")).unwrap();
    std::fs::write(dir.path().join("c.jpg"), "another photo").unwrap();
    let opened = project::open(&path).unwrap();
    let names: Vec<&str> = opened.pages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["a.jpg", "d.jpg"]);
    let missing: Vec<(&str, Option<&str>, &Why)> = opened
        .missing
        .iter()
        .map(|m| (m.page.name.as_str(), m.after.as_deref(), &m.why))
        .collect();
    assert_eq!(
        missing,
        [
            ("b.jpg", Some("a.jpg"), &Why::NotFound),
            ("c.jpg", Some("b.jpg"), &Why::Changed)
        ]
    );
    assert_eq!(
        opened.missing[0].page.photo.path,
        dir.path().join("b.jpg"),
        "absolute in memory"
    );

    project::save(&path, &opened.pages, &opened.missing).unwrap();
    assert_eq!(file(&path), before, "saved again unchanged");
}

#[test]
fn a_missing_first_page_stays_first() {
    let dir = dir("missing_first");
    let pages = Pages::in_order(vec![
        photo(dir.path(), "b.jpg", "photo b"),
        photo(dir.path(), "a.jpg", "photo a"),
    ]);
    let path = dir.path().join("p.notebook");
    project::save(&path, &pages, &[]).unwrap();
    std::fs::remove_file(dir.path().join("b.jpg")).unwrap();
    let opened = project::open(&path).unwrap();
    assert_eq!(opened.missing[0].after, None);
    // The page it followed has gone too: it goes at the end.
    let gone = Missing {
        after: Some("gone.jpg".to_owned()),
        ..opened.missing[0].clone()
    };
    let other = dir.path().join("q.notebook");
    project::save(&other, &opened.pages, &[gone]).unwrap();
    project::save(&path, &opened.pages, &opened.missing).unwrap();
    let names = |path: &Path| -> Vec<String> {
        file(path)["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(&path), ["b.jpg", "a.jpg"]);
    assert_eq!(names(&other), ["a.jpg", "b.jpg"]);
}

#[test]
fn pages_without_a_file_cant_be_saved() {
    let dir = dir("no_path");
    let dropped = Page::new("dropped.jpg".to_owned(), Arc::from(&b"bytes"[..]));
    let pages = Pages::in_order(vec![photo(dir.path(), "a.jpg", "photo a"), dropped]);
    let path = dir.path().join("p.notebook");
    let Err(SaveError::NoPath(names)) = project::save(&path, &pages, &[]) else {
        panic!("saved");
    };
    assert_eq!(names, ["dropped.jpg"]);
    assert!(!path.exists());
}

#[test]
fn newer_or_damaged_projects_are_refused() {
    let dir = dir("refused");
    let path = dir.path().join("p.notebook");
    write_gzipped(&path, r#"{"version": 99, "pages": "who knows"}"#);
    assert!(matches!(
        project::open(&path),
        Err(OpenError::NewerVersion(99))
    ));
    write_gzipped(&path, "not json");
    assert!(matches!(project::open(&path), Err(OpenError::Format(_))));
    write_gzipped(&path, r#"{"pages": []}"#);
    assert!(matches!(project::open(&path), Err(OpenError::Format(_))));
    write_gzipped(&path, r#"{"version": 1, "pages": []}"#);
    assert!(project::open(&path).is_ok());
    // Not gzipped, as projects once were.
    std::fs::write(&path, r#"{"version": 1, "pages": []}"#).unwrap();
    assert!(matches!(project::open(&path), Err(OpenError::Format(_))));
    std::fs::write(&path, "").unwrap();
    assert!(matches!(project::open(&path), Err(OpenError::Format(_))));
    assert!(matches!(
        project::open(&dir.path().join("none.notebook")),
        Err(OpenError::Io(_))
    ));
}

/// The file format, so a change to it is noticed: bump [`PROJECT_VERSION`] if older builds would
/// misread the new one.
#[test]
fn project_file_format() {
    let dir = dir("format");
    let mut transcript = Transcript::new(
        vec![
            Line {
                text_md: "# Mon".to_owned(),
                bbox: Some([1.0, 2.0, 3.0, 4.0]),
            },
            Line {
                text_md: String::new(),
                bbox: None,
            },
        ],
        vec![Uncertain {
            line: 0,
            text: "Mon".to_owned(),
            bbox: Some([2.0, 2.0, 3.0, 4.0]),
            alternatives: vec!["Man".to_owned()],
            resolved: false,
            deleted: None,
        }],
        Crop::whole(10, 20),
        MarkdownStyle::default(),
        "claude-opus-5-5".to_owned(),
        Usage {
            input_tokens: 1,
            output_tokens: 3,
            thinking_tokens: 2,
        },
    );
    assert!(transcript.resolve(0, "Mon"));
    let page = Page {
        corners: Some([[0.0, 0.0], [10.0, 0.0], [10.0, 20.0], [0.0, 20.0]]),
        transcript: Some(transcript),
        ..photo(dir.path(), "IMG_0001.jpg", "photo")
    };
    let path = dir.path().join("p.notebook");
    project::save(&path, &Pages::in_order(vec![page]), &[]).unwrap();
    insta::assert_snapshot!(String::from_utf8(project::json(&path).unwrap()).unwrap());
}

#[test]
fn restyling_reads_claudes_answer_again() {
    let style = MarkdownStyle {
        underline: "*".to_owned(),
        underlined_line_heading: 3,
    };
    let fresh = recorded("success");
    let restyled = fresh.restyled(&style).unwrap();
    assert_eq!(restyled.style, style);
    assert_eq!(restyled.restyled(&fresh.style).unwrap(), fresh);

    let made_up = Transcript {
        source: None,
        ..fresh
    };
    assert!(made_up.restyled(&style).is_err());
}
