//! What the user chose in Settings, kept as JSON in their config directory.
//!
//! See `decisions/2026-10-30-settings-dialog.md`.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::llm_model::Model;
use crate::markdown_style::MarkdownStyle;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Which Claude new and forced transcriptions go to.
    pub model: Model,
    /// How Claude's markup is written in transcripts.
    pub markdown: MarkdownStyle,
}

impl Settings {
    /// In the user's config directory: `~/Library/Application Support` on macOS, `~/.config` on
    /// Linux, `%APPDATA%` on Windows.
    pub fn default_path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("notebook-tools/settings.json"))
    }

    /// The settings saved at `path`. Defaults if there are none, or they can't be read; a setting
    /// missing from the file has its default.
    pub fn load(path: &Path) -> Self {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                log::warn!("couldn't read settings {}: {err}", path.display());
                return Self::default();
            }
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            log::warn!("ignoring settings {}: {err}", path.display());
            Self::default()
        })
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("nested/settings.json")
    }

    #[test]
    fn saved_settings_load_back() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            model: Model::Sonnet,
            markdown: MarkdownStyle {
                underline: "*".to_owned(),
                underlined_line_heading: 3,
            },
        };
        settings.save(&path(&dir)).unwrap();
        assert_eq!(Settings::load(&path(&dir)), settings);
    }

    #[test]
    fn missing_or_unreadable_settings_are_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(&path(&dir)), Settings::default());
        std::fs::create_dir_all(path(&dir).parent().unwrap()).unwrap();
        std::fs::write(path(&dir), "not json").unwrap();
        assert_eq!(Settings::load(&path(&dir)), Settings::default());
    }

    #[test]
    fn missing_settings_have_their_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(path(&dir).parent().unwrap()).unwrap();
        std::fs::write(
            path(&dir),
            r#"{"model": "sonnet", "markdown": {"underline": "*"}}"#,
        )
        .unwrap();
        let settings = Settings::load(&path(&dir));
        assert_eq!(settings.model, Model::Sonnet);
        assert_eq!(settings.markdown.underline, "*");
        assert_eq!(
            settings.markdown.underlined_line_heading,
            MarkdownStyle::default().underlined_line_heading
        );
    }
}
