//! The Settings dialog: which model transcribes, with what each is good for, and how Claude's
//! markup is written. It edits a copy of the settings, which Save hands back.
//!
//! See `decisions/2026-10-30-settings-dialog.md`.

use eframe::egui;
use egui::{RichText, Sense, Stroke, TextStyle, Ui, Vec2};
use notebook_core::llm_model::Model;
use notebook_core::markdown_style::MarkdownStyle;
use notebook_core::settings::Settings;

use crate::ui::dialog::{self, Action, Choice};

/// Under the model choice.
const MODEL_HINT: &str = "For new and forced transcriptions. Cached transcripts by either model \
are used; this one's is preferred.";

/// Beside the markdown style, which only reaches transcripts parsed from now on and unedited ones.
const FUTURE_ONLY: &str = "Future transcripts only";

const FUTURE_ONLY_HINT: &str = "Applies to future and unedited transcripts only, so we don't \
clobber your edits. \"Reset Transcript\" on edited pages to pick up changes.";

/// Marks that can wrap underlined text, and how they look.
const UNDERLINES: [(&str, &str); 2] = [("_", "_underscores_"), ("*", "*asterisks*")];

/// Heading levels a wholly underlined line can become.
const HEADINGS: std::ops::RangeInclusive<usize> = 1..=6;

/// What each model is good for. From `decisions/2026-10-24-sonnet-quality.md`.
fn tradeoffs(model: Model) -> &'static str {
    match model {
        Model::Opus => {
            "Slower (20–45s a page), about $0.07 a page. Flags more of the words it's unsure \
            of. Best for pages you'll correct closely."
        }
        Model::Sonnet => {
            "2–3× faster, about $0.03 a page, and reads words about as well. Flags fewer of its \
            mistakes, so more are left to find by reading, and may drop separators, underlines \
            and crossed-out words. Good for a quick first pass."
        }
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
            ui.heading("Settings");
            ui.add_space(8.0);
            model(ui, &mut draft.model);
            ui.add_space(12.0);
            markdown(ui, &mut draft.markdown);
            dialog::buttons(ui, "Save", Action::Default, true)
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
    ui.label(RichText::new("Transcription model").strong());
    for m in Model::ALL {
        ui.radio_value(model, m, m.label());
        ui.indent(m.name(), |ui| {
            ui.label(RichText::new(tradeoffs(m)).small().weak());
        });
    }
    ui.add_space(4.0);
    ui.label(RichText::new(MODEL_HINT).small().weak());
}

fn markdown(ui: &mut Ui, style: &mut MarkdownStyle) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Markdown").strong());
        ui.spacing_mut().item_spacing.x = 4.0;
        info_icon(ui).on_hover_text(FUTURE_ONLY_HINT);
        ui.label(RichText::new(FUTURE_ONLY).small().weak())
            .on_hover_text(FUTURE_ONLY_HINT);
    });
    egui::Grid::new("markdown style")
        .num_columns(2)
        .show(ui, |ui| {
            let label = ui.label("Underlines");
            let selected = UNDERLINES
                .iter()
                .find(|(marker, _)| *marker == style.underline)
                .map_or(style.underline.as_str(), |(_, label)| label);
            egui::ComboBox::from_id_salt("underline")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (marker, label) in UNDERLINES {
                        ui.selectable_value(&mut style.underline, marker.to_owned(), label);
                    }
                })
                .response
                .labelled_by(label.id);
            ui.end_row();
            let label = ui.label("Underlined lines");
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
    format!("{} Heading", "#".repeat(level))
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
