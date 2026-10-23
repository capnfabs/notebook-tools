use eframe::egui;
use egui::{Align, Layout, Sense, Shape, StrokeKind, TextureHandle, TextureOptions, Vec2};
use notebook_core::{Page, Pages, RgbaImage, decode, shrink_to_fit};

use crate::ingest::Ingest;
use crate::timing::timed;

/// Display size of the thumbnails in the bottom strip, in egui points (physical pixels =
/// points x `pixels_per_point`). Keep at most `THUMBNAIL_SIDE_PX` / 2 so 2x displays stay sharp.
const THUMB_SIZE_PT: f32 = 96.0;

#[derive(Default)]
pub struct NotebookApp {
    ingest: Ingest,
    pages: Pages,
    /// One per page, in the same order.
    thumbs: Vec<TextureHandle>,
    /// Name of the page the user picked. The first page is shown until they pick one.
    selected: Option<String>,
    /// Full-size texture for the selected page, keyed by page name.
    current: Option<(String, TextureHandle)>,
    errors: Vec<String>,
}

impl NotebookApp {
    /// The app's UI, separate from [`eframe::App`] so tests can drive it directly.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.ingest.start_reading_drops(&ctx) > 0 {
            self.errors.clear();
        }
        self.add_next_page(&ctx);

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
                // Only the original view exists so far.
                let _ = ui.selectable_label(true, "Original");
                ui.add_enabled(false, egui::Button::selectable(false, "Cleaned"));
                ui.add_enabled(false, egui::Button::selectable(false, "Transcribed"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled(false, egui::Button::new("Delete"));
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
                    ui.add_enabled(false, egui::Button::new("Export"));
                    ui.add_enabled(false, egui::Button::new("Re-export"));
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let name = self
                .pages
                .get(self.selected_index())
                .map(|p| p.name.clone());
            if let (Some(name), Some(texture)) = (name, self.current_texture(ui.ctx())) {
                ui.centered_and_justified(|ui| {
                    ui.add(
                        egui::Image::new(&texture)
                            .shrink_to_fit()
                            .alt_text(format!("Current: {name}")),
                    );
                });
            }
        });

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

    /// Loading progress and errors from the last drop.
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
    }

    /// The selected page at full size, decoding it if the selection changed.
    fn current_texture(&mut self, ctx: &egui::Context) -> Option<TextureHandle> {
        let index = self.selected_index();
        let page = self.pages.get(index)?;
        if self
            .current
            .as_ref()
            .is_none_or(|(name, _)| *name != page.name)
        {
            let name = &page.name;
            let texture = match timed(format_args!("{name}: decode"), || decode(&page.bytes)) {
                Ok(img) => {
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
            self.current = Some((page.name.clone(), texture));
        }
        self.current.as_ref().map(|(_, texture)| texture.clone())
    }
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
