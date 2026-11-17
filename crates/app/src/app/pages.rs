//! Adding, loading, deleting, moving and transcribing pages, and the dialogs that confirm the ones
//! that would lose work.

use eframe::egui;
use notebook_core::geom::Quad;
use notebook_core::page::{Page, PageId};
use notebook_core::relink::{Dropped, Shape};
use notebook_core::transcript::Transcript;

use super::commands::shortcut_text;
use super::project::After;
use super::{NotebookApp, View, relink};
use crate::command::Command;
use crate::eval_log::{Event, PageRef};
use crate::ingest::{Dropping, Ingest, Pick, Received};
use crate::t;
use crate::ui::dialog;
use crate::ui::dialog::{Choice, Stakes};
use crate::ui::transcript_view::TranscriptView;

/// Transcribe All asks first when it would send more pages than this. See
/// `decisions/2026-10-24-confirm-before-transcribing.md`.
pub(super) const TRANSCRIBE_ALL_CONFIRM_ABOVE: usize = 4;

/// What the user tried to do when they were asked for an API key, to carry on with once they've
/// entered one.
pub(super) enum AfterKey {
    Transcribe(PageId),
    TranscribeAll,
    Retranscribe(PageId),
}

impl NotebookApp {
    pub(super) fn select(&mut self, index: usize) {
        if let Some(page) = self.pages.get(index) {
            self.selected = Some(page.id);
        }
    }

    /// Switching to the Transcribed view transcribes the page shown.
    pub(super) fn set_view(&mut self, view: View) {
        let switched = self.view != view;
        self.view = view;
        if switched && view == View::Transcribed {
            self.auto_transcribe_selected();
        }
    }

    /// Asks which files to add, and adds them as pages.
    pub(super) fn add_picked(&mut self, ctx: &egui::Context) {
        let Some(pick_files) = self.pick_files.clone() else {
            return;
        };
        self.asked_which_files = true;
        let received = Ingest::read_paths(&pick_files(Pick::Photos));
        self.add_received(ctx, received);
    }

    /// Adds the files dropped this frame as pages, or opens the project dropped among them,
    /// leaving the rest out. See `decisions/2026-10-31-open-by-drop-and-argument.md`.
    pub(super) fn add_dropped(&mut self, ctx: &egui::Context) {
        let dropping = Ingest::dropped(ctx);
        self.take_files(ctx, dropping, |ingest| ingest.read_drops(ctx));
    }

    /// Takes the files the system asked the app to open since the last frame as if each batch had
    /// been dropped on the window. See `decisions/2026-11-15-macos-bundle.md`.
    pub(super) fn add_opened(&mut self, ctx: &egui::Context) {
        let Some(opened) = &self.opened_files else {
            return;
        };
        let batches: Vec<_> = opened.try_iter().collect();
        for paths in batches {
            let dropping = Dropping::of(paths.iter().map(|path| Some(path.as_path())));
            self.take_files(ctx, dropping, |_| Ingest::read_paths(&paths));
        }
    }

    /// Does what `dropping` some files does, with `read` reading them if they're to be pages.
    fn take_files(
        &mut self,
        ctx: &egui::Context,
        dropping: Dropping,
        read: impl FnOnce(&Ingest) -> Vec<Received>,
    ) {
        match dropping {
            Dropping::AddPages => {
                let received = read(&self.ingest);
                self.add_received(ctx, received);
            }
            // The question already asked is answered first.
            Dropping::OpenProject { .. } if self.dialog_open() => {}
            Dropping::OpenProject { path, others } => {
                if others > 0 {
                    self.show_hint(ctx, t!("drop-others-left-out", count = others));
                }
                self.after_saving(ctx, After::OpenProject(path));
            }
            Dropping::SeveralProjects => self.errors.push(t!("drop-several-projects")),
        }
    }

    /// Adds `received` as pages, and starts loading them. Pages are usable before they've
    /// loaded: they can be moved, deleted and transcribed. A photo that's already a page is
    /// skipped, and one that a page is missing is given to it, without asking. One that only looks
    /// like a missing page's photo, by its EXIF, asks whether it is, after the rest are added. See
    /// `decisions/2026-11-15-page-ids.md` and `decisions/2026-11-17-relink-photos.md`.
    pub(super) fn add_received(&mut self, ctx: &egui::Context, received: Vec<Received>) {
        if !received.is_empty() {
            self.errors.clear();
        }
        for (name, path, bytes) in received {
            let photo =
                bytes.and_then(|bytes| relink::read_photo(name.clone(), path, bytes.into()));
            let photo = match photo {
                Ok(photo) => photo,
                Err(err) => {
                    self.open_failed(&name, err);
                    continue;
                }
            };
            let missing_it: Vec<PageId> = self
                .pages
                .with_photo(&photo.sha256)
                .filter(|p| p.missing_photo().is_some())
                .map(|p| p.id)
                .collect();
            if !missing_it.is_empty() {
                for id in missing_it {
                    self.fill_in(ctx, id, &photo);
                }
                continue;
            }
            // A photo that's already a page is left out.
            if self.pages.with_photo(&photo.sha256).next().is_some() {
                continue;
            }
            match notebook_core::relink::dropped(&self.pages, &photo) {
                Dropped::Ask(candidates) => self.ask_which_page(photo, candidates),
                Dropped::OtherShape(pages) => {
                    let hint = t!(
                        "relink-dropped-other-shape",
                        file = photo.name.as_str(),
                        pages = pages.join(", ")
                    );
                    self.show_hint(ctx, hint);
                    self.add_page(photo);
                }
                Dropped::Unrelated => self.add_page(photo),
            }
        }
    }

    /// Gives page `id`, missing its photo, `photo`, the same file, and loads it.
    fn fill_in(&mut self, ctx: &egui::Context, id: PageId, photo: &Page) {
        let Some(page) = self.pages.find_mut(id) else {
            return;
        };
        let hint = t!("photo-found", name = page.name.as_str());
        page.relink(photo, Shape::Same);
        self.start_loading(id);
        self.selected = Some(id);
        self.show_hint(ctx, hint);
    }

    /// Adds `page` and starts loading it, unless its photo is already a page's.
    pub(super) fn add_page(&mut self, page: Page) {
        if self.pages.with_photo(&page.sha256).next().is_some() {
            return;
        }
        let id = page.id;
        self.pages.add(page);
        self.start_loading(id);
    }

    /// Starts loading page `id`, if it has its photo, and looking it up in the cache if it
    /// isn't transcribed.
    pub(super) fn start_loading(&mut self, id: PageId) {
        let Some(page) = self.pages.find(id) else {
            return;
        };
        let Some(bytes) = page.bytes().cloned() else {
            return;
        };
        if page.transcript.is_none() {
            self.transcriptions.look_up(id, bytes.clone());
        }
        let corners = page.corners;
        self.ingest
            .load(id, bytes, self.clean_params.clone(), corners);
    }

    /// Fills in pages that finished loading or cleaning again, and removes those that couldn't be
    /// loaded.
    pub(super) fn add_loaded(&mut self, ctx: &egui::Context) {
        while let Some((id, loaded)) = self.ingest.next() {
            // The page may have been deleted meanwhile, or given other corners.
            let Some(index) = self.pages.position(id).filter(|&i| {
                self.pages.get(i).is_some_and(|p| {
                    let corners = loaded.as_ref().map_or(p.corners, |l| l.corners);
                    corners == p.corners
                })
            }) else {
                continue;
            };
            match loaded {
                Ok(mut loaded) => {
                    let Some(page) = self.pages.get_mut(index) else {
                        continue;
                    };
                    // Cleaned again: the photo is the same, so its texture stays.
                    if let Some(old) = page.loaded.take() {
                        loaded.photo_jpeg = old.photo_jpeg;
                    }
                    self.textures.add_thumbnails(ctx, page, &loaded);
                    page.loaded = Some(loaded);
                }
                Err(err) => {
                    let name = self.pages.get(index).map(|p| p.name.clone());
                    self.open_failed(&name.unwrap_or_default(), err);
                    self.delete_page(index);
                }
            }
        }
    }

    /// Says in the status line that the file `name` couldn't be opened, and why.
    fn open_failed(&mut self, name: &str, error: String) {
        self.errors
            .push(t!("open-failed", name = name, error = error));
    }

    /// Removes page `index`. If it was shown, selects the one after it, or before it if it was
    /// the last.
    pub(super) fn delete_page(&mut self, index: usize) {
        let shown = self.selected_index() == index;
        if !shown {
            self.pin_selection();
        }
        let Some(page) = self.pages.remove(index) else {
            return;
        };
        self.textures.forget(page.id);
        self.transcriptions.forget(page.id);
        if !shown {
            return;
        }
        self.transcript_view = TranscriptView::default();
        let next = self
            .pages
            .get(index)
            .or_else(|| self.pages.get(index.wrapping_sub(1)));
        self.selected = next.map(|p| p.id);
    }

    /// Deletes the selected page, asking first if it has a transcript.
    pub(super) fn request_delete(&mut self) {
        let index = self.selected_index();
        let Some(page) = self.pages.get(index) else {
            return;
        };
        if page.transcript.is_some() {
            self.confirm_delete = Some(page.id);
        } else {
            self.delete_page(index);
        }
    }

    pub(super) fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, name)) = self.asking_about(self.confirm_delete) else {
            self.confirm_delete = None;
            return;
        };
        let choice = dialog::show(ctx, "confirm delete", 320.0, |ui| {
            ui.label(t!("confirm-delete", name = name.as_str()));
            dialog::buttons(ui, &t!("confirm-delete-button"), Stakes::Dangerous, true)
        });
        if choice == Some(Choice::Affirmative)
            && let Some(index) = self.pages.position(id)
        {
            self.delete_page(index);
        }
        if choice.is_some() {
            self.confirm_delete = None;
        }
    }

    /// The page a dialog is asking about, and its name, unless it's gone.
    fn asking_about(&self, id: Option<PageId>) -> Option<(PageId, String)> {
        let page = self.pages.find(id?)?;
        Some((page.id, page.name.clone()))
    }

    /// Pages without a transcript that aren't being transcribed, including any that failed, and
    /// not those whose photos are missing, which can't be.
    pub(super) fn untranscribed(&self) -> Vec<&Page> {
        self.pages
            .iter()
            .filter(|p| {
                p.transcript.is_none()
                    && p.bytes().is_some()
                    && !self.transcriptions.queued_or_running(p.id)
            })
            .collect()
    }

    /// Queues every page [`Self::untranscribed`]. Cached ones aren't sent.
    pub(super) fn transcribe_all(&mut self) {
        let ids: Vec<PageId> = self.untranscribed().into_iter().map(|p| p.id).collect();
        for id in ids {
            if let Some(page) = self.pages.find(id) {
                self.transcriptions
                    .start_later(page, self.clean_params.clone());
            }
        }
    }

    /// Whether pages can be sent to Claude now. If not, asks for an API key, if the app can keep
    /// one, to do `then` with once the user enters it.
    fn ready_to_send(&mut self, then: AfterKey) -> bool {
        if self.transcriptions.can_send() {
            return true;
        }
        if self.auth.is_some() {
            self.after_key = Some(then);
            self.key_dialog.open();
        }
        false
    }

    /// Does what the user was doing when they were asked for an API key.
    pub(super) fn carry_on(&mut self, ctx: &egui::Context, after: AfterKey) {
        match after {
            AfterKey::Transcribe(id) => {
                if let Some(index) = self.pages.position(id) {
                    self.start_transcribing(index);
                }
            }
            AfterKey::TranscribeAll => self.request_transcribe_all(),
            AfterKey::Retranscribe(id) => {
                if let Some(index) = self.pages.position(id) {
                    self.request_retranscribe(index);
                }
            }
        }
        ctx.request_repaint();
    }

    /// Transcribes every untranscribed page, asking first if there are many.
    pub(super) fn request_transcribe_all(&mut self) {
        if !self.ready_to_send(AfterKey::TranscribeAll) {
            return;
        }
        if self.untranscribed().len() > TRANSCRIBE_ALL_CONFIRM_ABOVE {
            self.confirm_transcribe_all = true;
        } else {
            self.transcribe_all();
        }
    }

    pub(super) fn transcribe_all_dialog(&mut self, ctx: &egui::Context) {
        if !self.confirm_transcribe_all {
            return;
        }
        let count = self.untranscribed().len();
        let choice = dialog::show(ctx, "confirm transcribe all", 320.0, |ui| {
            ui.label(t!("confirm-transcribe-all", count = count));
            let button = t!("confirm-transcribe-all-button");
            dialog::buttons(ui, &button, Stakes::Safe, count > 0)
        });
        if choice == Some(Choice::Affirmative) {
            self.transcribe_all();
        }
        if choice.is_some() {
            self.confirm_transcribe_all = false;
        }
    }

    /// Retranscribes page `id`, skipping the cache.
    pub(super) fn retranscribe(&mut self, id: PageId) {
        if let Some(page) = self.pages.find(id) {
            self.transcriptions
                .retranscribe(page, self.clean_params.clone());
        }
    }

    /// Retranscribes page `index`, asking first if it's been corrected. See
    /// `decisions/2026-10-24-cache-status.md`.
    pub(super) fn request_retranscribe(&mut self, index: usize) {
        let Some(page) = self.pages.get(index) else {
            return;
        };
        let corrected = page.transcript.as_ref().is_some_and(Transcript::corrected);
        let id = page.id;
        if !self.ready_to_send(AfterKey::Retranscribe(id)) {
            return;
        }
        if corrected {
            self.confirm_retranscribe = Some(id);
        } else {
            self.retranscribe(id);
        }
    }

    pub(super) fn retranscribe_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, name)) = self.asking_about(self.confirm_retranscribe) else {
            self.confirm_retranscribe = None;
            return;
        };
        let choice = dialog::show(ctx, "confirm force", 320.0, |ui| {
            ui.label(t!("confirm-force", name = name.as_str()));
            dialog::buttons(ui, &t!("confirm-force-button"), Stakes::Dangerous, true)
        });
        if choice == Some(Choice::Affirmative) {
            self.retranscribe(id);
        }
        if choice.is_some() {
            self.confirm_retranscribe = None;
        }
    }

    /// Drops page `index`'s corrections, once the user confirms.
    pub(super) fn request_reset(&mut self, index: usize) {
        self.confirm_reset = self.pages.get(index).map(|p| p.id);
    }

    pub(super) fn reset_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, name)) = self.asking_about(self.confirm_reset) else {
            self.confirm_reset = None;
            return;
        };
        let choice = dialog::show(ctx, "confirm reset", 320.0, |ui| {
            ui.label(t!("confirm-reset", name = name.as_str()));
            dialog::buttons(ui, &t!("confirm-reset-button"), Stakes::Dangerous, true)
        });
        if choice == Some(Choice::Affirmative)
            && let Some(page) = self.pages.find_mut(id)
            && let Some(transcript) = page.transcript.as_mut()
        {
            transcript.reset();
            self.eval_log.record(Event::Reset {
                page: PageRef { id, name },
                model: transcript.model.clone(),
            });
            // Claude's transcription may be in an older style.
            self.restyle_if_stale(id);
        }
        if choice.is_some() {
            self.confirm_reset = None;
        }
    }

    /// On switching to the Transcribed view: transcribes the selected page, unless it's
    /// transcribed, being transcribed, or failed (which waits for Retry).
    pub(super) fn auto_transcribe_selected(&mut self) {
        let index = self.selected_index();
        let Some(page) = self.pages.get(index) else {
            return;
        };
        // Without credentials, it waits for the user to ask, rather than asking for a key.
        if page.transcript.is_none()
            && page.bytes().is_some()
            && self.transcriptions.error(page.id).is_none()
            && self.transcriptions.can_send()
        {
            self.start_transcribing(index);
        }
    }

    /// Shows the selected page's transcript, transcribing the page if it isn't, or retrying if
    /// that failed. If it's transcribed, says how to retranscribe it.
    pub(super) fn transcribe_selected(&mut self, ctx: &egui::Context) {
        self.view = View::Transcribed;
        let index = self.selected_index();
        let Some(page) = self.pages.get(index) else {
            return;
        };
        if self.transcriptions.queued_or_running(page.id) {
            return;
        }
        if page.transcript.is_some() {
            let force = shortcut_text(ctx, Command::ForceRetranscribe);
            let hint = t!(
                "already-transcribed",
                name = page.name.as_str(),
                shortcut = force
            );
            self.show_hint(ctx, hint);
        } else {
            self.start_transcribing(index);
        }
    }

    /// Sends page `index` to be transcribed, unless it's in the cache. Without credentials, asks
    /// for an API key first.
    pub(super) fn start_transcribing(&mut self, index: usize) {
        let Some(id) = self.pages.get(index).map(|p| p.id) else {
            return;
        };
        if !self.ready_to_send(AfterKey::Transcribe(id)) {
            return;
        }
        if let Some(page) = self.pages.get(index) {
            self.transcriptions.start(page, self.clean_params.clone());
        }
    }

    /// Shows page `index` in the Cleaned view, ready for its corners to be clicked in.
    pub(super) fn place_corners(&mut self, index: usize) {
        self.set_view(View::Cleaned);
        if let Some(page) = self.pages.get(index) {
            self.corners.place(page.id);
        }
    }

    /// Puts page `index`'s corners at `corners`, or back where cleaning finds them, and cleans it
    /// again with them. A page whose photo is missing keeps its corners.
    pub(super) fn set_corners(&mut self, index: usize, corners: Option<Quad>) {
        let Some(page) = self.pages.get_mut(index) else {
            return;
        };
        let Some(bytes) = page.bytes().cloned() else {
            return;
        };
        page.corners = corners;
        if page.loaded.as_ref().is_some_and(|l| l.corners == corners) {
            return;
        }
        let params = self.clean_params.clone();
        self.ingest.load(page.id, bytes, params, corners);
    }

    /// Moves page `from` to index `to`, keeping the shown page shown. See
    /// `decisions/2026-10-24-page-order.md`.
    pub(super) fn move_page(&mut self, from: usize, to: usize) {
        self.pin_selection();
        self.pages.move_page(from, to);
    }

    /// Puts the pages back in filename order, keeping the shown page shown.
    pub(super) fn sort_pages(&mut self) {
        self.pin_selection();
        self.pages.sort_by_name();
    }

    /// Selects the shown page, so it stays shown when the pages move. Until the user picks one,
    /// the shown page is whichever is first.
    pub(super) fn pin_selection(&mut self) {
        self.selected = self.pages.get(self.selected_index()).map(|p| p.id);
    }

    pub(super) fn selected_index(&self) -> usize {
        let id = self.selected;
        id.and_then(|id| self.pages.position(id)).unwrap_or(0)
    }
}
