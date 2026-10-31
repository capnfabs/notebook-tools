//! The command line: `notebook-tools [project.notebook]`. See
//! `decisions/2026-10-31-open-by-drop-and-argument.md`. The project opening is tested in
//! `viewer/project.rs`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notebook_app::cli::project_to_open;
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
    match project_to_open(args) {
        Ok(opened) => panic!("opened {:?}", opened.map(|(path, _)| path)),
        Err(err) => err,
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
    assert!(project_to_open(Vec::<OsString>::new()).unwrap().is_none());
}

#[test]
fn a_project_opens() {
    let dir = test_dir("opens");
    let path = saved_project(dir.path());
    let (opened_path, opened) = project_to_open([path.clone().into()]).unwrap().unwrap();
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
