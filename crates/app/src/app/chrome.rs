//! Everything around the selected page: the drop zone, toolbar, page strip and status line.

use eframe::egui;
use egui::{Align, Layout, Shape};

use super::commands::with_shortcut;
use super::{NotebookApp, View};
use crate::command::Command;
use crate::ingest::{Dropping, Ingest};
use crate::t;
use crate::ui::thumbnails;

impl NotebookApp {
    /// Where to drop photos, before there are any pages.
    pub(super) fn show_drop_zone(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let rect = ui.max_rect().shrink(24.0);
            let hovering = Ingest::hovering(ui.ctx());
            let stroke = if hovering.is_some() {
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
                // Says what the files dragged over it would do once dropped.
                let title = hovering
                    .as_ref()
                    .map_or_else(|| t!("drop-zone-title"), drop_text);
                ui.heading(title);
                // The photos that can be added, unless it's a project that would be opened.
                if hovering.is_none_or(|d| d == Dropping::AddPages) {
                    ui.label(t!("drop-zone-formats"));
                }
                self.show_status(ui);
            });
        });
    }

    /// Everything but the drop zone: the toolbar, the page strip and status line, the selected
    /// page in the current view, and any dialog.
    pub(super) fn show_viewer(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(ui, |ui| self.show_toolbar(ui));
        egui::Panel::bottom("pages").show(ui, |ui| self.show_bottom_bar(ui));
        egui::CentralPanel::default().show(ui, |ui| match self.view {
            View::Original => self.show_original(ui),
            View::Cleaned => self.show_cleaned(ui),
            View::Transcribed => self.show_transcribed(ui),
        });

        self.delete_dialog(ui.ctx());
        self.transcribe_all_dialog(ui.ctx());
        self.retranscribe_dialog(ui.ctx());
        self.reset_dialog(ui.ctx());
        self.relink_dialogs(ui.ctx());
        self.export
            .dialog(ui.ctx(), &self.pages, self.selected_index());

        if let Some(dropping) = Ingest::hovering(ui.ctx()) {
            show_drop_overlay(ui, &dropping);
        }
    }

    /// The views to pick from, which page is shown, and buttons to delete or sort pages.
    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (view, command) in [
                (View::Original, Command::ViewOriginal),
                (View::Cleaned, Command::ViewCleaned),
                (View::Transcribed, Command::ViewTranscribed),
            ] {
                let button = egui::Button::selectable(self.view == view, command.label());
                let button = ui
                    .add_enabled(self.can(command), button)
                    .on_disabled_hover_text(t!("no-transcriber-hint"));
                if with_shortcut(button, command).clicked() {
                    self.set_view(view);
                }
            }
            ui.separator();
            let index = self.selected_index();
            if let Some(page) = self.pages.get(index) {
                ui.label(t!(
                    "page-position",
                    name = page.name.as_str(),
                    number = index + 1,
                    count = self.pages.len()
                ));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.command_button(ui, Command::Delete).clicked() {
                    self.request_delete();
                }
                if self.command_button(ui, Command::SortByName).clicked() {
                    self.sort_pages();
                }
            });
        });
    }

    /// The page strip, and under it the status line and buttons to export and transcribe.
    fn show_bottom_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let selected = self.selected_index();
        let thumbs = |id| self.textures.thumbnail(id);
        match self.thumbnails.show(ui, &self.pages, thumbs, selected) {
            Some(thumbnails::Action::Select(id)) => self.selected = Some(id),
            Some(thumbnails::Action::Move { from, to }) => self.move_page(from, to),
            None => {}
        }
        ui.horizontal(|ui| {
            self.show_status(ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can_transcribe = self.can(Command::Transcribe);
                let can_show = self.transcriptions.can_show();
                let mut export = self.command_button(ui, Command::ExportAs);
                if !can_show {
                    export = export.on_disabled_hover_text(t!("no-transcriber-hint"));
                }
                if export.clicked() {
                    self.export.open_dialog();
                }
                // Export is Export As… until there's been an export to repeat.
                let can_reexport = self.can(Command::Export) && self.export.can_reexport();
                let reexport = egui::Button::new(t!("command-reexport"));
                let reexport = ui.add_enabled(can_reexport, reexport);
                if with_shortcut(reexport, Command::Export).clicked() {
                    self.export
                        .reexport(ui.ctx(), &self.pages, self.selected_index());
                }
                let transcribe_all = self
                    .command_button(ui, Command::TranscribeAll)
                    .on_disabled_hover_text(if can_transcribe {
                        t!("all-transcribed-hint")
                    } else {
                        t!("no-transcriber-hint")
                    });
                if transcribe_all.clicked() {
                    self.request_transcribe_all();
                }
            });
        });
    }

    /// Loading and transcription progress, which photos are missing, errors from the last drop,
    /// and what the last export did.
    pub(super) fn show_status(&self, ui: &mut egui::Ui) {
        if self.ingest.pending() > 0 {
            spinner(ui, &t!("status-loading", count = self.ingest.pending()));
        }
        match self.transcriptions.pending() {
            0 => {}
            count => spinner(ui, &t!("status-transcribing", count = count)),
        }
        if let Some(count) = self.export.running() {
            spinner(ui, &t!("status-exporting", count = count));
        }
        if let Some(missing) = self.missing_summary() {
            ui.colored_label(ui.visuals().error_fg_color, missing);
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
}

impl NotebookApp {
    /// Which pages' photos are missing, if any are: as they are now, so it changes as they're
    /// relinked, deleted or found by a drop.
    fn missing_summary(&self) -> Option<String> {
        let missing: Vec<&str> = self
            .pages
            .iter()
            .filter(|p| p.missing_photo().is_some())
            .map(|p| p.name.as_str())
            .collect();
        (!missing.is_empty()).then(|| {
            t!(
                "photos-missing",
                count = missing.len(),
                names = missing.join(", ")
            )
        })
    }
}

pub(super) fn spinner(ui: &mut egui::Ui, label: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(label);
    });
}

/// Darkens the window while files are dragged over it, saying what dropping them would do. A
/// label rather than painted text, so it's in the accessibility tree.
fn show_drop_overlay(ui: &egui::Ui, dropping: &Dropping) {
    let rect = ui.ctx().content_rect();
    let id = egui::Id::unique("drop_overlay");
    // Not an `Area`, which is hidden on its first frame while it's measured.
    let builder = egui::UiBuilder::new()
        .layer_id(egui::LayerId::new(egui::Order::Foreground, id))
        .max_rect(rect)
        .layout(Layout::centered_and_justified(egui::Direction::TopDown));
    let mut overlay = egui::Ui::new(ui.ctx().clone(), id, builder);
    overlay
        .painter()
        .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(160));
    let text = egui::RichText::new(drop_text(dropping))
        .heading()
        .color(egui::Color32::WHITE);
    overlay.add(egui::Label::new(text).selectable(false));
}

/// What dropping files does, to show while they're dragged over the window. See
/// `decisions/2026-10-31-open-by-drop-and-argument.md`.
fn drop_text(dropping: &Dropping) -> String {
    match dropping {
        Dropping::AddPages => t!("drop-overlay"),
        Dropping::OpenProject { path, .. } => {
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            t!("drop-overlay-open", name = name.into_owned())
        }
        Dropping::SeveralProjects => t!("drop-overlay-one-project"),
    }
}
