//! Everything notebook-tools does that isn't UI: decoding and cleaning photos, transcribing them
//! with Claude, caching and correcting transcripts, and exporting them. The app imports items by
//! their module path, e.g. `notebook_core::page::Page`; nothing is re-exported here. No UI types,
//! so a native mobile shell could use it too (`decisions/2026-10-23-egui-desktop-native-mobile-shell.md`).
//! See `decisions/2026-10-31-code-layout.md`.

pub mod cache;
pub mod claude_transcribe;
pub mod clean;
pub mod decode;
pub mod export;
pub mod geom;
pub mod imaging;
mod json_file;
pub mod llm_model;
pub mod loaded;
pub mod markdown_style;
pub mod page;
pub mod project;
pub mod reframe;
pub mod settings;
#[cfg(test)]
mod test_images;
pub mod transcript;
