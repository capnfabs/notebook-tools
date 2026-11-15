//! The Transcribed view: the cleaned page beside its transcript, where the user corrects it.
//!
//! Uncertain words are marked on both sides; clicking one, or its box on the page, offers Claude's
//! other readings or one of the user's own. Struck-through text can be deleted. Double-clicking a
//! line edits it, which a single click hints at, and pointing at a line outlines its box.
//!
//! The keyboard does the same in the text: Tab goes through it, scrolling to what has the focus
//! and highlighting it as pointing does; Enter on a marked word opens its popup with the
//! focus inside, and Enter on the rest of a line edits it. Escape, or picking a reading, puts the
//! focus back. Another page's transcript takes the focus at its top. The boxes on the page are
//! for the pointer only, out of the Tab order.
//!
//! See `decisions/2026-10-23-transcript-corrections.md`,
//! `decisions/2026-10-30-correction-ui.md` and `decisions/2026-10-31-keyboard-focus.md`.

use std::ops::Range;

use eframe::egui;
use egui::accesskit::Role;
use egui::{
    Align, Color32, CursorIcon, Id, Key, Label, PointerButton, Popup, PopupCloseBehavior, Pos2,
    Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, TextStyle, Tooltip,
    UiBuilder, Vec2, WidgetInfo,
};
use notebook_core::clean::Crop;
use notebook_core::reframe::Reframe;
use notebook_core::transcript::{Bbox, Span, Transcript, Unfound, struck_ranges};

use crate::eval_log::{Event, PageRef, Via};
use crate::t;

/// Room for corrections to lengthen lines, as a multiple of the longest line.
const EDIT_ROOM: f32 = 1.2;

/// How long the hint to double-click a line stays, in seconds.
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
    page: Option<PageRef>,
    /// What's under the pointer, and what has the keyboard focus, last frame: highlighted on both
    /// sides.
    seen: Seen,
    /// The line under the pointer, being edited or with the focus last frame, outlined on the page.
    hot_line: Option<usize>,
    /// An uncertain word whose box was just clicked, for the transcript to scroll to.
    scroll_to: Option<usize>,
    /// Where a line was clicked once, and when: once it's not a double-click, says how to edit.
    hint: Option<(Pos2, f64)>,
    correcting: Option<Correcting>,
    editing_line: Option<EditingLine>,
    /// What gets the keyboard focus back this frame, once it's drawn, and next frame.
    refocus: Option<Refocus>,
    refocus_next: Option<Refocus>,
    /// The last pass in which something here had the keyboard focus. If it was the one before a
    /// new page is shown, the new page's transcript takes the focus at the top.
    focused_in_pass: Option<u64>,
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

/// What's highlighted because the user is pointing at it, or it has the keyboard focus.
#[derive(Default)]
struct Seen {
    hovered: Option<Target>,
    focused: Option<Target>,
}

/// Where the keyboard focus goes back to, after the popup or editing a line: the piece of line
/// `line` it came from (`from`), if that's still there, else what's now in its place. That's the first piece
/// of the line reaching past byte `at`, or else the first piece of a later line, so when what it
/// came from is gone, such as deleted struck text, the focus goes on to the next thing.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Refocus {
    line: usize,
    at: usize,
    from: Option<Piece>,
}

/// A piece of a line, as the focus remembers it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Target(Target),
    /// Plain text: this text.
    Plain(String),
}

impl Refocus {
    /// The first piece of the transcript.
    const TOP: Self = Self {
        line: 0,
        at: 0,
        from: None,
    };

    /// Which of line `line`'s `pieces` gets the focus, given that no earlier line's did. Each piece
    /// is what it is, if it can be corrected, and where it is in the line's `text`.
    fn pick(
        &self,
        line: usize,
        text: &str,
        pieces: &[(Option<Target>, Range<usize>, Response)],
    ) -> Option<usize> {
        if line < self.line {
            return None;
        }
        if line == self.line {
            // The same piece; or if there are several the same, the nearest.
            let same = pieces
                .iter()
                .enumerate()
                .filter(|(_, (target, range, _))| match (&self.from, target) {
                    (Some(Piece::Target(from)), Some(target)) => from == target,
                    (Some(Piece::Plain(from)), None) => text[range.clone()] == *from,
                    _ => false,
                })
                .min_by_key(|(_, (_, range, _))| range.start.abs_diff(self.at));
            if let Some((i, _)) = same {
                return Some(i);
            }
        }
        // An empty line's piece is empty, at 0.
        pieces.iter().position(|(_, range, _)| {
            line > self.line || range.end > self.at || range.start >= self.at
        })
    }
}

/// The popup for correcting a [`Target`].
struct Correcting {
    target: Target,
    /// Opened from the word's box on the page, rather than from the text.
    from_box: bool,
    anchor: Rect,
    /// The user's own reading so far.
    custom: String,
    /// Opened from the keyboard: the popup takes the focus, and gives it back here when it's done.
    return_to: Option<Refocus>,
    /// The popup is yet to take the focus.
    take_focus: bool,
    /// What in the popup could take the focus last frame.
    items: Vec<Id>,
}

/// A line being edited.
struct EditingLine {
    line: usize,
    /// Its text so far.
    text: String,
    /// Started from the keyboard: where the focus goes back to when it's done.
    return_to: Option<Refocus>,
}

/// What happened to a line's row.
struct Row {
    rect: Rect,
    /// To edit the line: double-clicked, or Enter pressed on its text, and then where the focus
    /// goes back to after.
    edit: Option<Option<Refocus>>,
    /// Where it was clicked once, if it was.
    clicked_at: Option<Pos2>,
    /// Something in it has the keyboard focus.
    focused: bool,
}

/// What the user picked in the popup.
enum Pick {
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
        page: PageRef,
        transcript: &mut Transcript,
        image: Option<(Rect, Crop)>,
        text_rect: Rect,
    ) -> Vec<Event> {
        let pass = ui.ctx().cumulative_pass_nr();
        // A page's name can change, as when its photo is found, without it being another page.
        if self.page.as_ref().is_none_or(|p| p.id != page.id) {
            let had_focus = self.focused_in_pass.is_some_and(|p| p + 1 == pass);
            *self = Self::default();
            if had_focus {
                self.refocus_next = Some(Refocus::TOP);
            }
        }
        self.page = Some(page.clone());
        if ui.input(|i| i.pointer.any_pressed()) {
            self.hint = None;
        }
        self.refocus = self.refocus_next.take();
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

        let mut seen = Seen::default();
        if let Some((image_rect, crop)) = image {
            self.boxes(ui, transcript, &marks, image_rect, crop, &mut seen);
        }
        ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    self.lines(ui, transcript, &spans, &marks, &mut seen);
                });
        });
        self.popup(ui, transcript);
        self.edit_hint(ui);
        self.seen = seen;

        // By correcting the last open word, or editing it out of its line.
        if open > 0 && transcript.open_uncertain() == 0 {
            let spans = transcript.uncertain_spans();
            self.events.push(Event::AllResolved {
                page,
                model: transcript.model.clone(),
                uncertain: spans.len(),
                unfound: spans.iter().filter(|s| s.is_none()).count(),
            });
        }
        std::mem::take(&mut self.events)
    }

    /// Whether `target` is highlighted: under the pointer, with the focus, or being corrected.
    fn is_hot(&self, target: &Target) -> bool {
        self.seen.hovered.as_ref() == Some(target)
            || self.seen.focused.as_ref() == Some(target)
            || self
                .correcting
                .as_ref()
                .is_some_and(|c| c.target == *target)
    }

    fn open_correction(
        &mut self,
        target: Target,
        from_box: bool,
        anchor: Rect,
        return_to: Option<Refocus>,
    ) {
        self.editing_line = None;
        self.correcting = Some(Correcting {
            target,
            from_box,
            anchor,
            custom: String::new(),
            take_focus: return_to.is_some(),
            return_to,
            items: Vec::new(),
        });
    }

    /// Gives the keyboard focus to `refocus` next frame.
    fn refocus_next(&mut self, ui: &egui::Ui, refocus: Refocus) {
        self.refocus_next = Some(refocus);
        ui.ctx().request_repaint();
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
        seen: &mut Seen,
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
            let hot = self.is_hot(&Target::Uncertain(i));
            let rect = paint(bbox, highlight(ui, mark, hot), outline(ui, mark, hot));
            self.uncertain_box(ui, transcript, i, mark, rect, seen);
        }
    }

    /// Makes uncertain word `index`'s box, painted at `rect`, clickable to correct it, or says on
    /// hover why it can't be. Out of the Tab order: the keyboard corrects the word in the text.
    fn uncertain_box(
        &mut self,
        ui: &mut egui::Ui,
        transcript: &Transcript,
        index: usize,
        mark: Mark,
        rect: Rect,
        seen: &mut Seen,
    ) {
        let u = &transcript.uncertain[index];
        let target = Target::Uncertain(index);
        let correctable = mark != Mark::Unfound;
        let sense = if correctable {
            Sense::CLICK
        } else {
            Sense::hover()
        };
        let response = ui.interact(rect, ui.scope_id().with(("uncertain box", index)), sense);
        let name = u.reading();
        response.widget_info(|| {
            WidgetInfo::labeled(Role::Button, correctable, t!("uncertain-box", word = name))
        });

        let response = if !correctable {
            response.on_hover_text(why_unfound(transcript, index))
        } else if u.text.is_empty() {
            response
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(t!("deleted-word-hint", word = name))
        } else {
            response.on_hover_cursor(CursorIcon::PointingHand)
        };
        if response.hovered() {
            seen.hovered = Some(target.clone());
        }
        if response.clicked() {
            self.open_correction(target.clone(), true, rect, None);
            self.scroll_to = Some(index);
        }
        self.follow(&target, true, rect);
    }

    /// The transcript, one row per line, with uncertain words and struck text marked.
    fn lines(
        &mut self,
        ui: &mut egui::Ui,
        transcript: &mut Transcript,
        spans: &[Option<Span>],
        marks: &[Mark],
        seen: &mut Seen,
    ) {
        let targets = line_targets(transcript, spans);
        let scroll_line = self
            .scroll_to
            .take()
            .and_then(|i| Some(spans.get(i)?.as_ref()?.line));
        let now = ui.input(|i| i.time);
        let (mut editing, mut pointed, mut focused) = (None, None, None);

        for (line, targets) in targets.iter().enumerate() {
            if self.editing_line.as_ref().is_some_and(|e| e.line == line) {
                self.edit_line(ui, transcript, line);
                editing = Some(line);
                continue;
            }

            let text = transcript.lines[line].text_md.as_str();
            let row = self.show_line_row(ui, line, text, targets, marks, seen);
            if ui.rect_contains_pointer(row.rect) {
                pointed = Some(line);
            }
            if row.focused {
                focused = Some(line);
            }
            if scroll_line == Some(line) {
                ui.scroll_to_rect(row.rect, Some(Align::Center));
            }
            if let Some(return_to) = row.edit {
                self.correcting = None;
                self.hint = None;
                self.editing_line = Some(EditingLine {
                    line,
                    text: text.to_owned(),
                    return_to,
                });
            } else if let Some(at) = row.clicked_at {
                self.hint = Some((at, now));
            }
        }
        self.hot_line = editing.or(pointed).or(focused);
        if focused.is_some() {
            self.focused_in_pass = Some(ui.ctx().cumulative_pass_nr());
        }
    }

    /// Line `line` in a text field, being edited. Enter or a click elsewhere keeps the edit;
    /// Escape drops it.
    fn edit_line(&mut self, ui: &mut egui::Ui, transcript: &mut Transcript, line: usize) {
        let Some(EditingLine {
            text, return_to, ..
        }) = &mut self.editing_line
        else {
            return;
        };
        let response = ui.add(
            TextEdit::singleline(text)
                // Also names the field for screen readers.
                .hint_text(t!("edit-empty-line"))
                .font(TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
        if response.lost_focus() {
            let from = &transcript.lines[line].text_md;
            let (enter, escape) =
                ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            let return_to = return_to.take();
            if !escape
                && *from != *text
                && let Some(page) = self.page.clone()
            {
                self.events.push(Event::LineEdited {
                    page,
                    model: transcript.model.clone(),
                    line,
                    from: from.clone(),
                    to: text.clone(),
                });
                transcript.set_line(line, std::mem::take(text));
            }
            self.editing_line = None;
            if let Some(refocus) = return_to
                && (enter || escape)
            {
                self.refocus_next(ui, refocus);
            }
        } else if !response.has_focus() {
            response.request_focus();
        }
    }

    /// Line `line`'s `text`, with its uncertain words and struck text (`targets`) marked to click.
    fn show_line_row(
        &mut self,
        ui: &mut egui::Ui,
        line: usize,
        text: &str,
        targets: &[(Target, Range<usize>)],
        marks: &[Mark],
        seen: &mut Seen,
    ) -> Row {
        // Each piece of the line: what it is if it can be corrected, and where it is in `text`.
        let mut pieces: Vec<(Option<Target>, Range<usize>, Response)> = Vec::new();
        let row = ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let plain = |ui: &mut egui::Ui, text: &str| {
                ui.add(Label::new(RichText::new(text).monospace()).sense(Sense::click()))
            };
            let mut at = 0;
            for (target, range) in targets {
                // Two uncertain words can overlap; only the first is marked.
                if range.start < at {
                    continue;
                }
                if range.start > at {
                    pieces.push((None, at..range.start, plain(ui, &text[at..range.start])));
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
                let response = ui.add(Label::new(word).sense(Sense::click()));
                pieces.push((Some(target.clone()), range.clone(), response));
                at = range.end;
            }
            // A paragraph break still needs something to double-click.
            if at < text.len() || text.is_empty() {
                let shown = if text.is_empty() { " " } else { &text[at..] };
                pieces.push((None, at..text.len(), plain(ui, shown)));
            }
        });

        let mut row = Row {
            rect: row.response.rect,
            edit: None,
            clicked_at: None,
            focused: false,
        };
        let refocus = self
            .refocus
            .as_ref()
            .and_then(|r| r.pick(line, text, &pieces));
        if refocus.is_some() {
            self.refocus = None;
        }
        for (n, (target, range, response)) in pieces.into_iter().enumerate() {
            if refocus == Some(n) {
                response.request_focus();
            }
            if response.gained_focus() {
                response.scroll_to_me(None);
            }
            row.focused |= response.has_focus();
            let Some(target) = target else {
                if response.double_clicked() {
                    row.edit = Some(None);
                } else if by_key(&response) {
                    row.edit = Some(Some(Refocus {
                        line,
                        at: range.start,
                        from: Some(Piece::Plain(text[range].to_owned())),
                    }));
                } else if response.clicked() {
                    row.clicked_at = response.interact_pointer_pos();
                }
                continue;
            };
            let response = response.on_hover_cursor(CursorIcon::PointingHand);
            if response.hovered() {
                seen.hovered = Some(target.clone());
            }
            if response.has_focus() {
                seen.focused = Some(target.clone());
            }
            if response.clicked() {
                let return_to = by_key(&response).then(|| Refocus {
                    line,
                    at: range.start,
                    from: Some(Piece::Target(target.clone())),
                });
                self.open_correction(target.clone(), false, response.rect, return_to);
            }
            self.follow(&target, false, response.rect);
        }
        row
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
            ui.label(t!("edit-hint"));
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
        // Tab and Shift-Tab go round the popup, rather than out of it: only Escape or a choice
        // leaves. egui moves the focus as the focused widget is added, so this goes first.
        let focused = ui.memory(|m| m.focused());
        let tab = if focused.is_some_and(|f| c.items.contains(&f)) {
            crate::ui::focus::keep_tab(ui.ctx())
        } else {
            0
        };
        let mut open = true;
        let mut choice = None;
        // What in the popup can take the focus, in order, and which is the text field.
        let mut items: Vec<Response> = Vec::new();
        let mut field = None;
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
            let button = |ui: &mut egui::Ui, items: &mut Vec<Response>, text: String| {
                let response = ui.button(text);
                items.push(response.clone());
                response.clicked()
            };
            let Some(u) = word else {
                if button(ui, &mut items, t!("delete-struck")) {
                    choice = Some(Pick::DeleteStruck);
                }
                return;
            };
            for (i, alternative) in u.alternatives.iter().enumerate() {
                if button(ui, &mut items, alternative.clone()) {
                    choice = Some(Pick::Reading(alternative.clone(), Via::Alternative(i)));
                }
            }
            // A deleted word's reading is among its alternatives.
            let keep = t!("keep-word", word = u.text.as_str());
            if !u.text.is_empty() && button(ui, &mut items, keep) {
                choice = Some(Pick::Reading(u.text.clone(), Via::Keep));
            }
            if !struck_ranges(&u.text).is_empty() && button(ui, &mut items, t!("delete-struck")) {
                choice = Some(Pick::DeleteStruck);
            }
            ui.horizontal(|ui| {
                let edit = ui.add(
                    TextEdit::singleline(&mut c.custom)
                        .hint_text(t!("custom-reading"))
                        .desired_width(160.0),
                );
                field = Some(items.len());
                items.push(edit.clone());
                let custom = c.custom.trim();
                let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let ok = egui::Button::new(t!("custom-reading-ok"));
                let ok = ui.add_enabled(!custom.is_empty(), ok);
                if ok.enabled() {
                    items.push(ok.clone());
                }
                if !custom.is_empty() && (ok.clicked() || entered) {
                    choice = Some(Pick::Reading(custom.to_owned(), Via::Custom));
                }
            });
        });
        if c.take_focus
            && let Some(first) = items.first()
        {
            first.request_focus();
            c.take_focus = false;
        }
        focus_within(ui, &items, field, tab);
        if items.iter().any(Response::has_focus) {
            self.focused_in_pass = Some(ui.ctx().cumulative_pass_nr());
        }
        c.items = items.iter().map(|r| r.id).collect();
        // Escape and picking a reading put the focus back; clicking elsewhere doesn't.
        let mut done = ui.input(|i| i.key_pressed(Key::Escape));
        if let Some(choice) = choice {
            let event = self
                .page
                .clone()
                .and_then(|page| apply(transcript, page, &c.target, choice));
            self.events.extend(event);
            done = true;
        }
        if done && let Some(refocus) = c.return_to.take() {
            self.refocus_next(ui, refocus);
        }
        if done || !open {
            self.correcting = None;
        }
    }
}

/// Moves the keyboard focus around `items`, if one of them has it, never out of them: to the
/// previous or next with the arrow keys, stopping at the ends, and `tab` places on, going round.
/// Left and right are the text field's, `items[field]`, when it has the focus.
fn focus_within(ui: &egui::Ui, items: &[Response], field: Option<usize>, tab: isize) {
    let Some(at) = items.iter().position(Response::has_focus) else {
        return;
    };
    crate::ui::focus::keep_arrows(ui.ctx());
    let to = if tab != 0 {
        (at as isize + tab).rem_euclid(items.len() as isize) as usize
    } else {
        let step = ui.input(|i| {
            let pressed = |key| i.key_pressed(key) && !i.modifiers.any();
            let sideways = field != Some(at);
            let back = pressed(Key::ArrowUp) || (sideways && pressed(Key::ArrowLeft));
            let on = pressed(Key::ArrowDown) || (sideways && pressed(Key::ArrowRight));
            isize::from(on) - isize::from(back)
        });
        at.saturating_add_signed(step).min(items.len() - 1)
    };
    if to != at {
        items[to].request_focus();
    }
}

/// Whether `response` was clicked from the keyboard, with Enter or Space while it had the focus.
fn by_key(response: &Response) -> bool {
    response.clicked() && !response.clicked_by(PointerButton::Primary)
}

/// What can be clicked in each of `transcript`'s lines, in order: the uncertain words found at
/// `spans`, and struck text that isn't part of one.
fn line_targets(
    transcript: &Transcript,
    spans: &[Option<Span>],
) -> Vec<Vec<(Target, Range<usize>)>> {
    let mut targets: Vec<Vec<(Target, Range<usize>)>> = vec![Vec::new(); transcript.lines.len()];
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
        let overlaps =
            |r: &Range<usize>, (_, w): &(Target, Range<usize>)| r.start < w.end && w.start < r.end;
        let plain: Vec<_> = struck
            .into_iter()
            .filter(|r| !targets.iter().any(|t| overlaps(r, t)))
            .map(|range| {
                let target = Target::Struck {
                    line,
                    range: range.clone(),
                };
                (target, range)
            })
            .collect();
        targets.extend(plain);
        targets.sort_by_key(|(_, range)| range.start);
    }
    targets
}

/// Makes `choice` for `target`, returning what to log if it changed anything.
fn apply(
    transcript: &mut Transcript,
    page: PageRef,
    target: &Target,
    choice: Pick,
) -> Option<Event> {
    let model = transcript.model.clone();
    match (target, choice) {
        (&Target::Uncertain(word), choice) => {
            let from = transcript.uncertain.get(word)?.text.clone();
            let (to, via) = match choice {
                Pick::Reading(to, via) => (transcript.resolve(word, &to).then_some(to)?, via),
                Pick::DeleteStruck => {
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
        (Target::Struck { line, range }, Pick::DeleteStruck) => {
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
        (Target::Struck { .. }, Pick::Reading(..)) => None,
    }
}

/// Why uncertain word `index`'s box has no word to correct.
fn why_unfound(transcript: &Transcript, index: usize) -> String {
    let u = &transcript.uncertain[index];
    match transcript.why_unfound(index) {
        Some(Unfound::EditedOut { .. }) => t!("unfound-edited-out", word = u.reading()),
        _ => match u.alternatives.as_slice() {
            [] => t!("unfound-not-in-transcript", word = u.text.as_str()),
            others => t!(
                "unfound-not-in-transcript-alternatives",
                word = u.text.as_str(),
                alternatives = others.join(" / ")
            ),
        },
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
