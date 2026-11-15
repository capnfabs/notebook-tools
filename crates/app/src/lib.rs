//! The egui desktop app (`decisions/2026-10-23-egui-desktop-native-mobile-shell.md`; desktop only
//! since `decisions/2026-10-24-drop-wasm.md`). [`NotebookApp`] (in [`app`]) is the UI; the modules beside it do its work
//! off the UI thread ([`ingest`], [`transcriptions`], [`export`]), turn key presses and the macOS
//! menu into [`Command`]s, and hold the user-facing text ([`i18n`]). Everything that isn't UI is in
//! `notebook_core`. See `decisions/2026-10-31-code-layout.md`.

pub mod app;
pub mod auth;
pub mod cli;
pub mod command;
pub mod eval_log;
mod export;
pub mod i18n;
mod ingest;
mod jobs;
#[cfg(target_os = "macos")]
pub mod menu;
#[cfg(target_os = "macos")]
pub mod quit;
mod timing;
mod transcriptions;
mod ui;

pub use app::NotebookApp;
pub use auth::{Auth, CheckKey, MakeTranscriber};
pub use command::Command;
pub use eval_log::{EvalLog, Event, Via};
pub use export::SavePath;
pub use ingest::{Load, Pick, PickFiles};
pub use transcriptions::{Fetch, OnTranscribeCompleted, Transcriber};
