use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use eframe::egui;
use egui::{
    Align, Color32, Key, Label, Layout, Modifiers, Pos2, Rect, RichText, Shape, Stroke, TextStyle,
    TextWrapMode, TextureHandle, TextureOptions, UiBuilder, Vec2, WidgetText,
};
use jiff::tz::TimeZone;
use notebook_core::cache::Cache;
use notebook_core::llm_model::Model;
use notebook_core::transcript::Transcript;
use notebook_core::{CleanParams, Page, Pages, RgbaImage, decode, shrink_to_fit};

use crate::eval_log::{EvalLog, Event};
use crate::export::{Export, SavePath};
use crate::ingest::{Ingest, Load};
use crate::jobs::Jobs;
use crate::timing::timed;
use crate::transcribe::{Transcriber, Transcriptions, Waker};
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

/// Transcribe all asks first when it would send more pages than this.
const TRANSCRIBE_ALL_CONFIRM_ABOVE: usize = 4;

/// Which version of the selected page the viewer shows.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum View {
    #[default]
    Original,
    Cleaned,
    /// Claude's transcription beside the cleaned page, for correcting. Switching to it transcribes
    /// the page shown, if it isn't already; selecting another page doesn't.
    Transcribed,
}

impl View {
    fn shows_cleaned(self) -> bool {
        match self {
            Self::Original => false,
            Self::Cleaned | Self::Transcribed => true,
        }
    }
}

/// The texture on screen, and what it shows.
struct Shown {
    name: String,
    cleaned: bool,
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

/// A page's thumbnails, as loaded.
struct Thumbs {
    original: TextureHandle,
    cleaned: TextureHandle,
}

/// Decodes the selected page's image for the viewer, on a thread of its own so it doesn't wait
/// behind pages that are loading. By page name and whether it's the cleaned image.
struct Decodes {
    jobs: Jobs<(String, bool), Result<egui::ColorImage, String>>,
    /// The image the viewer is waiting for. Any other that comes back is stale.
    wanted: Option<(String, bool)>,
}

impl Default for Decodes {
    fn default() -> Self {
        Self {
            jobs: Jobs::on_own_threads(),
            wanted: None,
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
    /// Full-size texture for the selected page and view. Only one is kept: see
    /// `decisions/2026-10-28-pages-cleaned-on-load.md`.
    current: Option<Shown>,
    decodes: Decodes,
    clean_params: CleanParams,
    /// Show the cleanup debug overlay.
    debug: bool,
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
    }

    /// Adds the files dropped this frame as pages, and starts loading them. Pages are usable
    /// before they've loaded: they can be moved, deleted and transcribed.
    fn add_dropped(&mut self, ctx: &egui::Context) {
        let received = self.ingest.read_drops(ctx);
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
                    .load(&name, bytes.clone(), self.clean_params.clone());
                self.transcriptions.look_up(&name, bytes);
            }
        }
    }

    /// Fills in pages that finished loading, and removes those that couldn't be.
    fn add_loaded(&mut self, ctx: &egui::Context) {
        while let Some((name, bytes, loaded)) = self.ingest.next() {
            // The page may have been deleted meanwhile, or replaced by another of the same name.
            let Some(index) = self.pages.position(&name).filter(|&i| {
                self.pages
                    .get(i)
                    .is_some_and(|p| Arc::ptr_eq(&p.bytes, &bytes))
            }) else {
                continue;
            };
            match loaded {
                Ok(loaded) => {
                    let texture = |suffix: &str, img: &RgbaImage| {
                        let name = format!("{name}{suffix}");
                        ctx.load_texture(name, color_image(img), TextureOptions::LINEAR)
                    };
                    let thumbs = Thumbs {
                        original: texture("", &loaded.thumbnail),
                        cleaned: texture(" (cleaned)", &loaded.cleaned_thumbnail),
                    };
                    self.thumbs.insert(name, thumbs);
                    if let Some(page) = self.pages.get_mut(index) {
                        page.loaded = Some(loaded);
                    }
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
        if self.current.as_ref().is_some_and(|c| c.name == page.name) {
            self.current = None;
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
        let pages: Vec<(String, Arc<[u8]>)> = self
            .untranscribed()
            .into_iter()
            .map(|p| (p.name.clone(), p.bytes.clone()))
            .collect();
        for (name, bytes) in pages {
            self.transcriptions
                .start_later(&name, bytes, self.clean_params.clone());
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
            self.transcriptions
                .force(name, page.bytes.clone(), self.clean_params.clone());
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
        let Some(page) = self.pages.get(self.selected_index()) else {
            return;
        };
        if page.transcript.is_none() && self.transcriptions.error(&page.name).is_none() {
            self.transcriptions
                .start(&page.name, page.bytes.clone(), self.clean_params.clone());
        }
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

    /// Cmd+Left and Cmd+Right move the selected page one place, unless a text field has the
    /// keyboard.
    fn move_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (left, right) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::ArrowLeft),
                i.consume_key(Modifiers::COMMAND, Key::ArrowRight),
            )
        });
        let index = self.selected_index();
        if left && index > 0 {
            self.move_page(index, index - 1);
        }
        if right && index + 1 < self.pages.len() {
            self.move_page(index, index + 1);
        }
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
        self.move_keys(ui.ctx());
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.view, View::Original, "Original");
                ui.selectable_value(&mut self.view, View::Cleaned, "Cleaned");
                let transcribed = ui
                    .add_enabled(
                        self.transcriptions.available(),
                        egui::Button::selectable(self.view == View::Transcribed, "Transcribed"),
                    )
                    .on_disabled_hover_text(NO_TRANSCRIBER_HINT);
                if transcribed.clicked() && self.view != View::Transcribed {
                    self.view = View::Transcribed;
                    self.transcribe_selected();
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
                    if ui.button("Delete").clicked() {
                        self.request_delete();
                    }
                    let sort =
                        ui.add_enabled(!self.pages.is_sorted(), egui::Button::new("Sort by name"));
                    if sort.clicked() {
                        self.sort_pages();
                    }
                    ui.checkbox(&mut self.debug, "Debug");
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
                    if export.clicked() {
                        self.export.open_dialog();
                    }
                    let reexport = ui.add_enabled(
                        can_export && self.export.can_reexport(),
                        egui::Button::new("Re-export"),
                    );
                    if reexport.clicked() {
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
                    if transcribe_all.clicked() {
                        if untranscribed > TRANSCRIBE_ALL_CONFIRM_ABOVE {
                            self.confirm_transcribe_all = true;
                        } else {
                            self.transcribe_all();
                        }
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.view == View::Transcribed {
                self.transcribed(ui);
                return;
            }
            let name = self
                .pages
                .get(self.selected_index())
                .map(|p| p.name.clone());
            if let (Some(name), Some(showing)) = (name, self.current_texture(ui.ctx())) {
                let panel = ui.available_rect_before_wrap();
                let image_rect = self.show_page(ui, &name, &showing, panel);
                if self.debug
                    && let Some(image_rect) = image_rect
                {
                    self.debug_overlay(ui, &name, image_rect);
                }
            }
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

    /// The page named `name` as large as fits in `rect`, centred, returning where it went. A
    /// placeholder gets a spinner, and a page that's loading only a spinner.
    fn show_page(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        showing: &Showing,
        rect: Rect,
    ) -> Option<Rect> {
        let spinner = Rect::from_center_size(rect.center(), Vec2::splat(32.0));
        let Some(texture) = showing.texture() else {
            egui::Spinner::new().paint_at(ui, spinner);
            return None;
        };
        let alt_text = match (showing, self.view.shows_cleaned()) {
            (Showing::Placeholder(_), _) => format!("Current: {name} (loading)"),
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
        let showing = self.current_texture(ui.ctx());
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
        let (mut page_rect, text_rect) = self.split.show(ui, rect, gap, limits);
        if !transcribing && let Some(status) = self.transcript_status(ui, index, page_rect) {
            page_rect.max.y = status.top() - ui.spacing().item_spacing.y;
        }
        let image_rect = showing.and_then(|showing| self.show_page(ui, &name, &showing, page_rect));
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
        let name = page.name.clone();
        let when = when_and_by(transcript, &TimeZone::system());
        let corrected = transcript.corrected();
        let error = self.transcriptions.error(&name);
        let text = match error {
            Some(err) => RichText::new(format!(
                "Retranscription failed: {err}. Using the cached version, transcribed {when}."
            ))
            .color(ui.visuals().error_fg_color),
            None if transcript.from_cache => {
                RichText::new(format!("From cache: transcribed {when}"))
            }
            None => RichText::new(format!("Transcribed {when}")),
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
                ui.button(FORCE_LABEL).clicked()
            })
            .inner
        });
        if scope.inner {
            if corrected {
                self.confirm_force = Some(name);
            } else {
                self.force(&name);
            }
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
            ui.button("Retry").clicked()
        } else if self.transcriptions.checking(name) {
            spinner(ui, &format!("Looking for {name} in the cache…"));
            false
        } else {
            ui.label(format!("{name} isn't transcribed yet."));
            ui.button("Transcribe").clicked()
        };
        if start {
            let params = self.clean_params.clone();
            self.transcriptions.start(name, page.bytes.clone(), params);
        }
    }

    /// The detected page outline over the original, and what cleaning found, for tuning cleanup.
    fn debug_overlay(&mut self, ui: &mut egui::Ui, name: &str, image_rect: Rect) {
        let report = self.pages.find(name).and_then(|p| p.loaded.as_ref());
        let Some(report) = report.map(|l| l.report.clone()) else {
            return;
        };
        let outline = report.outline.as_ref();
        if let (View::Original, Ok(quad)) = (self.view, outline) {
            let (w, h) = report.source_size;
            let to_screen = |[x, y]: [f32; 2]| {
                image_rect.min + Vec2::new(x / w as f32, y / h as f32) * image_rect.size()
            };
            let corners: Vec<Pos2> = quad.iter().copied().map(to_screen).collect();
            let painter = ui.painter_at(image_rect.expand(4.0));
            painter.add(Shape::closed_line(
                corners.clone(),
                Stroke::new(2.0, Color32::RED),
            ));
            for corner in corners {
                painter.circle_filled(corner, 4.0, Color32::RED);
            }
        }

        let lighting = &report.lighting;
        let mut lines = vec![
            match outline {
                Ok(quad) => format!("Page outline: {}", format_quad(quad)),
                Err(reason) => format!("No page outline: {reason}"),
            },
            format!(
                "Output: {} x {}",
                report.output_size.0, report.output_size.1
            ),
            format!("Paper colour: {:?}", lighting.paper_rgb),
            format!(
                "Black / white point: {} / {}",
                lighting.black_point, lighting.white_point
            ),
        ];
        lines.extend(
            format!("{:#?}", self.clean_params)
                .lines()
                .map(str::to_owned),
        );
        egui::Area::new(egui::Id::unique("clean_debug"))
            .order(egui::Order::Foreground)
            .fixed_pos(image_rect.left_top() + Vec2::splat(8.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    for line in lines {
                        ui.monospace(line);
                    }
                });
            });
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

    /// The selected page at full size in the current view, once it's decoded. Until then, its
    /// thumbnail while the full image is decoded off the UI thread.
    fn current_texture(&mut self, ctx: &egui::Context) -> Option<Showing> {
        let page = self.pages.get(self.selected_index())?;
        let cleaned = self.view.shows_cleaned();
        let current = self.current.as_ref();
        if let Some(shown) = current.filter(|s| s.name == page.name && s.cleaned == cleaned) {
            return Some(Showing::Full(shown.texture.clone()));
        }
        let Some(loaded) = &page.loaded else {
            return Some(Showing::Placeholder(None));
        };
        let key = (page.name.clone(), cleaned);
        if self.decodes.wanted.as_ref() != Some(&key) {
            let name = page.name.clone();
            let jpeg = if cleaned {
                loaded.cleaned.clone()
            } else {
                loaded.original.clone()
            };
            let max_side = ctx.input(|i| i.max_texture_side) as u32;
            self.decodes.jobs.spawn(key.clone(), move || {
                let img = timed(format_args!("{name}: decode"), || decode(&jpeg));
                let img = img.map_err(|e| e.to_string())?;
                Ok(color_image(&shrink_to_fit(img, max_side)))
            });
            self.decodes.wanted = Some(key);
        }
        let thumbs = self.thumbs.get(&page.name);
        let thumb = thumbs.map(|t| if cleaned { &t.cleaned } else { &t.original });
        Some(Showing::Placeholder(thumb.cloned()))
    }

    /// Puts the image the viewer is waiting for on screen once it's decoded. Others are stale.
    fn show_decoded(&mut self, ctx: &egui::Context) {
        while let Some(((name, cleaned), image)) = self.decodes.jobs.next() {
            if self.decodes.wanted.as_ref() != Some(&(name.clone(), cleaned)) {
                continue;
            }
            self.decodes.wanted = None;
            let texture = match image {
                Ok(image) => timed(format_args!("{name}: load texture"), || {
                    ctx.load_texture(&name, image, TextureOptions::LINEAR)
                }),
                // It decoded when the page loaded, so this shouldn't happen.
                Err(err) => {
                    log::error!("Couldn't decode {name}: {err}");
                    let Some(thumbs) = self.thumbs.get(&name) else {
                        continue;
                    };
                    if cleaned {
                        thumbs.cleaned.clone()
                    } else {
                        thumbs.original.clone()
                    }
                }
            };
            self.current = Some(Shown {
                name,
                cleaned,
                texture,
            });
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

fn format_quad(quad: &[[f32; 2]; 4]) -> String {
    let corners: Vec<String> = quad
        .iter()
        .map(|[x, y]| format!("({x:.0}, {y:.0})"))
        .collect();
    corners.join(" ")
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
        if let Some(cpu) = frame.info().cpu_usage.filter(|&s| s > 0.05) {
            log::debug!("slow frame: {:.0} ms", cpu * 1000.0);
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
