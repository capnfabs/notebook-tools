//! Adding, loading, deleting, moving and transcribing pages, and the dialogs that confirm the ones
//! that would lose work.

use std::sync::Arc;

use eframe::egui;
use notebook_core::geom::Quad;
use notebook_core::page::Page;
use notebook_core::transcript::Transcript;

use super::commands::shortcut_text;
use super::{NotebookApp, View};
use crate::command::Command;
use crate::eval_log::Event;
use crate::ingest::{Ingest, Received};
use crate::t;
use crate::ui::dialog;
use crate::ui::dialog::{Choice, Stakes};
use crate::ui::transcript_view::TranscriptView;

/// Transcribe All asks first when it would send more pages than this. See
/// `decisions/2026-10-24-confirm-before-transcribing.md`.
pub(super) const TRANSCRIBE_ALL_CONFIRM_ABOVE: usize = 4;

impl NotebookApp {
    pub(super) fn select(&mut self, index: usize) {
        if let Some(page) = self.pages.get(index) {
            self.selected = Some(page.name.clone());
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
    pub(super) fn add_picked(&mut self) {
        let Some(pick_files) = self.pick_files.clone() else {
            return;
        };
        self.asked_which_files = true;
        let received = Ingest::read_paths(&pick_files());
        self.add_received(received);
    }

    /// Adds the files dropped this frame as pages.
    pub(super) fn add_dropped(&mut self, ctx: &egui::Context) {
        let received = self.ingest.read_drops(ctx);
        self.add_received(received);
    }

    /// Adds `received` as pages, and starts loading them. Pages are usable before they've
    /// loaded: they can be moved, deleted and transcribed.
    pub(super) fn add_received(&mut self, received: Vec<Received>) {
        if !received.is_empty() {
            self.errors.clear();
        }
        for (name, bytes) in received {
            let bytes: Arc<[u8]> = match bytes {
                Ok(bytes) => bytes.into(),
                Err(err) => {
                    self.open_failed(&name, err);
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
    pub(super) fn add_loaded(&mut self, ctx: &egui::Context) {
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
                    // Cleaned again: the photo is the same, so its texture stays.
                    if let Some(old) = page.loaded.take() {
                        loaded.photo_jpeg = old.photo_jpeg;
                    }
                    self.textures.add_thumbnails(ctx, &name, &loaded);
                    page.loaded = Some(loaded);
                }
                Err(err) => {
                    self.open_failed(&name, err);
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
        self.textures.forget(&page.name);
        self.transcriptions.forget(&page.name);
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
    pub(super) fn request_delete(&mut self) {
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

    pub(super) fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = self.confirm_delete.clone() else {
            return;
        };
        let choice = dialog::show(ctx, "confirm delete", 320.0, |ui| {
            ui.label(t!("confirm-delete", name = name.as_str()));
            dialog::buttons(ui, &t!("confirm-delete-button"), Stakes::Dangerous, true)
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
    pub(super) fn untranscribed(&self) -> Vec<&Page> {
        self.pages
            .iter()
            .filter(|p| p.transcript.is_none() && !self.transcriptions.queued_or_running(&p.name))
            .collect()
    }

    /// Queues every page [`Self::untranscribed`]. Cached ones aren't sent.
    pub(super) fn transcribe_all(&mut self) {
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
    pub(super) fn request_transcribe_all(&mut self) {
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

    /// Retranscribes the page named `name`, skipping the cache.
    pub(super) fn retranscribe(&mut self, name: &str) {
        if let Some(page) = self.pages.find(name) {
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
        let name = page.name.clone();
        if corrected {
            self.confirm_retranscribe = Some(name);
        } else {
            self.retranscribe(&name);
        }
    }

    pub(super) fn retranscribe_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = self.confirm_retranscribe.clone() else {
            return;
        };
        let choice = dialog::show(ctx, "confirm force", 320.0, |ui| {
            ui.label(t!("confirm-force", name = name.as_str()));
            dialog::buttons(ui, &t!("confirm-force-button"), Stakes::Dangerous, true)
        });
        if choice == Some(Choice::Affirmative) {
            self.retranscribe(&name);
        }
        if choice.is_some() {
            self.confirm_retranscribe = None;
        }
    }

    /// Drops page `index`'s corrections, once the user confirms.
    pub(super) fn request_reset(&mut self, index: usize) {
        self.confirm_reset = self.pages.get(index).map(|p| p.name.clone());
    }

    pub(super) fn reset_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = self.confirm_reset.clone() else {
            return;
        };
        let choice = dialog::show(ctx, "confirm reset", 320.0, |ui| {
            ui.label(t!("confirm-reset", name = name.as_str()));
            dialog::buttons(ui, &t!("confirm-reset-button"), Stakes::Dangerous, true)
        });
        if choice == Some(Choice::Affirmative)
            && let Some(transcript) = self
                .pages
                .find_mut(&name)
                .and_then(|p| p.transcript.as_mut())
        {
            transcript.reset();
            self.eval_log.record(Event::Reset {
                page: name.clone(),
                model: transcript.model.clone(),
            });
            // Claude's transcription may be in an older style.
            self.restyle_if_stale(&name);
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
        if page.transcript.is_none() && self.transcriptions.error(&page.name).is_none() {
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
        if self.transcriptions.queued_or_running(&page.name) {
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

    /// Sends page `index` to be transcribed, unless it's in the cache.
    pub(super) fn start_transcribing(&mut self, index: usize) {
        if let Some(page) = self.pages.get(index) {
            self.transcriptions.start(page, self.clean_params.clone());
        }
    }

    /// Shows page `index` in the Cleaned view, ready for its corners to be clicked in.
    pub(super) fn place_corners(&mut self, index: usize) {
        self.set_view(View::Cleaned);
        if let Some(page) = self.pages.get(index) {
            self.corners.place(&page.name);
        }
    }

    /// Puts page `index`'s corners at `corners`, or back where cleaning finds them, and cleans it
    /// again with them.
    pub(super) fn set_corners(&mut self, index: usize, corners: Option<Quad>) {
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

    /// Selects the shown page by name, so it stays shown when the pages move. Until the user picks
    /// one, the shown page is whichever is first.
    pub(super) fn pin_selection(&mut self) {
        self.selected = self
            .pages
            .get(self.selected_index())
            .map(|p| p.name.clone());
    }

    pub(super) fn selected_index(&self) -> usize {
        let name = self.selected.as_ref();
        name.and_then(|name| self.pages.position(name)).unwrap_or(0)
    }
}
