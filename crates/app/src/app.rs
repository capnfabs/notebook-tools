use std::collections::HashMap;

use eframe::egui;
use egui::{
    Align, Color32, Layout, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle,
    TextureOptions, UiBuilder, Vec2,
};
use notebook_core::{
    CleanParams, CleanReport, Page, Pages, RgbaImage, clean, decode, shrink_to_fit,
};

use crate::export::{Export, SavePath};
use crate::ingest::Ingest;
use crate::timing::timed;
use crate::transcribe::{Transcriber, Transcriptions};
use crate::transcript_view::{self, TranscriptView};

/// Why the Transcribed view is unavailable.
const NO_TRANSCRIBER_HINT: &str =
    "Transcription needs ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN, and the desktop app";

/// Display size of the thumbnails in the bottom strip, in egui points (physical pixels =
/// points x `pixels_per_point`). Keep at most `THUMBNAIL_SIDE_PX` / 2 so 2x displays stay sharp.
const THUMB_SIZE_PT: f32 = 96.0;

/// Which version of the selected page the viewer shows.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum View {
    #[default]
    Original,
    Cleaned,
    /// Claude's transcription beside the cleaned page, for correcting. Selecting it transcribes
    /// the page, once.
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

#[derive(Default)]
pub struct NotebookApp {
    ingest: Ingest,
    pages: Pages,
    /// One per page, in the same order.
    thumbs: Vec<TextureHandle>,
    /// Name of the page the user picked. The first page is shown until they pick one.
    selected: Option<String>,
    /// Stays the same when another page is selected.
    view: View,
    /// Full-size texture for the selected page and view. Only one is kept: see
    /// `decisions/2026-10-23-pages-keep-encoded-bytes.md`.
    current: Option<Shown>,
    clean_params: CleanParams,
    /// What cleaning found, by page name. Small, so kept for every page cleaned so far.
    reports: HashMap<String, CleanReport>,
    /// Show the cleanup debug overlay.
    debug: bool,
    errors: Vec<String>,
    transcriptions: Transcriptions,
    transcript_view: TranscriptView,
    export: Export,
    /// The transcribed page the user asked to delete, until they confirm.
    confirm_delete: Option<String>,
}

impl NotebookApp {
    /// Without a transcriber, the Transcribed view is disabled.
    pub fn with_transcriber(transcriber: Option<Transcriber>) -> Self {
        Self {
            transcriptions: Transcriptions::new(transcriber),
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

    /// The app's UI, separate from [`eframe::App`] so tests can drive it directly.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.ingest.start_reading_drops(&ctx) > 0 {
            self.errors.clear();
        }
        self.add_next_page(&ctx);
        while let Some((name, transcript)) = self.transcriptions.next() {
            let index = self.pages.iter().position(|p| p.name == name);
            // The page may have been deleted meanwhile.
            if let Some(page) = index.and_then(|i| self.pages.get_mut(i)) {
                page.transcript = Some(transcript);
            }
        }

        if self.pages.is_empty() {
            self.drop_zone(ui);
        } else {
            self.viewer(ui);
        }
    }

    /// Decodes at most one received file per frame, so a big drop doesn't freeze the UI.
    fn add_next_page(&mut self, ctx: &egui::Context) {
        let Some((name, bytes)) = self.ingest.next() else {
            return;
        };
        ctx.request_repaint();

        let page = timed(format_args!("{name}: decode + thumbnail"), || {
            bytes.and_then(|b| Page::from_file(name.clone(), b).map_err(|e| e.to_string()))
        });
        let page = match page {
            Ok(page) => page,
            Err(err) => {
                self.errors.push(format!("Couldn't open {name}: {err}"));
                return;
            }
        };
        let thumb = timed(format_args!("{name}: thumbnail texture"), || {
            ctx.load_texture(&name, color_image(&page.thumbnail), TextureOptions::LINEAR)
        });
        // Duplicate names are skipped.
        if let Some(index) = self.pages.add(page) {
            self.thumbs.insert(index, thumb);
        }
    }

    /// Removes page `index`, selecting the one after it, or before it if it was the last.
    fn delete_page(&mut self, index: usize) {
        let Some(page) = self.pages.remove(index) else {
            return;
        };
        drop(self.thumbs.remove(index));
        self.reports.remove(&page.name);
        self.transcriptions.forget(&page.name);
        if self.current.as_ref().is_some_and(|c| c.name == page.name) {
            self.current = None;
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
        let (mut delete, mut cancel) = (false, false);
        let modal = egui::Modal::new(egui::Id::unique("confirm delete")).show(ctx, |ui| {
            ui.label(format!("Delete {name} and its transcript?"));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                delete = ui.button("Delete page").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if delete {
            let index = self.pages.iter().position(|p| p.name == name);
            if let Some(index) = index {
                self.delete_page(index);
            }
            self.confirm_delete = None;
        } else if cancel || modal.should_close() {
            self.confirm_delete = None;
        }
    }

    fn selected_index(&self) -> usize {
        let name = self.selected.as_ref();
        name.and_then(|name| self.pages.iter().position(|p| p.name == *name))
            .unwrap_or(0)
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
                ui.label("JPEG or PNG");
                self.status(ui);
            });
        });
    }

    fn viewer(&mut self, ui: &mut egui::Ui) {
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
                if transcribed.clicked() {
                    self.view = View::Transcribed;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Delete").clicked() {
                        self.request_delete();
                    }
                    ui.checkbox(&mut self.debug, "Debug");
                });
            });
        });

        egui::Panel::bottom("pages").show(ui, |ui| {
            ui.add_space(4.0);
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| self.thumbnails(ui));
            });
            ui.horizontal(|ui| {
                self.status(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Only transcribed pages can be exported.
                    let can_export = self.transcriptions.available();
                    let export = ui
                        .add_enabled(can_export, egui::Button::new("Export"))
                        .on_disabled_hover_text(NO_TRANSCRIBER_HINT);
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
            if let (Some(name), Some(texture)) = (name, self.current_texture(ui.ctx())) {
                let panel = ui.available_rect_before_wrap();
                let image_rect = self.show_page(ui, &name, &texture, panel);
                if self.debug {
                    self.debug_overlay(ui, &name, image_rect);
                }
            }
        });

        self.delete_dialog(ui.ctx());
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

    fn thumbnails(&mut self, ui: &mut egui::Ui) {
        let selected = self.selected_index();
        for (i, (page, thumb)) in self.pages.iter().zip(&self.thumbs).enumerate() {
            let response = ui
                .add(
                    egui::Image::new(thumb)
                        .max_size(Vec2::splat(THUMB_SIZE_PT))
                        .alt_text(&page.name)
                        .sense(Sense::click()),
                )
                .on_hover_text(&page.name);
            if i == selected {
                ui.painter().rect_stroke(
                    response.rect,
                    2.0,
                    ui.visuals().selection.stroke,
                    StrokeKind::Outside,
                );
            }
            if response.clicked() {
                self.selected = Some(page.name.clone());
            }
        }
    }

    /// The page named `name` as large as fits in `rect`, centred, returning where it went.
    fn show_page(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        texture: &TextureHandle,
        rect: Rect,
    ) -> Rect {
        let alt_text = if self.view.shows_cleaned() {
            format!("Current: {name} (cleaned)")
        } else {
            format!("Current: {name}")
        };
        let image = egui::Image::new(texture).shrink_to_fit().alt_text(alt_text);
        // Give the image exactly the rect it's drawn in. In a justified layout its response
        // would cover the whole panel, with the image centred inside.
        let size = image.calc_size(rect.size(), Some(texture.size_vec2()));
        ui.put(Rect::from_center_size(rect.center(), size), image)
            .rect
    }

    /// The cleaned page beside its transcript, for correcting.
    fn transcribed(&mut self, ui: &mut egui::Ui) {
        let index = self.selected_index();
        let Some(name) = self.pages.get(index).map(|p| p.name.clone()) else {
            return;
        };
        let gap = ui.spacing().item_spacing.x * 2.0;
        let (page_rect, text_rect) = transcript_view::split(ui.available_rect_before_wrap(), gap);
        let image_rect = self
            .current_texture(ui.ctx())
            .map(|texture| self.show_page(ui, &name, &texture, page_rect));
        let transcript = self
            .pages
            .get_mut(index)
            .and_then(|p| p.transcript.as_mut());
        if let Some(transcript) = transcript {
            self.transcript_view
                .show(ui, &name, transcript, image_rect, text_rect);
        } else {
            ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
                self.transcription_progress(ui, index);
            });
        }
    }

    /// Page `index` being transcribed, or why it couldn't be, starting it if it hasn't been
    /// tried yet.
    fn transcription_progress(&mut self, ui: &mut egui::Ui, index: usize) {
        let Some(page) = self.pages.get(index) else {
            return;
        };
        let name = &page.name;
        if let Some(err) = self.transcriptions.error(name) {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Couldn't transcribe {name}: {err}"),
            );
            if ui.button("Retry").clicked() {
                let params = self.clean_params.clone();
                self.transcriptions
                    .start(name, page.bytes.clone(), params, ui.ctx());
            }
        } else {
            if !self.transcriptions.in_flight(name) {
                let params = self.clean_params.clone();
                self.transcriptions
                    .start(name, page.bytes.clone(), params, ui.ctx());
            }
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Transcribing {name}…"));
            });
        }
    }

    /// The detected page outline over the original, and what cleaning found, for tuning cleanup.
    fn debug_overlay(&mut self, ui: &mut egui::Ui, name: &str, image_rect: Rect) {
        let Some(report) = self.report(name).cloned() else {
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

    /// What cleaning found for the page named `name`, cleaning it if that hasn't happened yet.
    fn report(&mut self, name: &str) -> Option<&CleanReport> {
        if !self.reports.contains_key(name) {
            let page = self.pages.iter().find(|p| p.name == name)?;
            let img = decode(&page.bytes).ok()?;
            let cleaned = timed(format_args!("{name}: clean"), || {
                clean(&img, &self.clean_params)
            });
            self.reports.insert(name.to_owned(), cleaned.report);
        }
        self.reports.get(name)
    }

    /// Loading progress and errors from the last drop, and what the last export did.
    fn status(&self, ui: &mut egui::Ui) {
        if self.ingest.pending() > 0 {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Loading {}…", self.ingest.pending()));
            });
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

    /// The selected page at full size in the current view, decoding (and cleaning) it if the
    /// selection or view changed.
    fn current_texture(&mut self, ctx: &egui::Context) -> Option<TextureHandle> {
        let index = self.selected_index();
        let page = self.pages.get(index)?;
        let cleaned = self.view.shows_cleaned();
        if self
            .current
            .as_ref()
            .is_none_or(|shown| shown.name != page.name || shown.cleaned != cleaned)
        {
            let name = &page.name;
            let texture = match timed(format_args!("{name}: decode"), || decode(&page.bytes)) {
                Ok(img) => {
                    let img = if cleaned {
                        let cleaned = timed(format_args!("{name}: clean"), || {
                            clean(&img, &self.clean_params)
                        });
                        self.reports.insert(name.clone(), cleaned.report);
                        cleaned.image
                    } else {
                        img
                    };
                    let max_side = ctx.input(|i| i.max_texture_side) as u32;
                    log::debug!(
                        "{name}: {:?}, max texture side {max_side}",
                        img.dimensions()
                    );
                    let img = timed(format_args!("{name}: shrink"), || {
                        shrink_to_fit(img, max_side)
                    });
                    let color = timed(format_args!("{name}: color image"), || color_image(&img));
                    timed(format_args!("{name}: load texture"), || {
                        ctx.load_texture(name, color, TextureOptions::LINEAR)
                    })
                }
                // Already decoded once when the page was added, so this shouldn't happen.
                Err(err) => {
                    log::error!("Couldn't decode {}: {err}", page.name);
                    self.thumbs[index].clone()
                }
            };
            self.current = Some(Shown {
                name: page.name.clone(),
                cleaned,
                texture,
            });
        }
        self.current.as_ref().map(|shown| shown.texture.clone())
    }
}

fn format_quad(quad: &[[f32; 2]; 4]) -> String {
    let corners: Vec<String> = quad
        .iter()
        .map(|[x, y]| format!("({x:.0}, {y:.0})"))
        .collect();
    corners.join(" ")
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
