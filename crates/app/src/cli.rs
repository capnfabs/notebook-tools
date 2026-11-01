//! The command line: `notebook-tools [project.notebook]` starts with the project open, and
//! `notebook-tools --clear-caches-only` clears the transcription cache and exits. Anything else is
//! an error in the terminal, before the window opens, so an agent or script driving the app finds
//! out at once. See `decisions/2026-10-31-open-by-drop-and-argument.md` and
//! `decisions/2026-11-01-manual-cache-clearing.md`.

use std::ffi::OsString;
use std::path::PathBuf;

use notebook_core::cache::Cache;
use notebook_core::project::{self, Opened};

use crate::ingest::is_project;
use crate::t;

/// Clears the cache and exits, without opening a window.
pub const CLEAR_CACHES: &str = "--clear-caches-only";

/// What the command line asks for.
pub enum Launch {
    /// Start the app, with this project open, if any.
    Open(Option<(PathBuf, Opened)>),
    /// Clear the caches with [`clear_caches`], and exit.
    ClearCaches,
}

/// What the command line `args` (after the program's name) ask for, with the project they name
/// opened; or what's wrong with them.
pub fn launch(args: impl IntoIterator<Item = OsString>) -> Result<Launch, String> {
    let path = match args.into_iter().collect::<Vec<_>>().as_slice() {
        [] => return Ok(Launch::Open(None)),
        [arg] if arg == CLEAR_CACHES => return Ok(Launch::ClearCaches),
        [arg] => PathBuf::from(arg),
        _ => return Err(t!("cli-usage")),
    };
    if !is_project(&path) {
        return Err(t!("cli-not-a-project", path = path.display().to_string()));
    }
    match project::open(&path) {
        Ok(opened) => Ok(Launch::Open(Some((path, opened)))),
        Err(err) => Err(crate::i18n::open_project_error(&path, &err)),
    }
}

/// Removes every cached transcription in `dir`, the cache directory if the system has one. Says
/// what was done, or what went wrong.
pub fn clear_caches(dir: Option<PathBuf>) -> Result<String, String> {
    let Some(dir) = dir else {
        return Err(t!("cli-no-cache-dir"));
    };
    let shown = dir.display().to_string();
    if !dir.exists() {
        return Ok(t!("cli-no-cache", dir = shown));
    }
    match Cache::new(&dir).clear() {
        Ok(count) => Ok(t!("cli-cache-cleared", count = count, dir = shown)),
        Err(err) => Err(t!("cli-clear-failed", dir = shown, error = err.to_string())),
    }
}
