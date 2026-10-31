//! The app: [`NotebookApp`] holds the pages and everything about how they're shown, and draws
//! the UI each frame. Its methods are spread over this module's files by what they do:
//!
//! - [`commands`]: what each [`Command`](crate::command::Command) can do now, and doing it.
//! - [`pages`]: adding, loading, deleting, moving and transcribing pages, and the dialogs that
//!   confirm those.
//! - [`views`]: the selected page in the Original, Cleaned and Transcribed views.
//! - [`transcript_panel`]: the transcript's status and transcription progress.
//! - [`chrome`]: the drop zone, toolbar, page strip and status line.
//! - [`textures`]: the images those show, decoded off the UI thread.
//!
//! See `decisions/2026-10-31-code-layout.md`.

mod chrome;
mod commands;
mod pages;
mod textures;
mod transcript_panel;
mod views;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use notebook_core::cache::Cache;
use notebook_core::clean::CleanParams;
use notebook_core::llm_model::Model;
use notebook_core::page::Pages;
use notebook_core::settings::Settings;
use notebook_core::transcript::Transcript;
use textures::Textures;

use crate::command::{EguiKeys, Frontend};
use crate::eval_log::{EvalLog, Event};
use crate::export::{Export, SavePath};
use crate::ingest::{Ingest, Load, PickFiles};
use crate::jobs::Waker;
use crate::t;
use crate::transcriptions::{Transcriber, Transcriptions};
use crate::ui::corners::Corners;
use crate::ui::settings::SettingsDialog;
use crate::ui::split::Split;
use crate::ui::thumbnails::Thumbnails;
use crate::ui::transcript_view::TranscriptView;

/// Frames taking longer than this, in seconds, are logged.
const SLOW_FRAME_SECS: f32 = 0.05;

/// How long a hint stays in the status line, in seconds.
const HINT_SECONDS: f64 = 4.0;

/// Which version of the selected page the viewer shows.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum View {
    #[default]
    Original,
    /// The original with the page's corners to drag, beside the page cleaned with them.
    Cleaned,
    /// Claude's transcription beside the cleaned page, for correcting. Switching to it transcribes
    /// the page shown, if it isn't already; selecting another page doesn't.
    Transcribed,
}

/// Where the user's commands come from: egui's key presses unless there's a native menu bar.
struct Input(Box<dyn Frontend>);

impl Default for Input {
    fn default() -> Self {
        Self(Box::new(EguiKeys))
    }
}

#[derive(Default)]
pub struct NotebookApp {
    ingest: Ingest,
    pages: Pages,
    /// Name of the page the user picked. The first page is shown until they pick one.
    selected: Option<String>,
    /// The strip of page thumbnails.
    thumbnails: Thumbnails,
    /// Stays the same when another page is selected.
    view: View,
    textures: Textures,
    clean_params: CleanParams,
    /// The page's corners in the Cleaned view.
    corners: Corners,
    /// Between the photo and the cleaned page in the Cleaned view.
    cleaned_split: Split,
    errors: Vec<String>,
    transcriptions: Transcriptions,
    transcript_view: TranscriptView,
    /// Between the page and its transcript in the Transcribed view.
    transcript_split: Split,
    export: Export,
    /// The transcribed page the user asked to delete, until they confirm.
    confirm_delete: Option<String>,
    /// The user asked to transcribe more than [`pages::TRANSCRIBE_ALL_CONFIRM_ABOVE`] pages, until
    /// they confirm.
    confirm_transcribe_all: bool,
    /// The corrected page the user asked to retranscribe, until they confirm.
    confirm_retranscribe: Option<String>,
    /// The page whose corrections the user asked to drop, until they confirm.
    confirm_reset: Option<String>,
    settings_dialog: SettingsDialog,
    /// Where settings are saved. Without one, they last until the app quits.
    settings_path: Option<PathBuf>,
    eval_log: EvalLog,
    input: Input,
    /// Asks which files to add as pages. Without one, Add Pages… is disabled.
    pick_files: Option<PickFiles>,
    /// The file picker was opened since [`Self::frame_time_for_logs`] last looked.
    asked_which_files: bool,
    /// A passing note for the status line, and the time (`InputState::time`) it goes.
    hint: Option<(String, f64)>,
}

impl NotebookApp {
    /// Without a transcriber, only cached transcripts show; without a cache either, the
    /// Transcribed view is disabled. Without a cache, every page is sent to Claude.
    pub fn with_transcription(transcriber: Option<Transcriber>, cache: Option<Cache>) -> Self {
        Self {
            transcriptions: Transcriptions::new(transcriber, cache),
            ..Self::default()
        }
    }

    /// Without one, exporting to a file does nothing.
    pub fn with_save_path(self, save_path: SavePath) -> Self {
        Self {
            export: Export::new(Some(save_path)),
            ..self
        }
    }

    /// Asks which files to add with `pick_files`, for Add Pages….
    pub fn with_file_picker(self, pick_files: PickFiles) -> Self {
        Self {
            pick_files: Some(pick_files),
            ..self
        }
    }

    /// Starts with the settings saved at `path`, and saves them there when they change.
    pub fn with_settings_file(mut self, path: PathBuf) -> Self {
        self.transcriptions.set_settings(Settings::load(&path));
        Self {
            settings_path: Some(path),
            ..self
        }
    }

    /// Takes commands from `frontend`, such as the native menu bar, instead of egui's key presses.
    pub fn with_frontend(self, frontend: Box<dyn Frontend>) -> Self {
        Self {
            input: Input(frontend),
            ..self
        }
    }

    /// Loads dropped pages with `load` rather than [`notebook_core::loaded::Loaded::new`].
    pub fn with_loader(self, load: Load) -> Self {
        Self {
            ingest: Ingest::new(load),
            ..self
        }
    }

    /// Whether any page is waiting to be transcribed or being transcribed.
    pub fn transcribing(&self) -> bool {
        self.transcriptions.pending() > 0
    }

    /// Whether any page is loading, or any image is being decoded for the viewer.
    pub fn loading(&self) -> bool {
        self.ingest.pending() > 0 || self.textures.decoding()
    }

    /// How long the last frame took, given its `cpu_usage` in seconds, if it was slow enough to log
    /// and not because it waited for the user in the save dialog or file picker. Call once per
    /// frame: it forgets the dialog was opened.
    pub fn frame_time_for_logs(&mut self, cpu_usage: Option<f32>) -> Option<f32> {
        let waited = self.export.take_asked_where() | std::mem::take(&mut self.asked_which_files);
        cpu_usage.filter(|&secs| secs > SLOW_FRAME_SECS && !waited)
    }

    /// What's been transcribed and corrected this session, for judging the models.
    pub fn eval_log(&self) -> &EvalLog {
        &self.eval_log
    }

    /// The app's UI, separate from [`eframe::App`] so tests can drive it directly.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // Set here because the context only exists once the UI runs.
        if !self.transcriptions.has_waker() {
            let ctx = ctx.clone();
            let waker: Waker = Arc::new(move || ctx.request_repaint());
            self.transcriptions.set_waker(waker.clone());
            self.ingest.set_waker(waker.clone());
            self.textures.set_waker(waker);
        }
        self.run_commands(&ctx);
        self.add_dropped(&ctx);
        self.add_loaded(&ctx);
        self.textures.poll(&ctx);
        self.export.poll(&ctx);
        self.take_transcriptions();

        if self.pages.is_empty() {
            self.show_drop_zone(ui);
        } else {
            self.show_viewer(ui);
        }
        self.show_settings_dialog(&ctx);
        let states = self.command_states(&ctx);
        self.input.0.sync(&|c| states[c as usize].clone());
    }

    /// Gives pages the transcripts that came back since the last frame.
    fn take_transcriptions(&mut self) {
        while let Some(done) = self.transcriptions.next() {
            if done.restyled {
                self.restyled(done.name, done.transcript);
                continue;
            }
            // The page may have been deleted meanwhile. Only a forced retranscription replaces a
            // transcript: a late cache hit mustn't undo the user's corrections.
            if let Some(page) = self.pages.find_mut(&done.name)
                && (done.forced || page.transcript.is_none())
            {
                let event = Event::transcribed(&done.name, &done.transcript, done.elapsed);
                self.eval_log.record(event);
                page.transcript = Some(done.transcript);
                if done.forced {
                    self.transcript_view = TranscriptView::default();
                }
                self.restyle_if_stale(&done.name);
            }
        }
    }

    /// Shows `text` in the status line for a few seconds.
    fn show_hint(&mut self, ctx: &egui::Context, text: String) {
        let now = ctx.input(|i| i.time);
        self.hint = Some((text, now + HINT_SECONDS));
    }

    /// The Settings dialog, if it's open. Saved settings apply from now on, and restyle the
    /// transcripts the user hasn't corrected.
    fn show_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(settings) = self.settings_dialog.show(ctx) else {
            return;
        };
        if let Some(path) = &self.settings_path
            && let Err(err) = settings.save(path)
        {
            log::warn!("couldn't save settings to {}: {err}", path.display());
            let error = t!("settings-save-failed", error = err.to_string());
            self.errors.push(error);
        }
        self.transcriptions.set_settings(settings);
        let names: Vec<String> = self.pages.iter().map(|p| p.name.clone()).collect();
        for name in names {
            self.restyle_if_stale(&name);
        }
    }

    /// Reads the page named `name`'s transcript from the cache again if it's in another style than
    /// the settings', unless the user has corrected it: that would lose their corrections.
    fn restyle_if_stale(&mut self, name: &str) {
        let style = &self.transcriptions.settings().markdown;
        let Some(page) = self.pages.find(name) else {
            return;
        };
        let Some(transcript) = &page.transcript else {
            return;
        };
        if transcript.corrected() || transcript.style == *style {
            return;
        }
        let model = Model::from_id(&transcript.model).unwrap_or_default();
        let bytes = page.bytes.clone();
        self.transcriptions.restyle(name, bytes, model);
    }

    /// Puts `transcript`, read again in the settings' style, in place of the page named `name`'s,
    /// unless the user corrected it meanwhile or the style changed again.
    fn restyled(&mut self, name: String, transcript: Transcript) {
        let style = &self.transcriptions.settings().markdown;
        let Some(page) = self.pages.find_mut(&name) else {
            return;
        };
        let Some(old) = &page.transcript else {
            return;
        };
        if old.corrected() || transcript.style != *style {
            return;
        }
        // Where it came from is still where the page's transcript came from.
        page.transcript = Some(Transcript {
            from_cache: old.from_cache,
            ..transcript
        });
        self.transcript_view = TranscriptView::default();
    }
}

impl eframe::App for NotebookApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Covers the previous frame's ui and paint, including texture uploads.
        if let Some(secs) = self.frame_time_for_logs(frame.info().cpu_usage) {
            log::debug!("slow frame: {:.0} ms", secs * 1000.0);
        }
        NotebookApp::ui(self, ui);
    }
}
