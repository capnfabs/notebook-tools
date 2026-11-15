//! The app: [`NotebookApp`] holds the pages and everything about how they're shown, and draws
//! the UI each frame. Its methods are spread over this module's files by what they do:
//!
//! - [`commands`]: what each [`Command`](crate::command::Command) can do now, and doing it.
//! - [`pages`]: adding, loading, deleting, moving and transcribing pages, and the dialogs that
//!   confirm those.
//! - [`views`]: the selected page in the Original, Cleaned and Transcribed views.
//! - [`transcript_panel`]: the transcript's status and transcription progress.
//! - [`chrome`]: the drop zone, toolbar, page strip and status line.
//! - [`project`]: New, Open, Save and Save As, and asking to save before changes are lost.
//! - [`textures`]: the images those show, decoded off the UI thread.
//!
//! See `decisions/2026-10-31-code-layout.md`.

mod chrome;
mod commands;
mod pages;
mod project;
mod textures;
mod transcript_panel;
mod views;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use notebook_core::cache::Cache;
use notebook_core::clean::CleanParams;
use notebook_core::page::{PageId, Pages};
use notebook_core::settings::Settings;
use textures::Textures;

use crate::auth::Auth;
use crate::command::{EguiKeys, Frontend};
use crate::eval_log::{EvalLog, Event};
use crate::export::{Export, SavePath};
use crate::ingest::{Ingest, Load, OpenedFiles, PickFiles};
use crate::jobs::Waker;
use crate::t;
use crate::transcriptions::{Transcriber, Transcriptions};
use crate::ui::api_key::{self, KeyDialog};
use crate::ui::corners::Corners;
use crate::ui::settings::{self, KeySummary, SettingsDialog};
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
    /// The page the user picked. The first page is shown until they pick one.
    selected: Option<PageId>,
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
    confirm_delete: Option<PageId>,
    /// The user asked to transcribe more than [`pages::TRANSCRIBE_ALL_CONFIRM_ABOVE`] pages, until
    /// they confirm.
    confirm_transcribe_all: bool,
    /// The corrected page the user asked to retranscribe, until they confirm.
    confirm_retranscribe: Option<PageId>,
    /// The page whose corrections the user asked to drop, until they confirm.
    confirm_reset: Option<PageId>,
    settings_dialog: SettingsDialog,
    /// Where pages' credentials come from. Without it, the transcriber given is all there is.
    auth: Option<Auth>,
    key_dialog: KeyDialog,
    /// What to do once the user enters a key, if they were asked for one by trying to transcribe.
    after_key: Option<pages::AfterKey>,
    /// Where settings are saved. Without one, they last until the app quits.
    settings_path: Option<PathBuf>,
    eval_log: EvalLog,
    input: Input,
    /// Asks which files to add as pages, or which project to open. Without one, Add Pages… and
    /// Open Project… are disabled.
    pick_files: Option<PickFiles>,
    /// Files the system asks the app to open, taken as drops.
    opened_files: Option<OpenedFiles>,
    /// Asks where to save exports and projects. Without one, they can't be saved to a file.
    save_path: Option<SavePath>,
    /// The file picker or the project's save dialog was opened since
    /// [`Self::frame_time_for_logs`] last looked.
    asked_which_files: bool,
    project: project::Project,
    /// Repaints the UI when work finishes off the UI thread. Set on the first frame.
    waker: Option<Waker>,
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

    /// Sends pages with `auth`'s credentials, asking for a key when there are none.
    pub fn with_auth(mut self, auth: Auth) -> Self {
        self.transcriptions.set_transcriber(auth.transcriber());
        self.auth = Some(auth);
        self
    }

    /// Asks where to save exports and projects with `save_path`. Without one, exporting to a file
    /// and saving the project do nothing.
    pub fn with_save_path(self, save_path: SavePath) -> Self {
        Self {
            export: Export::new(Some(save_path.clone())),
            save_path: Some(save_path),
            ..self
        }
    }

    /// Asks which files to add, or which project to open, with `pick_files`.
    pub fn with_file_picker(self, pick_files: PickFiles) -> Self {
        Self {
            pick_files: Some(pick_files),
            ..self
        }
    }

    /// Takes each batch of files that comes out of `opened` as if it had been dropped on the window.
    pub fn with_opened_files(self, opened: OpenedFiles) -> Self {
        Self {
            opened_files: Some(opened),
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
        if self.waker.is_none() {
            let ctx = ctx.clone();
            self.waker = Some(Arc::new(move || ctx.request_repaint()));
            self.set_wakers();
        }
        self.handle_close_request(&ctx);
        self.run_commands(&ctx);
        self.add_dropped(&ctx);
        self.add_opened(&ctx);
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
        self.show_key_dialog(&ctx);
        self.unsaved_dialog(&ctx);
        self.update_title(&ctx);
        let states = self.command_states(&ctx);
        self.input.0.sync(&|c| states[c as usize].clone());
        let unsaved = self.has_unsaved_changes();
        self.input.0.set_unsaved(unsaved);
    }

    /// Has work done off the UI thread repaint the UI when it finishes.
    fn set_wakers(&mut self) {
        let Some(waker) = self.waker.clone() else {
            return;
        };
        self.transcriptions.set_waker(waker.clone());
        self.key_dialog.set_waker(waker.clone());
        self.ingest.set_waker(waker.clone());
        self.textures.set_waker(waker);
    }

    /// Gives pages the transcripts that came back since the last frame.
    fn take_transcriptions(&mut self) {
        while let Some(done) = self.transcriptions.next() {
            // The page may have been deleted meanwhile. Only a forced retranscription replaces a
            // transcript: a late cache hit mustn't undo the user's corrections.
            if let Some(page) = self.pages.find_mut(done.id)
                && (done.forced || page.transcript.is_none())
            {
                let event = Event::transcribed(page, &done.transcript, done.elapsed);
                self.eval_log.record(event);
                page.transcript = Some(done.transcript);
                if done.forced {
                    self.transcript_view = TranscriptView::default();
                }
                self.restyle_if_stale(done.id);
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
        let key = self.key_summary();
        let covered = self.key_dialog.is_open();
        let settings = match self.settings_dialog.show(ctx, key.as_ref(), covered) {
            Some(settings::Outcome::Saved(settings)) => settings,
            Some(settings::Outcome::ClearFailed(error)) => {
                self.errors.push(error);
                return;
            }
            Some(settings::Outcome::EnterKey) => {
                self.after_key = None;
                self.key_dialog.open();
                return;
            }
            Some(settings::Outcome::RemoveKey) => {
                self.remove_key();
                return;
            }
            None => return,
        };
        if let Some(path) = &self.settings_path
            && let Err(err) = settings.save(path)
        {
            log::warn!("couldn't save settings to {}: {err}", path.display());
            let error = t!("settings-save-failed", error = err.to_string());
            self.errors.push(error);
        }
        self.transcriptions.set_settings(settings);
        let ids: Vec<PageId> = self.pages.iter().map(|p| p.id).collect();
        for id in ids {
            self.restyle_if_stale(id);
        }
    }

    /// The credentials pages are sent with, for Settings, if the app has an [`Auth`].
    fn key_summary(&self) -> Option<KeySummary> {
        let auth = self.auth.as_ref()?;
        Some(KeySummary {
            active: auth.active(),
            saved: auth.has_saved_key(),
        })
    }

    /// The dialog asking for an API key, if it's open. Once a key's saved, pages are sent with it,
    /// and what the user was doing when they were asked for it carries on.
    fn show_key_dialog(&mut self, ctx: &egui::Context) {
        let Some(auth) = &mut self.auth else {
            return;
        };
        match self.key_dialog.show(ctx, auth) {
            Some(api_key::Outcome::Saved { warning }) => {
                self.transcriptions.set_transcriber(auth.transcriber());
                if let Some(warning) = warning {
                    self.show_hint(ctx, warning);
                }
                if let Some(after) = self.after_key.take() {
                    self.carry_on(ctx, after);
                }
            }
            Some(api_key::Outcome::Closed) => self.after_key = None,
            None => {}
        }
    }

    /// Forgets the key saved in the keychain, so pages are sent with the environment's
    /// credentials, if any.
    fn remove_key(&mut self) {
        let Some(auth) = &mut self.auth else {
            return;
        };
        if let Err(err) = auth.remove() {
            log::warn!("couldn't remove the API key: {err}");
            let error = t!("settings-api-key-remove-failed", error = err);
            self.errors.push(error);
        }
        self.transcriptions.set_transcriber(auth.transcriber());
    }

    /// Reads page `id`'s transcript again from Claude's answer if it's in another
    /// style than the settings', unless the user has corrected it: that would lose their
    /// corrections.
    fn restyle_if_stale(&mut self, id: PageId) {
        let style = &self.transcriptions.settings().markdown;
        let Some(page) = self.pages.find_mut(id) else {
            return;
        };
        let Some(transcript) = &page.transcript else {
            return;
        };
        if transcript.corrected() || transcript.style == *style {
            return;
        }
        match transcript.restyled(style) {
            Ok(restyled) => {
                page.transcript = Some(restyled);
                self.transcript_view = TranscriptView::default();
            }
            Err(err) => log::warn!("{}: couldn't restyle the transcript: {err}", page.name),
        }
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
