//! The egui desktop app (`decisions/2026-10-23-egui-desktop-native-mobile-shell.md`; desktop only
//! since `decisions/2026-10-24-drop-wasm.md`). [`NotebookApp`] (in [`app`]) is the UI; the modules beside it do its work
//! off the UI thread ([`ingest`], [`transcriptions`], [`export`]), turn key presses and the macOS
//! menu into [`Command`]s, and hold the user-facing text ([`i18n`]). Everything that isn't UI is in
//! `notebook_core`. See `decisions/2026-10-31-code-layout.md`.

pub mod app;
#[cfg(target_os = "macos")]
mod app_delegate;
pub mod auth;
pub mod cli;
pub mod command;
pub mod eval_log;
mod export;
#[cfg(target_os = "macos")]
pub mod finder_open;
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
pub use ingest::{Load, OpenedFiles, Pick, PickFiles};
pub use transcriptions::{Fetch, OnTranscribeCompleted, Transcriber};

use eframe::egui;

/// The window's options. The icon is left empty, so eframe leaves the app's own icon alone, where
/// it would otherwise replace it with egui's while the app runs: the bundle's on macOS
/// (`decisions/2026-11-15-macos-bundle.md`), the generic one when run with `cargo run`.
pub fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_icon(egui::IconData::default()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    /// eframe takes an empty icon to mean the app's own.
    #[test]
    fn the_window_keeps_the_apps_icon() {
        let icon = super::native_options().viewport.icon.unwrap();
        assert_eq!(*icon, eframe::egui::IconData::default());
    }
}
