//! The Settings dialog: which model transcribes, with what each is good for, how Claude's markup
//! is written, and whether transcripts are cached. It edits a copy of the settings, which Save
//! hands back. Clearing the cache isn't a setting: it happens once the user confirms it.
//!
//! See `decisions/2026-10-30-settings-dialog.md` and
//! `decisions/2026-11-01-manual-cache-clearing.md`.

use eframe::egui;
use egui::{RichText, Sense, Stroke, TextStyle, Ui, Vec2};
use notebook_core::cache::Cache;
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

/// What the dialog did, for the app to act on.
pub enum Outcome {
    /// The user saved these settings.
    Saved(Settings),
    /// The cache couldn't be cleared, for this reason.
    ClearFailed(String),
}

/// Which question about clearing the cache is being asked.
#[derive(Clone, Copy)]
enum Confirm {
    /// Over the dialog, after Clear Cache.
    Clear,
    /// After saving with caching switched off, while the cache still has entries.
    SwitchedOff,
}

#[derive(Default)]
pub struct SettingsDialog {
    /// The settings being edited, while the dialog is open.
    draft: Option<Settings>,
    /// Whether the settings cached transcripts when the dialog opened.
    was_caching: bool,
    /// The system cache, if there is one.
    cache: Option<Cache>,
    /// How many entries the cache has, as of opening the dialog or clearing it.
    cached: usize,
    confirm: Option<Confirm>,
}

impl SettingsDialog {
    /// Opens the dialog on `current`, with `cache` to show and clear.
    pub fn open(&mut self, current: &Settings, cache: Option<Cache>) {
        self.draft = Some(current.clone());
        self.was_caching = current.cache_transcripts;
        self.cache = cache;
        self.cached = self.count();
        self.confirm = None;
    }

    /// Whether the dialog, or a question it asks, is showing.
    pub fn is_open(&self) -> bool {
        self.draft.is_some() || self.confirm.is_some()
    }

    /// The dialog and the question it's asking, if any. Returns what the user did that the app
    /// must act on.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let saved = self.show_settings(ctx).map(Outcome::Saved);
        let cleared = self.show_confirm(ctx);
        saved.or(cleared)
    }

    /// The settings, if they're being edited. Returns them when the user saves them.
    fn show_settings(&mut self, ctx: &egui::Context) -> Option<Settings> {
        let draft = self.draft.as_mut()?;
        // A question over the dialog has Enter, and the dialog's buttons wait for it.
        let asking = self.confirm.is_some();
        let (cache, cached) = (self.cache.as_ref(), self.cached);
        let mut clear = false;
        let choice = dialog::show(ctx, "settings", 420.0, |ui| {
            ui.heading(t!("settings-title"));
            ui.add_space(8.0);
            model(ui, &mut draft.model);
            ui.add_space(12.0);
            markdown(ui, &mut draft.markdown);
            ui.add_space(12.0);
            clear = caching(ui, &mut draft.cache_transcripts, cache, cached);
            dialog::buttons(ui, &t!("settings-save"), Stakes::Safe, !asking)
        });
        if clear {
            self.confirm = Some(Confirm::Clear);
        }
        match choice? {
            Choice::Affirmative => {
                let saved = self.draft.take()?;
                if self.was_caching && !saved.cache_transcripts {
                    self.cached = self.count();
                    if self.cached > 0 {
                        self.confirm = Some(Confirm::SwitchedOff);
                    }
                }
                Some(saved)
            }
            Choice::Cancel | Choice::Discard => {
                self.draft = None;
                self.confirm = None;
                None
            }
        }
    }

    /// The question about clearing the cache, if one's being asked. Clears it if the user says to,
    /// returning why that failed if it did.
    fn show_confirm(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let confirm = self.confirm?;
        let count = self.cached;
        let (question, button, keep) = match confirm {
            Confirm::Clear => (
                t!("confirm-clear-cache", count = count),
                t!("settings-cache-clear"),
                t!("dialog-cancel"),
            ),
            Confirm::SwitchedOff => (
                t!("confirm-cache-off", count = count),
                t!("confirm-cache-off-button"),
                t!("confirm-cache-off-keep"),
            ),
        };
        let choice = dialog::show(ctx, "clear cache", 360.0, |ui| {
            ui.label(question);
            dialog::buttons_saying(ui, &button, &keep, Stakes::Dangerous, true)
        })?;
        self.confirm = None;
        if choice != Choice::Affirmative {
            return None;
        }
        let cleared = self.cache.as_ref()?.clear();
        self.cached = self.count();
        match cleared {
            Ok(count) => {
                log::info!("Cleared {count} cached transcriptions");
                None
            }
            Err(err) => {
                log::warn!("couldn't clear the cache: {err}");
                let error = t!("settings-cache-clear-failed", error = err.to_string());
                Some(Outcome::ClearFailed(error))
            }
        }
    }

    /// How many entries the cache has; none if it can't be read.
    fn count(&self) -> usize {
        let Some(cache) = &self.cache else {
            return 0;
        };
        cache.count().unwrap_or_else(|err| {
            log::warn!("couldn't count {}: {err}", cache.dir().display());
            0
        })
    }
}

/// Where the cache is, whether to use it, and a button to clear its `cached` entries, which
/// returns whether it was clicked. Without a cache, only the switch.
fn caching(ui: &mut Ui, enabled: &mut bool, cache: Option<&Cache>, cached: usize) -> bool {
    ui.label(RichText::new(t!("settings-caching")).strong());
    if let Some(cache) = cache {
        ui.horizontal(|ui| {
            let label = ui.label(t!("settings-cache-location"));
            // Read-only, but selectable and copyable, and in the Tab order.
            let dir = cache.dir().display().to_string();
            let mut dir = dir.as_str();
            ui.add(egui::TextEdit::singleline(&mut dir).desired_width(f32::INFINITY))
                .labelled_by(label.id);
        });
    }
    ui.checkbox(enabled, t!("settings-cache-enable"));
    ui.label(
        RichText::new(t!("settings-cache-explanation"))
            .small()
            .weak(),
    );
    if cache.is_none() {
        return false;
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // Red, as it deletes what's cached; it asks first.
        let clear = dialog::button(ui, &t!("settings-cache-clear"), Stakes::Dangerous);
        let clicked = ui.add_enabled(cached > 0, clear).clicked();
        ui.label(RichText::new(t!("settings-cache-count", count = cached)).weak());
        clicked
    })
    .inner
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
