//! The Transcribed view: the cleaned page beside its transcript, where the user corrects it.
//!
//! Uncertain words are marked on both sides; clicking one, or its box on the page, offers Claude's
//! other readings or one of the user's own. Double-clicking a line edits it.
//!
//! See `decisions/2026-10-23-transcript-corrections.md`.

use std::ops::Range;

use eframe::egui;
use egui::accesskit::Role;
use egui::{
    Color32, CursorIcon, Id, Key, Label, Popup, PopupCloseBehavior, Pos2, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, TextEdit, TextStyle, UiBuilder, Vec2, WidgetInfo,
};
use notebook_core::Crop;
use notebook_core::reframe::Reframe;
use notebook_core::transcript::{Span, Transcript};

use crate::eval_log::{Event, Via};

/// Room for corrections to lengthen lines, as a multiple of the longest line.
const EDIT_ROOM: f32 = 1.2;

/// Widest `transcript` needs to be not to wrap, with room for corrections and the scroll bar.
pub fn natural_width(ui: &egui::Ui, transcript: &Transcript) -> f32 {
    let font = TextStyle::Monospace.resolve(ui.style());
    let widest = ui.ctx().fonts_mut(|fonts| {
        transcript
            .lines
            .iter()
            .map(|line| {
                let galley =
                    fonts.layout_no_wrap(line.text_md.clone(), font.clone(), Color32::PLACEHOLDER);
                galley.size().x
            })
            .fold(0.0, f32::max)
    });
    widest * EDIT_ROOM + ui.spacing().scroll.allocated_width()
}

/// What the user is doing in the view. Starts afresh when another page is shown.
#[derive(Default)]
pub struct TranscriptView {
    /// The page this state belongs to.
    page: String,
    /// The uncertain word under the pointer last frame, highlighted on both sides.
    hovered: Option<usize>,
    correcting: Option<Correcting>,
    /// The line being edited, and its text so far.
    editing_line: Option<(usize, String)>,
    /// Corrections made this frame, for the [`crate::eval_log::EvalLog`].
    events: Vec<Event>,
}

/// The popup for correcting an uncertain word.
struct Correcting {
    index: usize,
    /// Opened from the word's box on the page, rather than from the text.
    from_box: bool,
    anchor: Rect,
    /// The user's own reading so far.
    custom: String,
}

/// How an uncertain word is marked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Open,
    /// Corrected or kept by the user.
    Resolved,
    /// Not found in the text, so it can't be corrected.
    Unfound,
}

impl TranscriptView {
    /// Shows `transcript`'s uncertain words over the page drawn at `image`'s rect, which was cut
    /// from the photo at its [`Crop`], and the transcript itself in `text_rect`, applying the
    /// user's corrections to it. Returns what the user corrected.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        page: &str,
        transcript: &mut Transcript,
        image: Option<(Rect, Crop)>,
        text_rect: Rect,
    ) -> Vec<Event> {
        if self.page != page {
            *self = Self {
                page: page.to_owned(),
                ..Self::default()
            };
        }
        let open = transcript.open_uncertain();
        let spans = transcript.uncertain_spans();
        let marks: Vec<Mark> = transcript
            .uncertain
            .iter()
            .zip(&spans)
            .map(|(u, span)| match (span, u.resolved) {
                (None, _) => Mark::Unfound,
                (Some(_), true) => Mark::Resolved,
                (Some(_), false) => Mark::Open,
            })
            .collect();

        let mut hovered = None;
        if let Some((image_rect, crop)) = image {
            self.boxes(ui, transcript, &marks, image_rect, crop, &mut hovered);
        }
        ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    self.lines(ui, transcript, &spans, &marks, &mut hovered);
                });
        });
        self.popup(ui, transcript);
        self.hovered = hovered;

        // By correcting the last open word, or editing it out of its line.
        if open > 0 && transcript.open_uncertain() == 0 {
            let spans = transcript.uncertain_spans();
            self.events.push(Event::AllResolved {
                page: page.to_owned(),
                model: transcript.model.clone(),
                uncertain: spans.len(),
                unfound: spans.iter().filter(|s| s.is_none()).count(),
            });
        }
        std::mem::take(&mut self.events)
    }

    /// Whether uncertain word `index` is highlighted: under the pointer, or being corrected.
    fn is_hot(&self, index: usize) -> bool {
        self.hovered == Some(index) || self.correcting.as_ref().is_some_and(|c| c.index == index)
    }

    fn open_correction(&mut self, index: usize, from_box: bool, anchor: Rect) {
        self.editing_line = None;
        self.correcting = Some(Correcting {
            index,
            from_box,
            anchor,
            custom: String::new(),
        });
    }

    /// Keeps the popup next to the word it opened from, if it moved.
    fn follow(&mut self, index: usize, from_box: bool, rect: Rect) {
        if let Some(c) = &mut self.correcting
            && c.index == index
            && c.from_box == from_box
        {
            c.anchor = rect;
        }
    }

    /// The uncertain words' boxes over the page drawn at `image_rect`, cut from the photo at
    /// `crop`. Boxes follow the page if it was cut elsewhere when transcribed, slanting if it's
    /// turned. See `decisions/2026-10-30-boxes-follow-corners.md`.
    fn boxes(
        &mut self,
        ui: &mut egui::Ui,
        transcript: &Transcript,
        marks: &[Mark],
        image_rect: Rect,
        crop: Crop,
        hovered: &mut Option<usize>,
    ) {
        let reframe = Reframe::new(transcript.crop, crop);
        let (w, h) = crop.size;
        let scale = image_rect.size() / Vec2::new(w as f32, h as f32);
        let on_screen = |[x, y]: [f32; 2]| image_rect.min + Vec2::new(x, y) * scale;
        let painter = ui.painter_at(image_rect);
        for (i, (u, &mark)) in transcript.uncertain.iter().zip(marks).enumerate() {
            let Some(bbox) = u.bbox else {
                continue;
            };
            let quad: Vec<Pos2> = reframe.quad(bbox).into_iter().map(on_screen).collect();
            let rect = Rect::from_points(&quad).intersect(image_rect);
            let correctable = mark != Mark::Unfound;
            let sense = if correctable {
                Sense::click()
            } else {
                Sense::hover()
            };
            let response = ui.interact(rect, ui.scope_id().with(("uncertain box", i)), sense);
            response.widget_info(|| {
                WidgetInfo::labeled(Role::Button, correctable, format!("Box: {}", u.text))
            });
            let hot = self.is_hot(i);
            let (fill, stroke) = (highlight(ui, mark, hot), outline(ui, mark, hot));
            if reframe.is_identity() {
                painter.rect_filled(rect, 2.0, fill);
                painter.rect_stroke(rect, 2.0, stroke, StrokeKind::Outside);
            } else {
                painter.add(Shape::convex_polygon(quad, fill, stroke));
            }

            let response = if correctable {
                response.on_hover_cursor(CursorIcon::PointingHand)
            } else {
                let mut readings = vec![u.text.as_str()];
                readings.extend(u.alternatives.iter().map(String::as_str));
                response.on_hover_text(format!("Not found in the text: {}", readings.join(" / ")))
            };
            if response.hovered() {
                *hovered = Some(i);
            }
            if response.clicked() {
                self.open_correction(i, true, rect);
            }
            self.follow(i, true, rect);
        }
    }

    /// The transcript, one row per line, with uncertain words marked.
    fn lines(
        &mut self,
        ui: &mut egui::Ui,
        transcript: &mut Transcript,
        spans: &[Option<Span>],
        marks: &[Mark],
        hovered: &mut Option<usize>,
    ) {
        let mut words: Vec<Vec<(usize, Range<usize>)>> = vec![Vec::new(); transcript.lines.len()];
        for (i, span) in spans.iter().enumerate() {
            if let Some(Span { line, range }) = span {
                words[*line].push((i, range.clone()));
            }
        }

        for (line, words) in words.iter_mut().enumerate() {
            if let Some((editing, text)) = &mut self.editing_line
                && *editing == line
            {
                let response = ui.add(
                    TextEdit::singleline(text)
                        // Also names the field for screen readers.
                        .hint_text("Empty line")
                        .font(TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                );
                if response.lost_focus() {
                    // Enter or a click elsewhere keeps the edit; Escape drops it.
                    let from = &transcript.lines[line].text_md;
                    if !ui.input(|i| i.key_pressed(Key::Escape)) && *from != *text {
                        self.events.push(Event::LineEdited {
                            page: self.page.clone(),
                            model: transcript.model.clone(),
                            line,
                            from: from.clone(),
                            to: text.clone(),
                        });
                        transcript.set_line(line, std::mem::take(text));
                    }
                    self.editing_line = None;
                } else if !response.has_focus() {
                    response.request_focus();
                }
                continue;
            }

            // Two uncertain words can overlap; only the first is marked.
            words.sort_by_key(|(_, range)| range.start);
            let text = transcript.lines[line].text_md.as_str();
            let mut double_clicked = false;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut plain = |ui: &mut egui::Ui, text: &str| {
                    let label = Label::new(RichText::new(text).monospace()).sense(Sense::click());
                    double_clicked |= ui.add(label).double_clicked();
                };
                let mut at = 0;
                for (i, range) in words.iter() {
                    if range.start < at {
                        continue;
                    }
                    if range.start > at {
                        plain(ui, &text[at..range.start]);
                    }
                    let mark = marks[*i];
                    let mut word = RichText::new(&text[range.clone()])
                        .monospace()
                        .background_color(highlight(ui, mark, self.is_hot(*i)));
                    if mark == Mark::Resolved {
                        word = word.underline();
                    }
                    let response = ui
                        .add(Label::new(word).sense(Sense::click()))
                        .on_hover_cursor(CursorIcon::PointingHand);
                    if response.hovered() {
                        *hovered = Some(*i);
                    }
                    if response.clicked() {
                        self.open_correction(*i, false, response.rect);
                    }
                    self.follow(*i, false, response.rect);
                    at = range.end;
                }
                // A paragraph break still needs something to double-click.
                if at < text.len() || text.is_empty() {
                    plain(ui, if text.is_empty() { " " } else { &text[at..] });
                }
            });
            if double_clicked {
                self.correcting = None;
                self.editing_line = Some((line, text.to_owned()));
            }
        }
    }

    /// Claude's other readings of the word being corrected, plus Keep and a reading of the
    /// user's own.
    fn popup(&mut self, ui: &mut egui::Ui, transcript: &mut Transcript) {
        let Some(c) = &mut self.correcting else {
            return;
        };
        let Some(u) = transcript.uncertain.get(c.index) else {
            self.correcting = None;
            return;
        };
        let mut open = true;
        let mut choice = None;
        Popup::new(
            Id::unique("correct uncertain word"),
            ui.ctx().clone(),
            c.anchor,
            ui.layer_id(),
        )
        .open_bool(&mut open)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            for (i, alternative) in u.alternatives.iter().enumerate() {
                if ui.button(alternative).clicked() {
                    choice = Some((alternative.clone(), Via::Alternative(i)));
                }
            }
            if ui.button(format!("Keep \"{}\"", u.text)).clicked() {
                choice = Some((u.text.clone(), Via::Keep));
            }
            ui.horizontal(|ui| {
                let edit = ui.add(
                    TextEdit::singleline(&mut c.custom)
                        .hint_text("Something else")
                        .desired_width(160.0),
                );
                let custom = c.custom.trim();
                let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let ok = ui.add_enabled(!custom.is_empty(), egui::Button::new("OK"));
                if !custom.is_empty() && (ok.clicked() || entered) {
                    choice = Some((custom.to_owned(), Via::Custom));
                }
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            open = false;
        }
        if let Some((choice, via)) = choice {
            let from = u.text.clone();
            if transcript.resolve(c.index, &choice) {
                self.events.push(Event::Corrected {
                    page: self.page.clone(),
                    model: transcript.model.clone(),
                    word: c.index,
                    from,
                    to: choice,
                    via,
                });
            }
            open = false;
        }
        if !open {
            self.correcting = None;
        }
    }
}

/// The background behind an uncertain word, in the text and in its box.
fn highlight(ui: &egui::Ui, mark: Mark, hot: bool) -> Color32 {
    let visuals = ui.visuals();
    match (mark, hot) {
        (Mark::Open, false) => visuals.warn_fg_color.gamma_multiply(0.2),
        (Mark::Open, true) => visuals.warn_fg_color.gamma_multiply(0.45),
        (Mark::Resolved, true) => visuals.weak_text_color().gamma_multiply(0.3),
        (Mark::Resolved, false) | (Mark::Unfound, _) => Color32::TRANSPARENT,
    }
}

/// The outline of an uncertain word's box.
fn outline(ui: &egui::Ui, mark: Mark, hot: bool) -> Stroke {
    let visuals = ui.visuals();
    match mark {
        Mark::Open => Stroke::new(if hot { 2.5 } else { 1.5 }, visuals.warn_fg_color),
        Mark::Resolved => Stroke::new(1.0, visuals.weak_text_color()),
        Mark::Unfound => Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.5)),
    }
}
