//! Writing settings, cache entries and projects as JSON files, projects gzipped.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Serialize;

/// Writes `value` to `path` as pretty JSON, making its directory if needed. Writes a temporary file
/// beside it first and renames it into place, so a crash or full disk leaves the old file whole.
pub(crate) fn write(path: &Path, value: &impl Serialize) -> io::Result<()> {
    write_with(path, |file| Ok(serde_json::to_writer_pretty(file, value)?))
}

/// [`write`], gzipped.
pub(crate) fn write_gzipped(path: &Path, value: &impl Serialize) -> io::Result<()> {
    write_with(path, |file| {
        let mut gz = GzEncoder::new(file, Compression::default());
        serde_json::to_writer_pretty(&mut gz, value)?;
        gz.finish()?.flush()
    })
}

/// Writes `path` atomically, as [`write`] does, with `contents` filling the temporary file.
fn write_with(path: &Path, contents: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    contents(file.as_file_mut())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

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
        assert!(write_gzipped(&path, &bad).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "1");
    }

    #[test]
    fn gzipped_files_hold_pretty_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.notebook");
        write_gzipped(&path, &[1, 2]).unwrap();
        let mut json = String::new();
        flate2::read::GzDecoder::new(File::open(&path).unwrap())
            .read_to_string(&mut json)
            .unwrap();
        assert_eq!(json, "[\n  1,\n  2\n]");
    }
}
