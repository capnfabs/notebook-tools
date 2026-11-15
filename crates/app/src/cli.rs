//! The command line: `notebook-tools [--offline] [project.notebook]` starts with the project open,
//! and with `--offline`, without any credentials, so nothing is sent to Claude; `notebook-tools
//! --clear-caches-only` clears the transcription cache and exits. Development builds also take
//! `--no-keychain`. Anything else is an error in the terminal, before the window opens, so an agent
//! or script driving the app finds out at once. See
//! `decisions/2026-10-31-open-by-drop-and-argument.md`,
//! `decisions/2026-11-01-manual-cache-clearing.md` and
//! `decisions/2026-11-15-ant-login-fallback.md`.

use std::ffi::OsString;
use std::path::PathBuf;

use notebook_core::cache::Cache;
use notebook_core::project::{self, Opened};

use crate::ingest::is_project;
use crate::t;

/// Clears the cache and exits, without opening a window.
pub const CLEAR_CACHES: &str = "--clear-caches-only";
/// Starts without credentials: cached transcripts load, but nothing is sent.
pub const OFFLINE: &str = "--offline";
/// Development builds only: never touches the keychain, so macOS doesn't ask for access to it
/// again with every build. Credentials come from the environment or `ant`.
pub const NO_KEYCHAIN: &str = "--no-keychain";

/// How an argument macOS adds when Finder launches the app starts (`-psn_0_12345`). Ignored.
pub const PROCESS_SERIAL_NUMBER: &str = "-psn_";

/// What the command line asks for.
pub enum Launch {
    /// Start the app, with `project` open, if any; without credentials if `offline`, and without
    /// the keychain if `no_keychain`.
    Open {
        project: Option<(PathBuf, Opened)>,
        offline: bool,
        no_keychain: bool,
    },
    /// Clear the caches with [`clear_caches`], and exit.
    ClearCaches,
}

/// What the command line `args` (after the program's name) ask for, with the project they name
/// opened; or what's wrong with them.
pub fn launch(args: impl IntoIterator<Item = OsString>) -> Result<Launch, String> {
    let mut args: Vec<OsString> = args.into_iter().collect();
    // Older macOS names the process this way when Finder launches the app.
    args.retain(|arg| !arg.to_string_lossy().starts_with(PROCESS_SERIAL_NUMBER));
    if let [arg] = args.as_slice()
        && arg == CLEAR_CACHES
    {
        return Ok(Launch::ClearCaches);
    }
    let offline = take_flag(&mut args, OFFLINE)?;
    let no_keychain = take_flag(&mut args, NO_KEYCHAIN)?;
    if no_keychain && !cfg!(debug_assertions) {
        return Err(t!("cli-no-keychain-release"));
    }
    let project = match args.as_slice() {
        [] => None,
        [arg] if arg != CLEAR_CACHES => Some(open(PathBuf::from(arg))?),
        _ => return Err(t!("cli-usage")),
    };
    Ok(Launch::Open {
        project,
        offline,
        no_keychain,
    })
}

/// Whether `args` has `flag`, taking it out; an error if it's there twice.
fn take_flag(args: &mut Vec<OsString>, flag: &str) -> Result<bool, String> {
    match args.iter().filter(|arg| *arg == flag).count() {
        0 => Ok(false),
        1 => {
            args.retain(|arg| arg != flag);
            Ok(true)
        }
        _ => Err(t!("cli-usage")),
    }
}

/// The project at `path`, opened; or why it can't be.
fn open(path: PathBuf) -> Result<(PathBuf, Opened), String> {
    if !is_project(&path) {
        return Err(t!("cli-not-a-project", path = path.display().to_string()));
    }
    match project::open(&path) {
        Ok(opened) => Ok((path, opened)),
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
