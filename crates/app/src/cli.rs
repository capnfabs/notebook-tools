//! The command line: `notebook-tools [project.notebook]` starts with the project open. Anything
//! else is an error in the terminal, before the window opens, so an agent or script driving the
//! app finds out at once. See `decisions/2026-10-31-open-by-drop-and-argument.md`.

use std::ffi::OsString;
use std::path::PathBuf;

use notebook_core::project::{self, Opened};

use crate::ingest::is_project;
use crate::t;

/// The project the command line `args` (after the program's name) name, opened, if any; or what's
/// wrong with them.
pub fn project_to_open(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Option<(PathBuf, Opened)>, String> {
    let path = match args.into_iter().collect::<Vec<_>>().as_slice() {
        [] => return Ok(None),
        [arg] => PathBuf::from(arg),
        _ => return Err(t!("cli-usage")),
    };
    if !is_project(&path) {
        return Err(t!("cli-not-a-project", path = path.display().to_string()));
    }
    match project::open(&path) {
        Ok(opened) => Ok(Some((path, opened))),
        Err(err) => Err(crate::i18n::open_project_error(&path, &err)),
    }
}
