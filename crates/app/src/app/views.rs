//! The selected page in each [`View`](super::View): its photo, its photo with corners to drag beside the
//! cleaned page, or the cleaned page beside its transcript.

use eframe::egui;
use egui::{Rect, UiBuilder, Vec2};
use notebook_core::geom::rect_corners;

use super::NotebookApp;
use super::textures::Showing;
use crate::command::Command;
use crate::t;
use crate::ui::{icons, split, transcript_view};

impl NotebookApp {
    /// What the viewer shows of the selected page: its photo, or its `cleaned` image.
    fn showing(&mut self, ctx: &egui::Context, cleaned: bool) -> Option<Showing> {
        let page = self.pages.get(self.selected_index())?;
        Some(self.textures.get(ctx, page, cleaned))
    }

    /// The selected page's photo.
    pub(super) fn show_original(&mut self, ui: &mut egui::Ui) {
        let name = self
            .pages
            .get(self.selected_index())
            .map(|p| p.name.clone());
        if let (Some(name), Some(showing)) = (name, self.showing(ui.ctx(), false)) {
            let panel = ui.available_rect_before_wrap();
            self.show_page(ui, &name, &showing, panel, false);
        }
    }

    /// The selected page's photo with its corners to drag, beside the page cleaned with them. A
    /// page whose photo is missing says so once, under its (greyed out) corner buttons.
    pub(super) fn show_cleaned(&mut self, ui: &mut egui::Ui) {
        let index = self.selected_index();
        let Some(name) = self.pages.get(index).map(|p| p.name.clone()) else {
            return;
        };
        let rect = ui.available_rect_before_wrap();
        if let Some(showing @ Showing::Missing(_)) = self.showing(ui.ctx(), false) {
            let rect = self.show_corner_buttons(ui, index, rect);
            self.show_page(ui, &name, &showing, rect, false);
            return;
        }
        let gap = ui.spacing().item_spacing.x * 2.0;
        let limits = split::Limits::default();
        let (photo_rect, cleaned_rect) = self.cleaned_split.show(
            ui,
            rect,
            gap,
            limits,
            "cleaned split",
            &t!("resize-cleaned"),
        );
        let photo_rect = self.show_corner_buttons(ui, index, photo_rect);
        if let Some(showing) = self.showing(ui.ctx(), false)
            && let Some(image_rect) = self.show_page(ui, &name, &showing, photo_rect, false)
        {
            self.edit_corners(ui, index, image_rect);
        }
        if let Some(showing) = self.showing(ui.ctx(), true) {
            self.show_page(ui, &name, &showing, cleaned_rect, true);
        }
    }

    /// Buttons to place page `index`'s corners or put them back, at the top of `rect`. Returns
    /// the rest of `rect`.
    pub(super) fn show_corner_buttons(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        rect: Rect,
    ) -> Rect {
        let Some(page) = self.pages.get(index) else {
            return rect;
        };
        let prompt = self.corners.prompt(&page.name);
        let row = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ui.horizontal(|ui| {
                let place = self.command_button(ui, Command::PlaceCorners).clicked();
                let reset = self.command_button(ui, Command::ResetCorners).clicked();
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
    pub(super) fn edit_corners(&mut self, ui: &mut egui::Ui, index: usize, image_rect: Rect) {
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
            .unwrap_or_else(|| rect_corners(w, h));
        let changed = self
            .corners
            .show(ui, &page.name, quad, report.source_size, image_rect);
        if let Some(corners) = changed {
            self.set_corners(index, Some(corners));
        }
    }

    /// The page named `name`, its `cleaned` image or its photo, as large as fits in `rect`,
    /// centred, returning where it went. A placeholder gets a spinner, and a page that's loading
    /// only a spinner. A page whose photo is missing says why instead.
    pub(super) fn show_page(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        showing: &Showing,
        rect: Rect,
        cleaned: bool,
    ) -> Option<Rect> {
        if let Showing::Missing(why) = showing {
            show_missing(ui, why, rect);
            return None;
        }
        let spinner = Rect::from_center_size(rect.center(), Vec2::splat(32.0));
        let Some(texture) = showing.texture() else {
            egui::Spinner::new().paint_at(ui, spinner);
            return None;
        };
        let alt_text = match (showing, cleaned) {
            (Showing::Placeholder(_), false) => t!("current-page-loading", name = name),
            (Showing::Placeholder(_), true) => t!("current-page-cleaned-loading", name = name),
            (Showing::Full(_), true) => t!("current-page-cleaned", name = name),
            (Showing::Full(_), false) => t!("current-page", name = name),
            (Showing::Missing(_), _) => return None,
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
    pub(super) fn show_transcribed(&mut self, ui: &mut egui::Ui) {
        let index = self.selected_index();
        let Some(name) = self.pages.get(index).map(|p| p.name.clone()) else {
            return;
        };
        // A page being retranscribed shows progress, not the transcript it's replacing.
        let transcribing = self.transcriptions.queued_or_running(&name);
        let rect = ui.available_rect_before_wrap();
        let showing = self.showing(ui.ctx(), true);
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
        let (mut page_rect, text_rect) = self.transcript_split.show(
            ui,
            rect,
            gap,
            limits,
            "transcript split",
            &t!("resize-transcript"),
        );
        if !transcribing && let Some(status) = self.show_transcript_status(ui, index, page_rect) {
            page_rect.max.y = status.top() - ui.spacing().item_spacing.y;
        }
        let image_rect =
            showing.and_then(|showing| self.show_page(ui, &name, &showing, page_rect, true));
        let page = self.pages.get_mut(index);
        let crop = page
            .as_ref()
            .and_then(|p| Some(p.loaded.as_ref()?.report.crop()));
        let transcript = page
            .and_then(|p| p.transcript.as_mut())
            .filter(|_| !transcribing);
        if let Some(transcript) = transcript {
            let image = image_rect.zip(crop);
            let events = self
                .transcript_view
                .show(ui, &name, transcript, image, text_rect);
            for event in events {
                self.eval_log.record(event);
            }
        } else {
            ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
                self.show_transcription_progress(ui, index);
            });
        }
    }
}

/// In place of a page whose photo is missing: that it is, and `why`, centred in `rect`. Nothing
/// here takes the focus.
fn show_missing(ui: &mut egui::Ui, why: &str, rect: Rect) {
    let layout = egui::Layout::top_down(egui::Align::Center);
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(layout), |ui| {
        // Roughly centred: the title and a line or two of reason.
        ui.add_space((rect.height() / 2.0 - 48.0).max(0.0));
        let warn = ui.visuals().warn_fg_color;
        let (icon, _) = ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::hover());
        icons::paint_warning(ui.painter(), icon, warn);
        let title = egui::RichText::new(t!("missing-photo-title")).heading();
        ui.add(egui::Label::new(title.color(warn)).selectable(false));
        ui.add(egui::Label::new(why).wrap().selectable(false));
    });
}
