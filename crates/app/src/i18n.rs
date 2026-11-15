//! Everything the app says to the user is in `i18n/en-US/notebook_app.ftl`, looked up with [`t!`],
//! which checks message ids and arguments at compile time. egui draws every frame, so text is looked
//! up afresh each frame. Errors from `notebook_core` are worded here too, from their variants.
//!
//! See `decisions/2026-10-30-strings-in-fluent.md`.

use std::path::Path;
use std::sync::LazyLock;

use i18n_embed::LanguageLoader;
use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use notebook_core::claude_transcribe::OcrError;
use notebook_core::decode::DecodeError;
use notebook_core::export::PdfError;
use notebook_core::project::{Missing, OpenError, SaveError, Why};
use rust_embed::RustEmbed;

#[doc(hidden)]
pub use i18n_embed_fl::fl;

/// The message `id` from `notebook_app.ftl`, given its arguments: `t!("open-failed", name = n,
/// error = e)`.
#[macro_export]
macro_rules! t {
    // `fl!` writes `value.into()`, so a value like `n + 1` needs brackets.
    ($id:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::fl!($crate::i18n::LOADER, $id $(, $name = ($value))*)
    };
}

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

pub static LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader = fluent_language_loader!();
    loader
        .load_fallback_language(&Localizations)
        .expect("the bundled messages load");
    // Fluent marks where each argument starts and ends, for right-to-left text; egui would draw the
    // marks.
    loader.set_use_isolating(false);
    loader
});

/// Why the page couldn't be transcribed.
pub fn ocr_error(err: &OcrError) -> String {
    match err {
        OcrError::Decode(error) => t!("ocr-decode", error = error.as_str()),
        OcrError::Network(error) => t!("ocr-network", error = error.as_str()),
        OcrError::Api {
            status,
            kind,
            message,
        } => t!(
            "ocr-api",
            status = *status,
            kind = kind.as_str(),
            message = message.as_str()
        ),
        OcrError::Refusal {
            category,
            explanation,
        } => match (category, explanation) {
            (None, None) => t!("ocr-refused"),
            (Some(category), None) => t!("ocr-refused-category", category = category.as_str()),
            (None, Some(explanation)) => {
                t!(
                    "ocr-refused-explanation",
                    explanation = explanation.as_str()
                )
            }
            (Some(category), Some(explanation)) => t!(
                "ocr-refused-category-explanation",
                category = category.as_str(),
                explanation = explanation.as_str()
            ),
        },
        OcrError::MaxTokens => t!("ocr-max-tokens"),
        OcrError::Parse(error) => t!("ocr-parse", error = error.as_str()),
    }
}

/// Why a photo couldn't be opened.
pub fn decode_error(err: &DecodeError) -> String {
    match err {
        DecodeError::Unrecognized => t!("decode-unrecognized"),
        DecodeError::Image(error) => error.to_string(),
        DecodeError::Heic(error) => t!("decode-heic", error = error.as_str()),
    }
}

/// Why the project at `path` couldn't be opened.
pub fn open_project_error(path: &Path, err: &OpenError) -> String {
    let path = path.display().to_string();
    match err {
        OpenError::Io(error) => t!(
            "project-open-failed",
            path = path,
            error = error.to_string()
        ),
        OpenError::Format(error) => {
            t!("project-not-a-project", path = path, error = error.as_str())
        }
        OpenError::NewerVersion(_) => t!("project-newer-version", path = path),
    }
}

/// Why the project couldn't be saved to `path`.
pub fn save_project_error(path: &Path, err: &SaveError) -> String {
    match err {
        SaveError::Io(error) => t!(
            "project-save-failed",
            path = path.display().to_string(),
            error = error.to_string()
        ),
        SaveError::NoPath(names) => {
            t!(
                "project-no-path",
                names = names.join(", "),
                count = names.len()
            )
        }
    }
}

/// Why a page's photo wasn't found when its project opened.
pub fn missing_photo(missing: &Missing) -> String {
    let name = missing.page.name.as_str();
    let path = missing.page.photo.path.display().to_string();
    match &missing.why {
        Why::NotFound => t!("photo-not-found", name = name, path = path),
        Why::Changed => t!("photo-changed", name = name, path = path),
        Why::Unreadable(error) => {
            t!(
                "photo-unreadable",
                name = name,
                path = path,
                error = error.as_str()
            )
        }
    }
}

/// Why the PDF couldn't be made.
pub fn pdf_error(err: &PdfError) -> String {
    match err {
        PdfError::EmptyImage { page } => t!("pdf-empty-image", page = page.as_str()),
        PdfError::Embed { page, error } => {
            t!(
                "pdf-embed-failed",
                page = page.as_str(),
                error = error.as_str()
            )
        }
        PdfError::Build(error) => t!("pdf-build-failed", error = error.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Message ids in `notebook_app.ftl`.
    fn message_ids() -> Vec<String> {
        let ftl = Path::new(env!("CARGO_MANIFEST_DIR")).join("i18n/en-US/notebook_app.ftl");
        let ftl = std::fs::read_to_string(ftl).unwrap();
        ftl.lines()
            .filter_map(|line| {
                let (id, _) = line.split_once(" =")?;
                let id_like = id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
                (id_like && !id.is_empty()).then(|| id.to_owned())
            })
            .collect()
    }

    /// The app's source, without its unit tests.
    fn source(dir: &Path, into: &mut String) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                source(&path, into);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let code = std::fs::read_to_string(path).unwrap();
                into.push_str(code.split("#[cfg(test)]").next().unwrap_or_default());
            }
        }
    }

    /// So the file holds only what's shown, and stays the one place to revise it.
    #[test]
    fn every_message_is_used() {
        let mut code = String::new();
        source(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut code,
        );
        let ids = message_ids();
        assert!(ids.len() > 100, "found only {} messages", ids.len());
        let unused: Vec<&String> = ids
            .iter()
            .filter(|id| !code.contains(&format!("\"{id}\"")))
            .collect();
        assert!(unused.is_empty(), "unused messages: {unused:?}");
    }

    #[test]
    fn arguments_are_not_wrapped_in_isolation_marks() {
        let text = t!("open-failed", name = "a.png", error = "oops");
        assert_eq!(text, "Couldn't open a.png: oops");
    }

    #[test]
    fn plurals_agree_with_the_count() {
        assert_eq!(t!("status-transcribing", count = 1), "Transcribing 1 page…");
        assert_eq!(
            t!("status-transcribing", count = 3),
            "Transcribing 3 pages…"
        );
        assert_eq!(
            t!("transcript-lines-cut-off", count = 1),
            "The bounds of this page have shrunk since last transcription; \
             1 transcribed line falls outside this page's new bounds."
        );
        assert_eq!(
            t!("export-copied", count = 1, name = "a.png"),
            "Copied a.png to the clipboard"
        );
        assert_eq!(
            t!("export-copied", count = 2, name = ""),
            "Copied 2 pages to the clipboard"
        );
    }

    #[test]
    fn messages_can_name_commands() {
        assert!(t!("settings-future-only-hint").contains("\"Reset Transcript\""));
    }

    #[test]
    fn core_errors_are_worded() {
        let refusal = OcrError::Refusal {
            category: Some("cyber".to_owned()),
            explanation: None,
        };
        assert_eq!(
            ocr_error(&refusal),
            "Claude declined to transcribe this page (cyber)"
        );
        let api = OcrError::Api {
            status: 529,
            kind: "overloaded_error".to_owned(),
            message: "Overloaded".to_owned(),
        };
        assert_eq!(
            ocr_error(&api),
            "Claude API error 529 (overloaded_error): Overloaded"
        );
        assert_eq!(
            decode_error(&DecodeError::Unrecognized),
            "not a JPEG, PNG or HEIC image"
        );
    }
}
