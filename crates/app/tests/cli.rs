//! The command line: `notebook-tools [project.notebook | --clear-caches-only]`. See
//! `decisions/2026-10-31-open-by-drop-and-argument.md` and
//! `decisions/2026-11-01-manual-cache-clearing.md`. The project opening is tested in
//! `viewer/project.rs`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notebook_app::cli::{CLEAR_CACHES, Launch, clear_caches, launch};
use notebook_app::t;
use notebook_core::page::{Page, Pages};
use notebook_core::project;
use tempfile::TempDir;

fn test_dir(test: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(&format!("cli-{test}-"))
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
}

/// What's wrong with the command line `args`.
fn error(args: &[&Path]) -> String {
    let args: Vec<OsString> = args.iter().map(|a| a.as_os_str().to_owned()).collect();
    match launch(args) {
        Ok(Launch::Open(opened)) => panic!("opened {:?}", opened.map(|(path, _)| path)),
        Ok(Launch::ClearCaches) => panic!("cleared the caches"),
        Err(err) => err,
    }
}

/// The project the command line `args` open, if any.
fn opened(args: impl IntoIterator<Item = OsString>) -> Option<(PathBuf, project::Opened)> {
    match launch(args) {
        Ok(Launch::Open(opened)) => opened,
        Ok(Launch::ClearCaches) => panic!("cleared the caches"),
        Err(err) => panic!("{err}"),
    }
}

/// A project at `dir/notes.notebook` with one page.
fn saved_project(dir: &Path) -> PathBuf {
    let photo = dir.join("a.png");
    std::fs::write(&photo, "a photo").unwrap();
    let mut pages = Pages::default();
    pages.add(Page::new("a.png".into(), Arc::from(&b"a photo"[..])).at(photo));
    let path = dir.join("notes.notebook");
    project::save(&path, &pages, &[]).unwrap();
    path
}

#[test]
fn no_arguments_open_nothing() {
    assert!(opened(Vec::<OsString>::new()).is_none());
}

#[test]
fn a_project_opens() {
    let dir = test_dir("opens");
    let path = saved_project(dir.path());
    let (opened_path, opened) = opened([path.clone().into()]).unwrap();
    assert_eq!(opened_path, path);
    assert_eq!(opened.pages.len(), 1);
}

#[test]
fn anything_else_is_an_error() {
    let dir = test_dir("errors");
    let project = saved_project(dir.path());
    let photo = dir.path().join("a.png");
    assert_eq!(
        error(&[&photo]),
        t!("cli-not-a-project", path = photo.display().to_string())
    );
    assert_eq!(error(&[&project, &project]), t!("cli-usage"));

    let missing = dir.path().join("gone.notebook");
    let open_failed = t!(
        "project-open-failed",
        path = missing.display().to_string(),
        error = ""
    );
    assert!(
        error(&[&missing]).starts_with(&open_failed),
        "{}",
        error(&[&missing])
    );

    let damaged = dir.path().join("damaged.notebook");
    std::fs::write(&damaged, "not a project").unwrap();
    let not_a_project = t!(
        "project-not-a-project",
        path = damaged.display().to_string(),
        error = ""
    );
    assert!(error(&[&damaged]).starts_with(&not_a_project));
}

#[test]
fn the_app_says_whats_wrong_and_exits_before_opening_a_window() {
    let dir = test_dir("exits");
    let missing = dir.path().join("gone.notebook");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_notebook-tools"))
        .arg(&missing)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.trim_end(), error(&[&missing]));
}

#[test]
fn clear_caches_only_must_be_alone() {
    assert!(matches!(
        launch([OsString::from(CLEAR_CACHES)]),
        Ok(Launch::ClearCaches)
    ));
    let dir = test_dir("clear_alone");
    let project = saved_project(dir.path());
    assert_eq!(error(&[Path::new(CLEAR_CACHES), &project]), t!("cli-usage"));
}

#[test]
fn clearing_the_caches_says_what_went() {
    let dir = test_dir("clear");
    let cache = dir.path().join("transcriptions");
    std::fs::create_dir_all(&cache).unwrap();
    for name in ["a", "b"] {
        let entry = format!(
            "{}-v1-opus.json",
            notebook_core::cache::sha256_hex(name.as_bytes())
        );
        std::fs::write(cache.join(entry), "{}").unwrap();
    }
    let shown = cache.display().to_string();
    assert_eq!(
        clear_caches(Some(cache.clone())),
        Ok(t!("cli-cache-cleared", count = 2, dir = shown.as_str()))
    );
    assert!(!cache.exists());
    assert_eq!(
        clear_caches(Some(cache.clone())),
        Ok(t!("cli-no-cache", dir = shown.as_str()))
    );
    assert_eq!(clear_caches(None), Err(t!("cli-no-cache-dir")));
}
