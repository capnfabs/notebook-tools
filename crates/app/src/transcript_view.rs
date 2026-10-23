//! The Transcribed view: the cleaned page beside its transcript, where the user corrects it.
//!
//! Uncertain words are marked on both sides; clicking one, or its box on the page, offers Claude's
//! other readings or one of the user's own. Double-clicking a line edits it.

use std::ops::Range;

use eframe::egui;
use egui::accesskit::Role;
use egui::{
    Color32, CursorIcon, Id, Key, Label, Popup, PopupCloseBehavior, Rect, RichText, Sense, Stroke,
    StrokeKind, TextEdit, TextStyle, UiBuilder, Vec2, WidgetInfo,
};
use notebook_core::ocr::{Span, Transcript};

/// Narrower than this, the transcript goes under the page instead of beside it.
const SIDE_BY_SIDE_MIN_WIDTH: f32 = 600.0;

/// Where the page and the transcript go in `rect`: side by side, or the page above the
/// transcript when `rect` is narrow.
pub fn split(rect: Rect, gap: f32) -> (Rect, Rect) {
    if rect.width() >= SIDE_BY_SIDE_MIN_WIDTH {
        let (page, text) = rect.split_left_right_at_fraction(0.5);
        (
            page.shrink2(Vec2::new(gap / 2.0, 0.0)),
            text.shrink2(Vec2::new(gap / 2.0, 0.0)),
        )
    } else {
        let (page, text) = rect.split_top_bottom_at_fraction(0.5);
        (
            page.shrink2(Vec2::new(0.0, gap / 2.0)),
            text.shrink2(Vec2::new(0.0, gap / 2.0)),
        )
    }
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
    /// Shows `transcript`'s uncertain words over the page drawn at `image_rect`, and the
    /// transcript itself in `text_rect`, applying the user's corrections to it.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        page: &str,
        transcript: &mut Transcript,
        image_rect: Option<Rect>,
        text_rect: Rect,
    ) {
        if self.page != page {
            *self = Self {
                page: page.to_owned(),
                ..Self::default()
            };
        }
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
        if let Some(image_rect) = image_rect {
            self.boxes(ui, transcript, &marks, image_rect, &mut hovered);
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

    /// The uncertain words' boxes over the page drawn at `image_rect`.
    fn boxes(
        &mut self,
        ui: &mut egui::Ui,
        transcript: &Transcript,
        marks: &[Mark],
        image_rect: Rect,
        hovered: &mut Option<usize>,
    ) {
        let (w, h) = transcript.image_size;
        let scale = image_rect.size() / Vec2::new(w as f32, h as f32);
        let painter = ui.painter_at(image_rect);
        for (i, (u, &mark)) in transcript.uncertain.iter().zip(marks).enumerate() {
            let Some([x0, y0, x1, y1]) = u.bbox else {
                continue;
            };
            let rect = Rect::from_min_max(
                image_rect.min + Vec2::new(x0, y0) * scale,
                image_rect.min + Vec2::new(x1, y1) * scale,
            )
            .intersect(image_rect);
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
            painter.rect_filled(rect, 2.0, highlight(ui, mark, hot));
            painter.rect_stroke(rect, 2.0, outline(ui, mark, hot), StrokeKind::Outside);

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
                    if !ui.input(|i| i.key_pressed(Key::Escape)) {
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
            for alternative in &u.alternatives {
                if ui.button(alternative).clicked() {
                    choice = Some(alternative.clone());
                }
            }
            if ui.button(format!("Keep \"{}\"", u.text)).clicked() {
                choice = Some(u.text.clone());
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
                    choice = Some(custom.to_owned());
                }
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            open = false;
        }
        if let Some(choice) = choice {
            transcript.resolve(c.index, &choice);
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
