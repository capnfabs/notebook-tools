//! The app's UI, driven through egui_kittest as a user would: by clicking, typing, dragging and
//! dropping, with stand-ins for Claude, the cache and native dialogs. See
//! `decisions/2026-10-23-testing-approach.md`.

mod common;

mod api_key;
mod corners;
mod corrections;
mod export;
mod focus;
mod keys;
mod missing;
mod pages;
mod project;
mod settings;
mod status;
mod transcription;
