//! Writing settings and cache entries as JSON files.

use std::io;
use std::path::Path;

use serde::Serialize;

/// Writes `value` to `path` as pretty JSON, making its directory if needed.
pub(crate) fn write(path: &Path, value: &impl Serialize) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(value)?)
}
