//! Transcribed pages as files to save.

use crate::ocr::Transcript;

/// `pages` as one markdown document, in order. Each page starts with an HTML comment naming its
/// file, which doesn't show when rendered.
pub fn markdown(pages: &[(&str, &Transcript)]) -> String {
    let pages: Vec<String> = pages
        .iter()
        .map(|(name, transcript)| format!("<!-- {name} -->\n{}\n", transcript.markdown()))
        .collect();
    pages.join("\n")
}
