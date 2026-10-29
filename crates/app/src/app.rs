use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use eframe::egui;
use egui::{
    Align, Label, Layout, Rect, Response, RichText, Shape, TextStyle, TextWrapMode, TextureHandle,
    TextureOptions, UiBuilder, Vec2, WidgetText,
};
use jiff::tz::TimeZone;
use notebook_core::cache::Cache;
use notebook_core::llm_model::Model;
use notebook_core::transcript::Transcript;
use notebook_core::{
    CleanParams, Page, Pages, Quad, RgbaImage, decode, photo_corners, shrink_to_fit,
};

use crate::command::{Command, CommandState, EguiKeys, Frontend};
use crate::eval_log::{EvalLog, Event};
use crate::export::{Export, SavePath};
use crate::ingest::{Ingest, Load, PickFiles, Received};
use crate::jobs::Jobs;
use crate::timing::timed;
use crate::transcribe::{Transcriber, Transcriptions, Waker};
use crate::ui::corners::Corners;
use crate::ui::dialog::{self, Action, Choice};
use crate::ui::split::{self, Split};
use crate::ui::thumbnails::{self, Thumbnails};
use crate::ui::transcript_view::{self, TranscriptView};

/// Why the Transcribed view is unavailable.
const NO_TRANSCRIBER_HINT: &str =
    "Transcription needs ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN, and the desktop app";

/// What the model selector changes.
const MODEL_HINT: &str = "Model for new and forced transcriptions. Cached transcripts by either \
model are used; this one's is preferred.";

/// The button that retranscribes a page, skipping the cache.
const FORCE_LABEL: &str = "Force retranscription";

/// Frames taking longer than this, in seconds, are logged.
const SLOW_FRAME_SECS: f32 = 0.05;

/// Transcribe all asks first when it would send more pages than this.
const TRANSCRIBE_ALL_CONFIRM_ABOVE: usize = 4;

/// How long a hint stays in the status line, in seconds.
const HINT_SECS: f64 = 4.0;

/// Under a transcript cropped with other corners than the page's now.
const BOUNDS_CHANGED: &str = "Bounds changed since this was transcribed.";

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

/// One of a page's full-size images: its name, whether it's the cleaned one, and its JPEG, which
/// changes when the page is cleaned again.
#[derive(Clone)]
struct ImageKey {
    name: String,
    cleaned: bool,
    jpeg: Arc<[u8]>,
}

impl PartialEq for ImageKey {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.cleaned == other.cleaned
            && Arc::ptr_eq(&self.jpeg, &other.jpeg)
    }
}

/// A texture on screen, and what it shows.
struct Shown {
    key: ImageKey,
    texture: TextureHandle,
}

/// What the viewer shows of the selected page.
enum Showing {
    Full(TextureHandle),
    /// Its thumbnail until the full image is decoded, or nothing while the page loads.
    Placeholder(Option<TextureHandle>),
}

impl Showing {
    fn texture(&self) -> Option<&TextureHandle> {
        match self {
            Self::Full(texture) => Some(texture),
            Self::Placeholder(texture) => texture.as_ref(),
        }
    }
}

/// Where the user's commands come from: egui's key presses unless there's a native menu bar.
struct Input(Box<dyn Frontend>);

impl Default for Input {
    fn default() -> Self {
        Self(Box::new(EguiKeys))
    }
}

/// A page's thumbnails, as loaded.
struct Thumbs {
    original: TextureHandle,
    cleaned: TextureHandle,
}

/// Decodes the selected page's images for the viewer, on a thread of its own so they don't wait
/// behind pages that are loading.
struct Decodes {
    jobs: Jobs<ImageKey, Result<egui::ColorImage, String>>,
    /// The original and the cleaned image the viewer is waiting for. Any other that comes back is
    /// stale.
    wanted: [Option<ImageKey>; 2],
}

impl Default for Decodes {
    fn default() -> Self {
        Self {
            jobs: Jobs::on_own_threads(),
            wanted: [None, None],
        }
    }
}

#[derive(Default)]
pub struct NotebookApp {
    ingest: Ingest,
    pages: Pages,
    /// One per loaded page, by name.
    thumbs: HashMap<String, Thumbs>,
    /// Name of the page the user picked. The first page is shown until they pick one.
    selected: Option<String>,
    /// The strip of page thumbnails.
    thumbnails: Thumbnails,
    /// Stays the same when another page is selected.
    view: View,
    /// Full-size textures of the selected page: the original, then the cleaned one. Only these are
    /// kept: see `decisions/2026-10-29-two-viewer-textures.md`.
    shown: [Option<Shown>; 2],
    decodes: Decodes,
    clean_params: CleanParams,
    /// The page's corners in the Cleaned view.
    corners: Corners,
    /// Between the photo and the cleaned page in the Cleaned view.
    cleaned_split: Split,
    errors: Vec<String>,
    transcriptions: Transcriptions,
    transcript_view: TranscriptView,
    /// Between the page and its transcript in the Transcribed view.
    split: Split,
    export: Export,
    /// The transcribed page the user asked to delete, until they confirm.
    confirm_delete: Option<String>,
    /// The user asked to transcribe more than [`TRANSCRIBE_ALL_CONFIRM_ABOVE`] pages, until they
    /// confirm.
    confirm_transcribe_all: bool,
    /// The corrected page the user asked to retranscribe, until they confirm.
    confirm_force: Option<String>,
    eval_log: EvalLog,
    input: Input,
    /// Commands to run next frame.
    queued: Vec<Command>,
    /// Asks which files to add as pages. Without one, Add Pages… is disabled.
    pick_files: Option<PickFiles>,
    /// The file picker was opened since [`Self::frame_time_for_logs`] last looked.
    asked_which_files: bool,
    /// A passing note for the status line, and the time (`InputState::time`) it goes.
    hint: Option<(String, f64)>,
}

impl NotebookApp {
    /// Without a transcriber, the Transcribed view is disabled. Without a cache, every page is sent
    /// to Claude.
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

    /// Takes commands from `frontend`, such as the native menu bar, instead of egui's key presses.
    pub fn with_frontend(self, frontend: Box<dyn Frontend>) -> Self {
        Self {
            input: Input(frontend),
            ..self
        }
    }

    /// Runs `command` next frame, if it's enabled then.
    pub fn queue(&mut self, command: Command) {
        self.queued.push(command);
    }

    /// Loads dropped pages with `load` rather than [`notebook_core::Loaded::new`].
    pub fn with_loader(self, load: Load) -> Self {
        Self {
            ingest: Ingest::new(load),
            ..self
        }
    }

    /// Whether any page is loading, or any image is being decoded for the viewer.
    pub fn loading(&self) -> bool {
        self.ingest.pending() > 0 || self.decodes.jobs.running() > 0
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
            self.decodes.jobs.set_waker(waker);
        }
        self.run_commands(&ctx);
        self.add_dropped(&ctx);
        self.add_loaded(&ctx);
        self.show_decoded(&ctx);
        self.export.poll(&ctx);
        while let Some(done) = self.transcriptions.next() {
            let index = self.pages.position(&done.name);
            // The page may have been deleted meanwhile. Only a forced retranscription replaces a
            // transcript: a late cache hit mustn't undo the user's corrections.
            if let Some(page) = index.and_then(|i| self.pages.get_mut(i))
                && (done.forced || page.transcript.is_none())
            {
                let event = Event::transcribed(&done.name, &done.transcript, done.elapsed);
                self.eval_log.record(event);
                page.transcript = Some(done.transcript);
                page.transcribed_with = done.corners;
                if done.forced {
                    self.transcript_view = TranscriptView::default();
                }
            }
        }

        if self.pages.is_empty() {
            self.drop_zone(ui);
        } else {
            self.viewer(ui);
        }
        let states = self.command_states(&ctx);
        self.input.0.sync(&|c| states[c as usize]);
    }

    /// What `command` can do right now. Menus, key presses and [`Self::queue`] all go by this.
    pub fn command_state(&self, ctx: &egui::Context, command: Command) -> CommandState {
        let spec = command.spec();
        let index = self.selected_index();
        let page = self.pages.get(index);
        let can_transcribe = self.transcriptions.available();
        let can_export = can_transcribe && self.export.running().is_none();
        let possible = match command {
            Command::AddPages => self.pick_files.is_some(),
            Command::Export | Command::ExportAs => page.is_some() && can_export,
            Command::ViewOriginal | Command::ViewCleaned => page.is_some(),
            Command::PlaceCorners => page.is_some_and(|p| p.loaded.is_some()),
            Command::ResetCorners => page.is_some_and(|p| p.corners.is_some()),
            Command::ViewTranscribed | Command::Transcribe => page.is_some() && can_transcribe,
            Command::ForceRetranscribe => {
                can_transcribe
                    && page.is_some_and(|p| {
                        p.transcript.is_some() && !self.transcriptions.queued_or_running(&p.name)
                    })
            }
            Command::TranscribeAll => can_transcribe && !self.untranscribed().is_empty(),
            Command::PreviousPage | Command::MoveLeft => page.is_some() && index > 0,
            Command::NextPage | Command::MoveRight => index + 1 < self.pages.len(),
            Command::SortByName => !self.pages.is_sorted(),
            Command::Delete => page.is_some(),
        };
        let typing = spec.edits_text && ctx.egui_wants_keyboard_input();
        let checked = match command {
            Command::ViewOriginal => Some(self.view == View::Original),
            Command::ViewCleaned => Some(self.view == View::Cleaned),
            Command::ViewTranscribed => Some(self.view == View::Transcribed),
            _ => None,
        };
        let title = match command {
            Command::Export if self.export.can_reexport() => "Re-export",
            _ => spec.label,
        };
        CommandState {
            enabled: possible && !typing && !self.dialog_open(),
            checked,
            title,
        }
    }

    /// [`Self::command_state`] for each command, indexed by `command as usize`.
    fn command_states(&self, ctx: &egui::Context) -> [CommandState; Command::ALL.len()] {
        Command::ALL.map(|c| self.command_state(ctx, c))
    }

    /// Whether a dialog is waiting for an answer. Commands wait for it too.
    fn dialog_open(&self) -> bool {
        self.confirm_delete.is_some()
            || self.confirm_force.is_some()
            || self.confirm_transcribe_all
            || self.export.dialog_open()
    }

    /// Runs the commands queued and given since the last frame.
    fn run_commands(&mut self, ctx: &egui::Context) {
        let states = self.command_states(ctx);
        let mut commands = std::mem::take(&mut self.queued);
        commands.extend(self.input.0.poll(ctx, &|c| states[c as usize]));
        for command in commands {
            self.run(ctx, command);
        }
    }

    /// Does what `command` says, if it's enabled.
    fn run(&mut self, ctx: &egui::Context, command: Command) {
        if !self.command_state(ctx, command).enabled {
            return;
        }
        let index = self.selected_index();
        match command {
            Command::AddPages => self.add_picked(),
            Command::Export if self.export.can_reexport() => {
                self.export.reexport(ctx, &self.pages, index);
            }
            Command::Export | Command::ExportAs => self.export.open_dialog(),
            Command::ViewOriginal => self.set_view(View::Original),
            Command::ViewCleaned => self.set_view(View::Cleaned),
            Command::ViewTranscribed => self.set_view(View::Transcribed),
            Command::PreviousPage => self.select(index - 1),
            Command::NextPage => self.select(index + 1),
            Command::Transcribe => self.transcribe_shown(ctx),
            Command::ForceRetranscribe => {
                self.set_view(View::Transcribed);
                self.request_force(index);
            }
            Command::TranscribeAll => self.request_transcribe_all(),
            Command::MoveLeft => self.move_page(index, index - 1),
            Command::MoveRight => self.move_page(index, index + 1),
            Command::SortByName => self.sort_pages(),
            Command::PlaceCorners => self.place_corners(index),
            Command::ResetCorners => self.set_corners(index, None),
            Command::Delete => self.request_delete(),
        }
    }

    /// Shows `text` in the status line for a few seconds.
    fn show_hint(&mut self, ctx: &egui::Context, text: String) {
        let now = ctx.input(|i| i.time);
        self.hint = Some((text, now + HINT_SECS));
    }

    fn select(&mut self, index: usize) {
        if let Some(page) = self.pages.get(index) {
            self.selected = Some(page.name.clone());
        }
    }

    /// Switching to the Transcribed view transcribes the page shown.
    fn set_view(&mut self, view: View) {
        let switched = self.view != view;
        self.view = view;
        if switched && view == View::Transcribed {
            self.transcribe_selected();
        }
    }

    /// Asks which files to add, and adds them as pages.
    fn add_picked(&mut self) {
        let Some(pick_files) = self.pick_files.clone() else {
            return;
        };
        self.asked_which_files = true;
        let received = Ingest::read_paths(&pick_files());
        self.add_received(received);
    }

    /// Adds the files dropped this frame as pages.
    fn add_dropped(&mut self, ctx: &egui::Context) {
        let received = self.ingest.read_drops(ctx);
        self.add_received(received);
    }

    /// Adds `received` as pages, and starts loading them. Pages are usable before they've
    /// loaded: they can be moved, deleted and transcribed.
    fn add_received(&mut self, received: Vec<Received>) {
        if !received.is_empty() {
            self.errors.clear();
        }
        for (name, bytes) in received {
            let bytes: Arc<[u8]> = match bytes {
                Ok(bytes) => bytes.into(),
                Err(err) => {
                    self.errors.push(format!("Couldn't open {name}: {err}"));
                    continue;
                }
            };
            // Duplicate names are skipped.
            if self
                .pages
                .add(Page::new(name.clone(), bytes.clone()))
                .is_some()
            {
                self.ingest
                    .load(&name, bytes.clone(), self.clean_params.clone(), None);
                self.transcriptions.look_up(&name, bytes);
            }
        }
    }

    /// Fills in pages that finished loading or cleaning again, and removes those that couldn't be
    /// loaded.
    fn add_loaded(&mut self, ctx: &egui::Context) {
        while let Some((name, bytes, loaded)) = self.ingest.next() {
            // The page may have been deleted meanwhile, or replaced by another of the same name, or
            // given other corners.
            let Some(index) = self.pages.position(&name).filter(|&i| {
                self.pages.get(i).is_some_and(|p| {
                    let corners = loaded.as_ref().map_or(p.corners, |l| l.corners);
                    Arc::ptr_eq(&p.bytes, &bytes) && corners == p.corners
                })
            }) else {
                continue;
            };
            match loaded {
                Ok(mut loaded) => {
                    let Some(page) = self.pages.get_mut(index) else {
                        continue;
                    };
                    // Cleaned again: the original is the same, so its texture stays.
                    if let Some(old) = page.loaded.take() {
                        loaded.original = old.original;
                    }
                    let texture = |suffix: &str, img: &RgbaImage| {
                        let name = format!("{name}{suffix}");
                        ctx.load_texture(name, color_image(img), TextureOptions::LINEAR)
                    };
                    let thumbs = Thumbs {
                        original: texture("", &loaded.thumbnail),
                        cleaned: texture(" (cleaned)", &loaded.cleaned_thumbnail),
                    };
                    page.loaded = Some(loaded);
                    self.thumbs.insert(name, thumbs);
                }
                Err(err) => {
                    self.errors.push(format!("Couldn't open {name}: {err}"));
                    self.delete_page(index);
                }
            }
        }
    }

    /// Removes page `index`. If it was shown, selects the one after it, or before it if it was
    /// the last.
    fn delete_page(&mut self, index: usize) {
        let shown = self.selected_index() == index;
        if !shown {
            self.pin_selection();
        }
        let Some(page) = self.pages.remove(index) else {
            return;
        };
        self.thumbs.remove(&page.name);
        self.transcriptions.forget(&page.name);
        for shown in &mut self.shown {
            if shown.as_ref().is_some_and(|s| s.key.name == page.name) {
                *shown = None;
            }
        }
        if !shown {
            return;
        }
        self.transcript_view = TranscriptView::default();
        let next = self
            .pages
            .get(index)
            .or_else(|| self.pages.get(index.wrapping_sub(1)));
        self.selected = next.map(|p| p.name.clone());
    }

    /// Deletes the selected page, asking first if it has a transcript.
    fn request_delete(&mut self) {
        let index = self.selected_index();
        let Some(page) = self.pages.get(index) else {
            return;
        };
        if page.transcript.is_some() {
            self.confirm_delete = Some(page.name.clone());
        } else {
            self.delete_page(index);
        }
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = self.confirm_delete.clone() else {
            return;
        };
        let choice = dialog::show(ctx, "confirm delete", 320.0, |ui| {
            ui.label(format!("Delete {name} and its transcript?"));
            dialog::buttons(ui, "Delete page", Action::Dangerous, true)
        });
        if choice == Some(Choice::Affirmative) {
            let index = self.pages.position(&name);
            if let Some(index) = index {
                self.delete_page(index);
            }
        }
        if choice.is_some() {
            self.confirm_delete = None;
        }
    }

    /// Pages without a transcript that aren't being transcribed, including any that failed.
    fn untranscribed(&self) -> Vec<&Page> {
        self.pages
            .iter()
            .filter(|p| p.transcript.is_none() && !self.transcriptions.queued_or_running(&p.name))
            .collect()
    }

    /// Queues every page [`Self::untranscribed`]. Cached ones aren't sent.
    fn transcribe_all(&mut self) {
        let names: Vec<String> = self
            .untranscribed()
            .into_iter()
            .map(|p| p.name.clone())
            .collect();
        for name in names {
            if let Some(page) = self.pages.find(&name) {
                self.transcriptions
                    .start_later(page, self.clean_params.clone());
            }
        }
    }

    /// Transcribes every untranscribed page, asking first if there are many.
    fn request_transcribe_all(&mut self) {
        if self.untranscribed().len() > TRANSCRIBE_ALL_CONFIRM_ABOVE {
            self.confirm_transcribe_all = true;
        } else {
            self.transcribe_all();
        }
    }

    fn transcribe_all_dialog(&mut self, ctx: &egui::Context) {
        if !self.confirm_transcribe_all {
            return;
        }
        let count = self.untranscribed().len();
        let choice = dialog::show(ctx, "confirm transcribe all", 320.0, |ui| {
            ui.label(format!("Transcribe {count} pages?"));
            dialog::buttons(ui, "Transcribe", Action::Default, count > 0)
        });
        if choice == Some(Choice::Affirmative) {
            self.transcribe_all();
        }
        if choice.is_some() {
            self.confirm_transcribe_all = false;
        }
    }

    /// Retranscribes the page named `name`, skipping the cache.
    fn force(&mut self, name: &str) {
        if let Some(page) = self.pages.find(name) {
            self.transcriptions.force(page, self.clean_params.clone());
        }
    }

    /// Retranscribes page `index`, asking first if it's been corrected.
    fn request_force(&mut self, index: usize) {
        let Some(page) = self.pages.get(index) else {
            return;
        };
        let corrected = page.transcript.as_ref().is_some_and(Transcript::corrected);
        let name = page.name.clone();
        if corrected {
            self.confirm_force = Some(name);
        } else {
            self.force(&name);
        }
    }

    fn force_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = self.confirm_force.clone() else {
            return;
        };
        let choice = dialog::show(ctx, "confirm force", 320.0, |ui| {
            ui.label(format!(
                "Retranscribe {name}? Your corrections will be lost."
            ));
            dialog::buttons(ui, "Retranscribe", Action::Dangerous, true)
        });
        if choice == Some(Choice::Affirmative) {
            self.force(&name);
        }
        if choice.is_some() {
            self.confirm_force = None;
        }
    }

    /// Transcribes the selected page, unless it's transcribed, being transcribed, or failed
    /// (which waits for Retry).
    fn transcribe_selected(&mut self) {
        let index = self.selected_index();
        let Some(page) = self.pages.get(index) else {
            return;
        };
        if page.transcript.is_none() && self.transcriptions.error(&page.name).is_none() {
            self.start_transcribing(index);
        }
    }

    /// Shows the selected page's transcript, transcribing the page if it isn't, or retrying if
    /// that failed. If it's transcribed, says how to retranscribe it.
    fn transcribe_shown(&mut self, ctx: &egui::Context) {
        self.view = View::Transcribed;
        let index = self.selected_index();
        let Some(page) = self.pages.get(index) else {
            return;
        };
        if self.transcriptions.queued_or_running(&page.name) {
            return;
        }
        if page.transcript.is_some() {
            let force = shortcut_text(ctx, Command::ForceRetranscribe);
            let hint = format!(
                "{} is already transcribed. {force} retranscribes it.",
                page.name
            );
            self.show_hint(ctx, hint);
        } else {
            self.start_transcribing(index);
        }
    }

    /// Sends page `index` to be transcribed, unless it's in the cache.
    fn start_transcribing(&mut self, index: usize) {
        if let Some(page) = self.pages.get(index) {
            self.transcriptions.start(page, self.clean_params.clone());
        }
    }

    /// Shows page `index` in the Cleaned view, ready for its corners to be clicked in.
    fn place_corners(&mut self, index: usize) {
        self.set_view(View::Cleaned);
        if let Some(page) = self.pages.get(index) {
            self.corners.place(&page.name);
        }
    }

    /// Puts page `index`'s corners at `corners`, or back where cleaning finds them, and cleans it
    /// again with them.
    fn set_corners(&mut self, index: usize, corners: Option<Quad>) {
        let Some(page) = self.pages.get_mut(index) else {
            return;
        };
        page.corners = corners;
        if page.loaded.as_ref().is_some_and(|l| l.corners == corners) {
            return;
        }
        let params = self.clean_params.clone();
        self.ingest
            .load(&page.name, page.bytes.clone(), params, corners);
    }

    /// Moves page `from` to index `to`, keeping the shown page shown.
    fn move_page(&mut self, from: usize, to: usize) {
        self.pin_selection();
        self.pages.move_page(from, to);
    }

    /// Puts the pages back in filename order, keeping the shown page shown.
    fn sort_pages(&mut self) {
        self.pin_selection();
        self.pages.sort_by_name();
    }

    /// Selects the shown page by name, so it stays shown when the pages move. Until the user picks
    /// one, the shown page is whichever is first.
    fn pin_selection(&mut self) {
        self.selected = self
            .pages
            .get(self.selected_index())
            .map(|p| p.name.clone());
    }

    fn selected_index(&self) -> usize {
        let name = self.selected.as_ref();
        name.and_then(|name| self.pages.position(name)).unwrap_or(0)
    }

    fn drop_zone(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let rect = ui.max_rect().shrink(24.0);
            let stroke = if is_hovering_files(ui) {
                ui.visuals().selection.stroke
            } else {
                ui.visuals().widgets.noninteractive.fg_stroke
            };
            let corners = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
                rect.left_top(),
            ];
            ui.painter()
                .extend(Shape::dashed_line(&corners, stroke, 8.0, 6.0));

            ui.vertical_centered(|ui| {
                ui.add_space(rect.center().y - ui.min_rect().top() - 40.0);
                ui.heading("Drop image files here");
                ui.label("JPEG, PNG or HEIC");
                self.status(ui);
            });
        });
    }

    fn viewer(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                for (view, command) in [
                    (View::Original, Command::ViewOriginal),
                    (View::Cleaned, Command::ViewCleaned),
                    (View::Transcribed, Command::ViewTranscribed),
                ] {
                    let enabled = view != View::Transcribed || self.transcriptions.available();
                    let button = egui::Button::selectable(self.view == view, command.spec().label);
                    let button = ui
                        .add_enabled(enabled, button)
                        .on_disabled_hover_text(NO_TRANSCRIBER_HINT);
                    if with_shortcut(button, command).clicked() {
                        self.set_view(view);
                    }
                }
                ui.separator();
                let index = self.selected_index();
                if let Some(page) = self.pages.get(index) {
                    ui.label(format!(
                        "{} · {} of {}",
                        page.name,
                        index + 1,
                        self.pages.len()
                    ));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if with_shortcut(ui.button("Delete"), Command::Delete).clicked() {
                        self.request_delete();
                    }
                    let sort =
                        ui.add_enabled(!self.pages.is_sorted(), egui::Button::new("Sort by name"));
                    if sort.clicked() {
                        self.sort_pages();
                    }
                    self.model_selector(ui);
                });
            });
        });

        egui::Panel::bottom("pages").show(ui, |ui| {
            ui.add_space(4.0);
            let selected = self.selected_index();
            let thumbs = |name: &str| self.thumbs.get(name).map(|t| &t.original);
            match self.thumbnails.show(ui, &self.pages, thumbs, selected) {
                Some(thumbnails::Action::Select(name)) => self.selected = Some(name),
                Some(thumbnails::Action::Move { from, to }) => self.move_page(from, to),
                None => {}
            }
            ui.horizontal(|ui| {
                self.status(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Only transcribed pages can be exported, one export at a time.
                    let can_transcribe = self.transcriptions.available();
                    let can_export = can_transcribe && self.export.running().is_none();
                    let mut export = ui.add_enabled(can_export, egui::Button::new("Export"));
                    if !can_transcribe {
                        export = export.on_disabled_hover_text(NO_TRANSCRIBER_HINT);
                    }
                    if with_shortcut(export, Command::ExportAs).clicked() {
                        self.export.open_dialog();
                    }
                    let reexport = ui.add_enabled(
                        can_export && self.export.can_reexport(),
                        egui::Button::new("Re-export"),
                    );
                    if with_shortcut(reexport, Command::Export).clicked() {
                        self.export
                            .reexport(ui.ctx(), &self.pages, self.selected_index());
                    }
                    let untranscribed = self.untranscribed().len();
                    let transcribe_all = ui
                        .add_enabled(
                            can_transcribe && untranscribed > 0,
                            egui::Button::new("Transcribe all"),
                        )
                        .on_disabled_hover_text(if can_transcribe {
                            "Every page is transcribed"
                        } else {
                            NO_TRANSCRIBER_HINT
                        });
                    if with_shortcut(transcribe_all, Command::TranscribeAll).clicked() {
                        self.request_transcribe_all();
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| match self.view {
            View::Original => self.original(ui),
            View::Cleaned => self.cleaned(ui),
            View::Transcribed => self.transcribed(ui),
        });

        self.delete_dialog(ui.ctx());
        self.transcribe_all_dialog(ui.ctx());
        self.force_dialog(ui.ctx());
        self.export
            .dialog(ui.ctx(), &self.pages, self.selected_index());

        if is_hovering_files(ui) {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("drop_overlay"),
            ));
            let rect = ui.ctx().content_rect();
            painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(160));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drop to add pages",
                egui::TextStyle::Heading.resolve(ui.style()),
                egui::Color32::WHITE,
            );
        }
    }

    /// Which model new and forced transcriptions use.
    fn model_selector(&mut self, ui: &mut egui::Ui) {
        let available = self.transcriptions.available();
        let mut model = self.transcriptions.model();
        ui.add_enabled_ui(available, |ui| {
            // In the toolbar's right-to-left layout, so the label comes out on the left.
            let combo = egui::ComboBox::from_id_salt("transcription model")
                .selected_text(model.label())
                .show_ui(ui, |ui| {
                    for m in Model::ALL {
                        ui.selectable_value(&mut model, m, m.label());
                    }
                })
                .response;
            let label = ui.label("Transcription model");
            combo
                .labelled_by(label.id)
                .on_hover_text(MODEL_HINT)
                .on_disabled_hover_text(NO_TRANSCRIBER_HINT);
        });
        self.transcriptions.set_model(model);
    }

    /// The selected page's photo.
    fn original(&mut self, ui: &mut egui::Ui) {
        let name = self
            .pages
            .get(self.selected_index())
            .map(|p| p.name.clone());
        if let (Some(name), Some(showing)) = (name, self.current_texture(ui.ctx(), false)) {
            let panel = ui.available_rect_before_wrap();
            self.show_page(ui, &name, &showing, panel, false);
        }
    }

    /// The selected page's photo with its corners to drag, beside the page cleaned with them.
    fn cleaned(&mut self, ui: &mut egui::Ui) {
        let index = self.selected_index();
        let Some(name) = self.pages.get(index).map(|p| p.name.clone()) else {
            return;
        };
        let rect = ui.available_rect_before_wrap();
        let gap = ui.spacing().item_spacing.x * 2.0;
        let limits = split::Limits::default();
        let (photo_rect, cleaned_rect) =
            self.cleaned_split
                .show(ui, rect, gap, limits, "Resize the cleaned page");
        let photo_rect = self.corner_buttons(ui, index, photo_rect);
        if let Some(showing) = self.current_texture(ui.ctx(), false)
            && let Some(image_rect) = self.show_page(ui, &name, &showing, photo_rect, false)
        {
            self.edit_corners(ui, index, image_rect);
        }
        if let Some(showing) = self.current_texture(ui.ctx(), true) {
            self.show_page(ui, &name, &showing, cleaned_rect, true);
        }
    }

    /// Buttons to place page `index`'s corners or put them back, at the top of `rect`. Returns
    /// the rest of `rect`.
    fn corner_buttons(&mut self, ui: &mut egui::Ui, index: usize, rect: Rect) -> Rect {
        let Some(page) = self.pages.get(index) else {
            return rect;
        };
        let prompt = self.corners.prompt(&page.name);
        let (can_place, can_reset) = (page.loaded.is_some(), page.corners.is_some());
        let row = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ui.horizontal(|ui| {
                let button = |ui: &mut egui::Ui, enabled, command: Command| {
                    let button = egui::Button::new(command.spec().label);
                    ui.add_enabled(enabled, button).clicked()
                };
                let place = button(ui, can_place, Command::PlaceCorners);
                let reset = button(ui, can_reset, Command::ResetCorners);
                if let Some(prompt) = prompt {
                    ui.label(prompt);
                }
                (place, reset)
            })
            .inner
        });
        match row.inner {
            (true, _) => self.place_corners(index),
            (_, true) => self.set_corners(index, None),
            _ => {}
        }
        rect.with_min_y(row.response.rect.bottom() + ui.spacing().item_spacing.y)
    }

    /// Page `index`'s corners over its photo, drawn at `image_rect`: the user's, else the ones
    /// cleaning found, else the photo's own. Cleans the page again when they change.
    fn edit_corners(&mut self, ui: &mut egui::Ui, index: usize, image_rect: Rect) {
        let Some(page) = self.pages.get(index) else {
            return;
        };
        let Some(loaded) = &page.loaded else {
            return;
        };
        let report = &loaded.report;
        let (w, h) = report.source_size;
        let quad = page
            .corners
            .or(report.detected.ok())
            .unwrap_or_else(|| photo_corners(w, h));
        let changed = self
            .corners
            .show(ui, &page.name, quad, report.source_size, image_rect);
        if let Some(corners) = changed {
            self.set_corners(index, Some(corners));
        }
    }

    /// The page named `name`, its `cleaned` image or its photo, as large as fits in `rect`,
    /// centred, returning where it went. A placeholder gets a spinner, and a page that's loading
    /// only a spinner.
    fn show_page(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        showing: &Showing,
        rect: Rect,
        cleaned: bool,
    ) -> Option<Rect> {
        let spinner = Rect::from_center_size(rect.center(), Vec2::splat(32.0));
        let Some(texture) = showing.texture() else {
            egui::Spinner::new().paint_at(ui, spinner);
            return None;
        };
        let alt_text = match (showing, cleaned) {
            (Showing::Placeholder(_), false) => format!("Current: {name} (loading)"),
            (Showing::Placeholder(_), true) => format!("Current: {name} (cleaned, loading)"),
            (Showing::Full(_), true) => format!("Current: {name} (cleaned)"),
            (Showing::Full(_), false) => format!("Current: {name}"),
        };
        let image = egui::Image::new(texture).shrink_to_fit().alt_text(alt_text);
        // Give the image exactly the rect it's drawn in. In a justified layout its response
        // would cover the whole panel, with the image centred inside.
        let size = image.calc_size(rect.size(), Some(texture.size_vec2()));
        let image_rect = ui
            .put(Rect::from_center_size(rect.center(), size), image)
            .rect;
        if let Showing::Placeholder(_) = showing {
            egui::Spinner::new().paint_at(ui, spinner);
        }
        Some(image_rect)
    }

    /// The cleaned page beside its transcript, for correcting.
    fn transcribed(&mut self, ui: &mut egui::Ui) {
        let index = self.selected_index();
        let Some(name) = self.pages.get(index).map(|p| p.name.clone()) else {
            return;
        };
        // A page being retranscribed shows progress, not the transcript it's replacing.
        let transcribing = self.transcriptions.queued_or_running(&name);
        let rect = ui.available_rect_before_wrap();
        let showing = self.current_texture(ui.ctx(), true);
        let shown = self
            .pages
            .get(index)
            .and_then(|p| p.transcript.as_ref())
            .filter(|_| !transcribing);
        let limits = split::Limits {
            text_natural: shown.map(|t| transcript_view::natural_width(ui, t)),
            image_cap: showing.as_ref().and_then(Showing::texture).map(|t| {
                let size = t.size_vec2();
                rect.height() * size.x / size.y
            }),
        };
        let gap = ui.spacing().item_spacing.x * 2.0;
        let (mut page_rect, text_rect) =
            self.split
                .show(ui, rect, gap, limits, "Resize the transcript");
        if !transcribing && let Some(status) = self.transcript_status(ui, index, page_rect) {
            page_rect.max.y = status.top() - ui.spacing().item_spacing.y;
        }
        let image_rect =
            showing.and_then(|showing| self.show_page(ui, &name, &showing, page_rect, true));
        let transcript = self
            .pages
            .get_mut(index)
            .and_then(|p| p.transcript.as_mut())
            .filter(|_| !transcribing);
        if let Some(transcript) = transcript {
            let events = self
                .transcript_view
                .show(ui, &name, transcript, image_rect, text_rect);
            for event in events {
                self.eval_log.record(event);
            }
        } else {
            ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
                self.transcription_progress(ui, index);
            });
        }
    }

    /// Where page `index`'s transcript came from, or why retranscribing it failed, and a button to
    /// retranscribe it, at the bottom of `rect`. Returns where it went, if the page has a
    /// transcript.
    fn transcript_status(&mut self, ui: &mut egui::Ui, index: usize, rect: Rect) -> Option<Rect> {
        let page = self.pages.get(index)?;
        let transcript = page.transcript.as_ref()?;
        let when = when_and_by(transcript, &TimeZone::system());
        let error = self.transcriptions.error(&page.name);
        let mut text = match error {
            Some(err) => {
                format!(
                    "Retranscription failed: {err}. Using the cached version, transcribed {when}."
                )
            }
            None if transcript.from_cache => format!("From cache: transcribed {when}"),
            None => format!("Transcribed {when}"),
        };
        if page.corners != page.transcribed_with {
            text = format!("{text}\n{BOUNDS_CHANGED}");
        }
        let text = match error {
            Some(_) => RichText::new(text).color(ui.visuals().error_fg_color),
            None => RichText::new(text),
        };
        let layout = Layout::bottom_up(Align::Min);
        // Bottom up, so the row sits at the bottom however many lines the text wraps to.
        let scope = ui.scope_builder(UiBuilder::new().max_rect(rect).layout(layout), |ui| {
            ui.horizontal(|ui| {
                // Long text wraps in the room left of the button.
                let force = WidgetText::from(FORCE_LABEL).into_galley(
                    ui,
                    Some(TextWrapMode::Extend),
                    f32::INFINITY,
                    TextStyle::Button,
                );
                let button_width = force.size().x + 2.0 * ui.spacing().button_padding.x;
                let text_width = ui.available_width() - button_width - ui.spacing().item_spacing.x;
                ui.scope(|ui| {
                    ui.set_max_width(text_width);
                    ui.add(Label::new(text).wrap());
                });
                with_shortcut(ui.button(FORCE_LABEL), Command::ForceRetranscribe).clicked()
            })
            .inner
        });
        if scope.inner {
            self.request_force(index);
        }
        Some(scope.response.rect)
    }

    /// Page `index` being transcribed, or why it couldn't be, or a button to transcribe it.
    fn transcription_progress(&mut self, ui: &mut egui::Ui, index: usize) {
        let Some(page) = self.pages.get(index) else {
            return;
        };
        let name = &page.name;
        let start = if self.transcriptions.queued_or_running(name) {
            spinner(ui, &format!("Transcribing {name}…"));
            false
        } else if let Some(err) = self.transcriptions.error(name) {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Couldn't transcribe {name}: {err}"),
            );
            with_shortcut(ui.button("Retry"), Command::Transcribe).clicked()
        } else if self.transcriptions.checking(name) {
            spinner(ui, &format!("Looking for {name} in the cache…"));
            false
        } else {
            ui.label(format!("{name} isn't transcribed yet."));
            with_shortcut(ui.button("Transcribe"), Command::Transcribe).clicked()
        };
        if start {
            self.start_transcribing(index);
        }
    }

    /// Loading and transcription progress, errors from the last drop, and what the last export
    /// did.
    fn status(&self, ui: &mut egui::Ui) {
        if self.ingest.pending() > 0 {
            spinner(ui, &format!("Loading {}…", self.ingest.pending()));
        }
        match self.transcriptions.pending() {
            0 => {}
            1 => spinner(ui, "Transcribing 1 page…"),
            n => spinner(ui, &format!("Transcribing {n} pages…")),
        }
        match self.export.running() {
            None => {}
            Some(1) => spinner(ui, "Exporting 1 page…"),
            Some(n) => spinner(ui, &format!("Exporting {n} pages…")),
        }
        for err in &self.errors {
            ui.colored_label(ui.visuals().error_fg_color, err);
        }
        let now = ui.input(|i| i.time);
        if let Some((hint, until)) = &self.hint
            && now < *until
        {
            ui.label(hint);
            ui.ctx().request_repaint_after_secs((*until - now) as f32);
        }
        match self.export.status() {
            Some(Ok(done)) => {
                ui.label(done);
            }
            Some(Err(err)) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
            None => {}
        }
    }

    /// The selected page's photo, or its `cleaned` image, at full size once it's decoded. Until
    /// then, the image it replaces or its thumbnail, while the full image is decoded off the UI
    /// thread. While the page is cleaned again with new corners, the cleaned image is a
    /// placeholder too.
    fn current_texture(&mut self, ctx: &egui::Context, cleaned: bool) -> Option<Showing> {
        let page = self.pages.get(self.selected_index())?;
        let Some(loaded) = &page.loaded else {
            return Some(Showing::Placeholder(None));
        };
        let jpeg = if cleaned {
            &loaded.cleaned
        } else {
            &loaded.original
        };
        let key = ImageKey {
            name: page.name.clone(),
            cleaned,
            jpeg: jpeg.clone(),
        };
        let slot = usize::from(cleaned);
        let shown = self.shown[slot]
            .as_ref()
            .filter(|s| s.key.name == page.name);
        let recleaning = cleaned && loaded.corners != page.corners;
        if let Some(shown) = shown.filter(|s| s.key == key) {
            let texture = shown.texture.clone();
            return Some(if recleaning {
                Showing::Placeholder(Some(texture))
            } else {
                Showing::Full(texture)
            });
        }
        if self.decodes.wanted[slot].as_ref() != Some(&key) {
            let (name, jpeg) = (page.name.clone(), jpeg.clone());
            let max_side = ctx.input(|i| i.max_texture_side) as u32;
            self.decodes.jobs.spawn(key.clone(), move || {
                let img = timed(format_args!("{name}: decode"), || decode(&jpeg));
                let img = img.map_err(|e| e.to_string())?;
                Ok(color_image(&shrink_to_fit(img, max_side)))
            });
            self.decodes.wanted[slot] = Some(key);
        }
        let thumbs = self.thumbs.get(&page.name);
        let thumb = thumbs.map(|t| if cleaned { &t.cleaned } else { &t.original });
        let replaced = shown.map(|s| &s.texture);
        Some(Showing::Placeholder(replaced.or(thumb).cloned()))
    }

    /// Puts the images the viewer is waiting for on screen once they're decoded. Others are
    /// stale.
    fn show_decoded(&mut self, ctx: &egui::Context) {
        while let Some((key, image)) = self.decodes.jobs.next() {
            let slot = usize::from(key.cleaned);
            if self.decodes.wanted[slot].as_ref() != Some(&key) {
                continue;
            }
            self.decodes.wanted[slot] = None;
            let name = &key.name;
            let texture = match image {
                Ok(image) => timed(format_args!("{name}: load texture"), || {
                    ctx.load_texture(name, image, TextureOptions::LINEAR)
                }),
                // It decoded when the page loaded, so this shouldn't happen.
                Err(err) => {
                    log::error!("Couldn't decode {name}: {err}");
                    let Some(thumbs) = self.thumbs.get(name) else {
                        continue;
                    };
                    if key.cleaned {
                        thumbs.cleaned.clone()
                    } else {
                        thumbs.original.clone()
                    }
                }
            };
            self.shown[slot] = Some(Shown { key, texture });
        }
    }
}

/// When and by which model `transcript` was made: "2026-10-23 14:05 by claude-opus-5-5", in `tz`,
/// or just "by …" if it wasn't cached.
fn when_and_by(transcript: &Transcript, tz: &TimeZone) -> String {
    match transcript.stored_at {
        Some(at) => format!("{} by {}", format_time(at, tz), transcript.model),
        None => format!("by {}", transcript.model),
    }
}

fn format_time(time: SystemTime, tz: &TimeZone) -> String {
    match jiff::Timestamp::try_from(time) {
        Ok(at) => at
            .to_zoned(tz.clone())
            .strftime("%Y-%m-%d %H:%M")
            .to_string(),
        Err(_) => "at an unknown time".to_owned(),
    }
}

/// `command`'s shortcut, as the platform writes it: "⇧⌘T" on a Mac.
fn shortcut_text(ctx: &egui::Context, command: Command) -> String {
    let shortcut = command.spec().shortcut;
    shortcut.map_or_else(String::new, |s| ctx.format_shortcut(&s))
}

/// `button`, saying on hover which shortcut does the same.
fn with_shortcut(button: Response, command: Command) -> Response {
    if command.spec().shortcut.is_none() {
        return button;
    }
    let keys = shortcut_text(&button.ctx, command);
    button.on_hover_text(keys)
}

fn spinner(ui: &mut egui::Ui, label: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(label);
    });
}

fn is_hovering_files(ui: &egui::Ui) -> bool {
    ui.ctx().input(|i| !i.raw.hovered_files.is_empty())
}

fn color_image(img: &RgbaImage) -> egui::ColorImage {
    let size = [img.width() as usize, img.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw())
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn times_are_shown_to_the_minute() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_761_225_959);
        assert_eq!(format_time(at, &TimeZone::UTC), "2025-10-23 13:25");
        let sydney = TimeZone::get("Australia/Sydney").unwrap();
        assert_eq!(format_time(at, &sydney), "2025-10-24 00:25");
    }
}
