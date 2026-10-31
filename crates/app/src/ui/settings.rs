//! The Settings dialog: which model transcribes, with what each is good for, and how Claude's
//! markup is written. It edits a copy of the settings, which Save hands back.
//!
//! See `decisions/2026-10-30-settings-dialog.md`.

use eframe::egui;
use egui::{RichText, Sense, Stroke, TextStyle, Ui, Vec2};
use notebook_core::llm_model::Model;
use notebook_core::markdown_style::MarkdownStyle;
use notebook_core::settings::Settings;

use crate::t;
use crate::ui::dialog::{self, Choice, Stakes};

/// Marks that can wrap underlined text.
const UNDERLINES: [&str; 2] = ["_", "*"];

/// Heading levels a wholly underlined line can become.
const HEADINGS: std::ops::RangeInclusive<usize> = 1..=6;

fn label(model: Model) -> String {
    match model {
        Model::Opus => t!("model-opus"),
        Model::Sonnet => t!("model-sonnet"),
    }
}

/// What each model is good for.
fn tradeoffs(model: Model) -> String {
    match model {
        Model::Opus => t!("model-opus-tradeoffs"),
        Model::Sonnet => t!("model-sonnet-tradeoffs"),
    }
}

/// How underlined text wrapped in `marker` looks.
fn underline_example(marker: &str) -> String {
    match marker {
        "_" => t!("underline-underscores"),
        "*" => t!("underline-asterisks"),
        other => other.to_owned(),
    }
}

#[derive(Default)]
pub struct SettingsDialog {
    /// The settings being edited, while the dialog is open.
    draft: Option<Settings>,
}

impl SettingsDialog {
    /// Opens the dialog on `current`.
    pub fn open(&mut self, current: &Settings) {
        self.draft = Some(current.clone());
    }

    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    /// The dialog, if it's open. Returns the edited settings when the user saves them.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Settings> {
        let draft = self.draft.as_mut()?;
        let choice = dialog::show(ctx, "settings", 420.0, |ui| {
            ui.heading(t!("settings-title"));
            ui.add_space(8.0);
            model(ui, &mut draft.model);
            ui.add_space(12.0);
            markdown(ui, &mut draft.markdown);
            dialog::buttons(ui, &t!("settings-save"), Stakes::Safe, true)
        });
        match choice? {
            Choice::Affirmative => self.draft.take(),
            Choice::Cancel => {
                self.draft = None;
                None
            }
        }
    }
}

fn model(ui: &mut Ui, model: &mut Model) {
    ui.label(RichText::new(t!("settings-model")).strong());
    for m in Model::ALL {
        ui.radio_value(model, m, label(m));
        ui.indent(m.name(), |ui| {
            ui.label(RichText::new(tradeoffs(m)).small().weak());
        });
    }
    ui.add_space(4.0);
    ui.label(RichText::new(t!("settings-model-hint")).small().weak());
}

fn markdown(ui: &mut Ui, style: &mut MarkdownStyle) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(t!("settings-markdown")).strong());
        ui.spacing_mut().item_spacing.x = 4.0;
        let hint = t!("settings-future-only-hint");
        info_icon(ui).on_hover_text(&hint);
        ui.label(RichText::new(t!("settings-future-only")).small().weak())
            .on_hover_text(hint);
    });
    egui::Grid::new("markdown style")
        .num_columns(2)
        .show(ui, |ui| {
            let label = ui.label(t!("settings-underlines"));
            egui::ComboBox::from_id_salt("underline")
                .selected_text(underline_example(&style.underline))
                .show_ui(ui, |ui| {
                    for marker in UNDERLINES {
                        let example = underline_example(marker);
                        ui.selectable_value(&mut style.underline, marker.to_owned(), example);
                    }
                })
                .response
                .labelled_by(label.id);
            ui.end_row();
            let label = ui.label(t!("settings-underlined-lines"));
            egui::ComboBox::from_id_salt("underlined line heading")
                .selected_text(heading(style.underlined_line_heading))
                .show_ui(ui, |ui| {
                    for level in HEADINGS {
                        ui.selectable_value(
                            &mut style.underlined_line_heading,
                            level,
                            heading(level),
                        );
                    }
                })
                .response
                .labelled_by(label.id);
            ui.end_row();
        });
}

/// How a heading of `level` is written.
fn heading(level: usize) -> String {
    t!("settings-heading", marks = "#".repeat(level))
}

/// A circled "i", drawn because egui's bundled fonts have no glyph for one.
fn info_icon(ui: &mut Ui) -> egui::Response {
    let size = ui.text_style_height(&TextStyle::Small);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let color = ui.visuals().weak_text_color();
    let painter = ui.painter();
    painter.circle_stroke(rect.center(), size / 2.0 - 0.5, Stroke::new(1.0, color));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "i",
        egui::FontId::proportional(size * 0.75),
        color,
    );
    response
}
