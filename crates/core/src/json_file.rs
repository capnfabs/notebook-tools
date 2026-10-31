//! Writing settings, cache entries and projects as JSON files.

use std::io;
use std::path::Path;

use serde::Serialize;

/// Writes `value` to `path` as pretty JSON, making its directory if needed. Writes a temporary file
/// beside it first and renames it into place, so a crash or full disk leaves the old file whole.
pub(crate) fn write(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_whole_files_and_leaves_no_temporary_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new/settings.json");
        write(&path, &[1, 2]).unwrap();
        write(&path, &"replaced").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "\"replaced\"");
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(files, 1, "only the file itself");
    }

    #[test]
    fn a_failed_write_leaves_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        write(&path, &1).unwrap();
        // A map with non-string keys can't be JSON.
        let bad = std::collections::HashMap::from([((1, 2), 3)]);
        assert!(write(&path, &bad).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "1");
    }
}
