//! The Transcribed view: the cleaned page beside its transcript, where the user corrects it.
//!
//! Uncertain words are marked on both sides; clicking one, or its box on the page, offers Claude's
//! other readings or one of the user's own. Struck-through text can be deleted. Double-clicking a
//! line edits it, which a single click hints at, and pointing at a line outlines its box.
//!
//! See `decisions/2026-10-23-transcript-corrections.md` and
//! `decisions/2026-10-30-correction-ui.md`.

use std::ops::Range;

use eframe::egui;
use egui::accesskit::Role;
use egui::{
    Align, Color32, CursorIcon, Id, Key, Label, Popup, PopupCloseBehavior, Pos2, Rect, RichText,
    Sense, Shape, Stroke, StrokeKind, TextEdit, TextStyle, Tooltip, UiBuilder, Vec2, WidgetInfo,
};
use notebook_core::Crop;
use notebook_core::reframe::Reframe;
use notebook_core::transcript::{Bbox, Span, Transcript, Unfound, struck_ranges};

use crate::eval_log::{Event, Via};

/// Room for corrections to lengthen lines, as a multiple of the longest line.
const EDIT_ROOM: f32 = 1.2;

/// Shown after a single click on a line.
pub const EDIT_HINT: &str = "Double-click to edit";
/// How long the hint stays, in seconds.
const HINT_SECONDS: f64 = 3.0;
/// How far the pointer can move before the hint goes.
const HINT_SLOP: f32 = 6.0;

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
    /// What's under the pointer last frame, highlighted on both sides.
    hovered: Option<Target>,
    /// The line under the pointer or being edited last frame, outlined on the page.
    hot_line: Option<usize>,
    /// An uncertain word whose box was just clicked, for the transcript to scroll to.
    scroll_to: Option<usize>,
    /// Where a line was clicked once, and when: once it's not a double-click, says how to edit.
    hint: Option<(Pos2, f64)>,
    correcting: Option<Correcting>,
    /// The line being edited, and its text so far.
    editing_line: Option<(usize, String)>,
    /// Corrections made this frame, for the [`crate::eval_log::EvalLog`].
    events: Vec<Event>,
}

/// Something in the transcript the user can click to correct.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Target {
    /// An uncertain word, by index.
    Uncertain(usize),
    /// Struck-through text that isn't part of an uncertain word: `range` of line `line`.
    Struck { line: usize, range: Range<usize> },
}

/// The popup for correcting a [`Target`].
struct Correcting {
    target: Target,
    /// Opened from the word's box on the page, rather than from the text.
    from_box: bool,
    anchor: Rect,
    /// The user's own reading so far.
    custom: String,
}

/// What the user picked in the popup.
enum Choice {
    Reading(String, Via),
    DeleteStruck,
}

/// How an uncertain word is marked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Open,
    /// Corrected, kept or deleted by the user.
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
        if ui.input(|i| i.pointer.any_pressed()) {
            self.hint = None;
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
        self.edit_hint(ui);
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

    /// Whether `target` is highlighted: under the pointer, or being corrected.
    fn is_hot(&self, target: &Target) -> bool {
        self.hovered.as_ref() == Some(target)
            || self
                .correcting
                .as_ref()
                .is_some_and(|c| c.target == *target)
    }

    fn open_correction(&mut self, target: Target, from_box: bool, anchor: Rect) {
        self.editing_line = None;
        self.correcting = Some(Correcting {
            target,
            from_box,
            anchor,
            custom: String::new(),
        });
    }

    /// Keeps the popup next to what it opened from, if that moved.
    fn follow(&mut self, target: &Target, from_box: bool, rect: Rect) {
        if let Some(c) = &mut self.correcting
            && c.target == *target
            && c.from_box == from_box
        {
            c.anchor = rect;
        }
    }

    /// The hot line's box and the uncertain words' boxes over the page drawn at `image_rect`, cut
    /// from the photo at `crop`. Boxes follow the page if it was cut elsewhere when transcribed,
    /// slanting if it's turned. See `decisions/2026-10-30-boxes-follow-corners.md`.
    fn boxes(
        &mut self,
        ui: &mut egui::Ui,
        transcript: &Transcript,
        marks: &[Mark],
        image_rect: Rect,
        crop: Crop,
        hovered: &mut Option<Target>,
    ) {
        let reframe = Reframe::new(transcript.crop, crop);
        let (w, h) = crop.size;
        let scale = image_rect.size() / Vec2::new(w as f32, h as f32);
        let painter = ui.painter_at(image_rect);
        // Where `bbox` is on screen, painted with `fill` and `stroke`.
        let paint = |bbox: Bbox, fill: Color32, stroke: Stroke| {
            let quad: Vec<Pos2> = reframe
                .quad(bbox)
                .into_iter()
                .map(|[x, y]| image_rect.min + Vec2::new(x, y) * scale)
                .collect();
            let rect = Rect::from_points(&quad).intersect(image_rect);
            if reframe.is_identity() {
                painter.rect(rect, 2.0, fill, stroke, StrokeKind::Outside);
            } else {
                painter.add(Shape::convex_polygon(quad, fill, stroke));
            }
            rect
        };

        let hot_line = self.hot_line.and_then(|l| transcript.lines.get(l)?.bbox);
        if let Some(bbox) = hot_line {
            let selection = ui.visuals().selection.bg_fill;
            paint(
                bbox,
                selection.gamma_multiply(0.15),
                Stroke::new(1.0, selection),
            );
        }

        for (i, (u, &mark)) in transcript.uncertain.iter().zip(marks).enumerate() {
            let Some(bbox) = u.bbox else {
                continue;
            };
            let target = Target::Uncertain(i);
            let hot = self.is_hot(&target);
            let rect = paint(bbox, highlight(ui, mark, hot), outline(ui, mark, hot));
            let correctable = mark != Mark::Unfound;
            let sense = if correctable {
                Sense::click()
            } else {
                Sense::hover()
            };
            let response = ui.interact(rect, ui.scope_id().with(("uncertain box", i)), sense);
            // A deleted word is named by the reading it had.
            let name = match u.text.as_str() {
                "" => u.alternatives.first().map_or("", String::as_str),
                text => text,
            };
            response.widget_info(|| {
                WidgetInfo::labeled(Role::Button, correctable, format!("Box: {name}"))
            });

            let response = if !correctable {
                response.on_hover_text(why_unfound(transcript, i))
            } else if u.text.is_empty() {
                response
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text(format!(
                        "Deleted \"{name}\": click to put it back or change it"
                    ))
            } else {
                response.on_hover_cursor(CursorIcon::PointingHand)
            };
            if response.hovered() {
                *hovered = Some(target.clone());
            }
            if response.clicked() {
                self.open_correction(target.clone(), true, rect);
                self.scroll_to = Some(i);
            }
            self.follow(&target, true, rect);
        }
    }

    /// The transcript, one row per line, with uncertain words and struck text marked.
    fn lines(
        &mut self,
        ui: &mut egui::Ui,
        transcript: &mut Transcript,
        spans: &[Option<Span>],
        marks: &[Mark],
        hovered: &mut Option<Target>,
    ) {
        let mut targets: Vec<Vec<(Target, Range<usize>)>> =
            vec![Vec::new(); transcript.lines.len()];
        for (i, span) in spans.iter().enumerate() {
            // A deleted word has nothing left in the text to click; its box still does.
            if let Some(Span { line, range }) = span
                && !range.is_empty()
            {
                targets[*line].push((Target::Uncertain(i), range.clone()));
            }
        }
        for (line, targets) in targets.iter_mut().enumerate() {
            let struck = struck_ranges(&transcript.lines[line].text_md);
            let overlaps = |r: &Range<usize>, (_, w): &(Target, Range<usize>)| {
                r.start < w.end && w.start < r.end
            };
            let plain: Vec<_> = struck
                .into_iter()
                .filter(|r| !targets.iter().any(|t| overlaps(r, t)))
                .map(|range| {
                    (
                        Target::Struck {
                            line,
                            range: range.clone(),
                        },
                        range,
                    )
                })
                .collect();
            targets.extend(plain);
            targets.sort_by_key(|(_, range)| range.start);
        }
        let scroll_line = self
            .scroll_to
            .take()
            .and_then(|i| Some(spans.get(i)?.as_ref()?.line));
        let now = ui.input(|i| i.time);
        let mut hot_line = None;

        for (line, targets) in targets.iter().enumerate() {
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
                hot_line = Some(line);
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

            let text = transcript.lines[line].text_md.as_str();
            let mut double_clicked = false;
            let mut clicked_at = None;
            let row = ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut plain = |ui: &mut egui::Ui, text: &str| {
                    let label = Label::new(RichText::new(text).monospace()).sense(Sense::click());
                    let response = ui.add(label);
                    double_clicked |= response.double_clicked();
                    if response.clicked() && !response.double_clicked() {
                        clicked_at = response.interact_pointer_pos();
                    }
                };
                let mut at = 0;
                for (target, range) in targets {
                    // Two uncertain words can overlap; only the first is marked.
                    if range.start < at {
                        continue;
                    }
                    if range.start > at {
                        plain(ui, &text[at..range.start]);
                    }
                    let hot = self.is_hot(target);
                    let mut word = RichText::new(&text[range.clone()]).monospace();
                    word = match target {
                        Target::Uncertain(i) => {
                            let word = word.background_color(highlight(ui, marks[*i], hot));
                            if marks[*i] == Mark::Resolved {
                                word.underline()
                            } else {
                                word
                            }
                        }
                        Target::Struck { .. } => word.background_color(struck_highlight(ui, hot)),
                    };
                    let response = ui
                        .add(Label::new(word).sense(Sense::click()))
                        .on_hover_cursor(CursorIcon::PointingHand);
                    if response.hovered() {
                        *hovered = Some(target.clone());
                    }
                    if response.clicked() {
                        self.open_correction(target.clone(), false, response.rect);
                    }
                    self.follow(target, false, response.rect);
                    at = range.end;
                }
                // A paragraph break still needs something to double-click.
                if at < text.len() || text.is_empty() {
                    plain(ui, if text.is_empty() { " " } else { &text[at..] });
                }
            });
            if ui.rect_contains_pointer(row.response.rect) {
                hot_line = Some(line);
            }
            if scroll_line == Some(line) {
                ui.scroll_to_rect(row.response.rect, Some(Align::Center));
            }
            if double_clicked {
                self.correcting = None;
                self.hint = None;
                self.editing_line = Some((line, text.to_owned()));
            } else if let Some(at) = clicked_at {
                self.hint = Some((at, now));
            }
        }
        self.hot_line = hot_line;
    }

    /// After a single click on a line, once it's clearly not the first of a double-click, says
    /// how to edit the line, until the pointer moves away.
    fn edit_hint(&mut self, ui: &egui::Ui) {
        let Some((at, when)) = self.hint else {
            return;
        };
        let (now, pointer) = ui.input(|i| (i.time, i.pointer.hover_pos()));
        let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
        let shown_for = now - when - delay;
        let moved = pointer.is_none_or(|p| p.distance(at) > HINT_SLOP);
        if moved || shown_for > HINT_SECONDS || self.editing_line.is_some() {
            self.hint = None;
            return;
        }
        if shown_for < 0.0 {
            ui.ctx().request_repaint_after_secs(-shown_for as f32);
            return;
        }
        ui.ctx()
            .request_repaint_after_secs((HINT_SECONDS - shown_for) as f32);
        let id = Id::unique("edit line hint");
        Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), id, at).show(|ui| {
            ui.label(EDIT_HINT);
        });
    }

    /// For an uncertain word: Claude's other readings, Keep, a reading of the user's own, and
    /// deleting any struck-through text in it. For other struck text, deleting it.
    fn popup(&mut self, ui: &mut egui::Ui, transcript: &mut Transcript) {
        let Some(c) = &mut self.correcting else {
            return;
        };
        let word = match &c.target {
            Target::Uncertain(i) => transcript.uncertain.get(*i),
            Target::Struck { line, range } => {
                let text = transcript.lines.get(*line).map(|l| &l.text_md);
                if !text.is_some_and(|t| struck_ranges(t).contains(range)) {
                    self.correcting = None;
                    return;
                }
                None
            }
        };
        if word.is_none() && matches!(c.target, Target::Uncertain(_)) {
            self.correcting = None;
            return;
        }
        let mut open = true;
        let mut choice = None;
        // One popup per target, so clicking another closes this one without closing the new one:
        // a popup closes on clicks that start while it's open.
        Popup::new(
            Id::unique("correct").with((&c.target, c.from_box)),
            ui.ctx().clone(),
            c.anchor,
            ui.layer_id(),
        )
        .open_bool(&mut open)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let Some(u) = word else {
                if ui.button("Delete struck text").clicked() {
                    choice = Some(Choice::DeleteStruck);
                }
                return;
            };
            for (i, alternative) in u.alternatives.iter().enumerate() {
                if ui.button(alternative).clicked() {
                    choice = Some(Choice::Reading(alternative.clone(), Via::Alternative(i)));
                }
            }
            // A deleted word's reading is among its alternatives.
            if !u.text.is_empty() && ui.button(format!("Keep \"{}\"", u.text)).clicked() {
                choice = Some(Choice::Reading(u.text.clone(), Via::Keep));
            }
            if !struck_ranges(&u.text).is_empty() && ui.button("Delete struck text").clicked() {
                choice = Some(Choice::DeleteStruck);
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
                    choice = Some(Choice::Reading(custom.to_owned(), Via::Custom));
                }
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            open = false;
        }
        if let Some(choice) = choice {
            let event = apply(transcript, &self.page, &c.target, choice);
            self.events.extend(event);
            open = false;
        }
        if !open {
            self.correcting = None;
        }
    }
}

/// Makes `choice` for `target`, returning what to log if it changed anything.
fn apply(
    transcript: &mut Transcript,
    page: &str,
    target: &Target,
    choice: Choice,
) -> Option<Event> {
    let (page, model) = (page.to_owned(), transcript.model.clone());
    match (target, choice) {
        (&Target::Uncertain(word), choice) => {
            let from = transcript.uncertain.get(word)?.text.clone();
            let (to, via) = match choice {
                Choice::Reading(to, via) => (transcript.resolve(word, &to).then_some(to)?, via),
                Choice::DeleteStruck => {
                    transcript.delete_struck(word).then_some(())?;
                    (transcript.uncertain[word].text.clone(), Via::DeleteStruck)
                }
            };
            Some(Event::Corrected {
                page,
                model,
                word,
                from,
                to,
                via,
            })
        }
        (Target::Struck { line, range }, Choice::DeleteStruck) => {
            let from = transcript.lines.get(*line)?.text_md.clone();
            transcript
                .delete_struck_at(*line, range.clone())
                .then_some(())?;
            Some(Event::LineEdited {
                page,
                model,
                line: *line,
                from,
                to: transcript.lines[*line].text_md.clone(),
            })
        }
        (Target::Struck { .. }, Choice::Reading(..)) => None,
    }
}

/// Why uncertain word `index`'s box has no word to correct.
fn why_unfound(transcript: &Transcript, index: usize) -> String {
    let u = &transcript.uncertain[index];
    match transcript.why_unfound(index) {
        Some(Unfound::EditedOut { .. }) => format!(
            "\"{}\" was edited out of its line, so it can't be corrected here. Reset Transcript \
             brings it back.",
            u.alternatives
                .first()
                .filter(|_| u.text.is_empty())
                .unwrap_or(&u.text)
        ),
        _ => {
            let others = match u.alternatives.as_slice() {
                [] => String::new(),
                others => format!(" (or {})", others.join(" / ")),
            };
            format!(
                "Claude was unsure of \"{}\"{others} here, but it isn't in the transcript. \
                 Double-click its line to fix it by hand.",
                u.text
            )
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

/// The background behind struck text that isn't an uncertain word: only when it's hot.
fn struck_highlight(ui: &egui::Ui, hot: bool) -> Color32 {
    if hot {
        ui.visuals().weak_text_color().gamma_multiply(0.3)
    } else {
        Color32::TRANSPARENT
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
