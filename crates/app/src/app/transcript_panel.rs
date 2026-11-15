//! What the Transcribed view says around the transcript: where it came from, with buttons to reset
//! or retranscribe it, or how transcribing the page is going.

use std::time::SystemTime;

use eframe::egui;
use egui::text::LayoutJob;
use egui::{
    Align, Color32, FontSelection, Label, Layout, Rect, RichText, TextStyle, TextWrapMode,
    UiBuilder, WidgetText,
};
use jiff::tz::TimeZone;
use notebook_core::transcript::Transcript;

use super::NotebookApp;
use super::chrome::spinner;
use super::commands::with_shortcut;
use crate::command::Command;
use crate::t;

/// The most lines the transcript's status takes beside its buttons, wrapped; any more and it goes
/// above them.
const MAX_STATUS_LINES_BESIDE: usize = 2;

impl NotebookApp {
    /// Where page `index`'s transcript came from, or why retranscribing it failed, and buttons to
    /// reset or retranscribe it, at the bottom of `rect`. Returns where it went, if the page has a
    /// transcript. See `decisions/2026-10-31-transcript-status-layout.md`.
    pub(super) fn show_transcript_status(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        rect: Rect,
    ) -> Option<Rect> {
        let lines = self.status_lines(ui, index)?;
        let page = self.pages.get(index)?;
        // Why Force Retranscribe is disabled.
        let disabled_why = if page.missing_photo().is_some() {
            t!("cant-transcribe-missing", name = page.name.as_str())
        } else {
            t!("no-transcriber-hint")
        };

        // Buttons first, so the text gets what room they leave. Every widget here is sized before
        // it's placed, so `Align::Max` puts its bottom at the bottom of `rect`.
        let row = Layout::right_to_left(Align::Max);
        let scope = ui.scope_builder(UiBuilder::new().max_rect(rect).layout(row), |ui| {
            let force = self
                .command_button(ui, Command::ForceRetranscribe)
                .on_disabled_hover_text(disabled_why);
            let reset = self.command_button(ui, Command::ResetTranscript);
            let buttons = force.rect | reset.rect;
            // Wrapped to the room left of the buttons, to count its lines.
            let galley = WidgetText::from(layout_job(ui, &lines)).into_galley(
                ui,
                Some(TextWrapMode::Wrap),
                ui.available_width(),
                TextStyle::Body,
            );
            let beside = (galley.rows.len() <= MAX_STATUS_LINES_BESIDE).then(|| {
                let layout = Layout::left_to_right(Align::Max);
                ui.with_layout(layout, |ui| ui.add(Label::new(galley)))
                    .inner
                    .rect
            });
            (reset.clicked(), force.clicked(), buttons, beside)
        });
        let (reset, force, buttons, beside) = scope.inner;
        // Too long to go beside the buttons: above them, full width.
        let text = beside.unwrap_or_else(|| {
            let above = rect.with_max_y(buttons.top() - ui.spacing().item_spacing.y);
            let layout = Layout::bottom_up(Align::Min);
            let builder = UiBuilder::new().max_rect(above).layout(layout);
            let label = Label::new(layout_job(ui, &lines)).wrap();
            ui.scope_builder(builder, |ui| ui.add(label)).inner.rect
        });
        if reset {
            self.request_reset(index);
        }
        if force {
            self.request_retranscribe(index);
        }
        Some(text | buttons)
    }

    /// The lines of page `index`'s transcript status, each in its own colour: only warnings look
    /// like warnings. `None` if it has no transcript.
    fn status_lines(&self, ui: &egui::Ui, index: usize) -> Option<Vec<(String, Option<Color32>)>> {
        let page = self.pages.get(index)?;
        let transcript = page.transcript.as_ref()?;
        let when = when_and_by(transcript, &TimeZone::system());
        let error = self.transcriptions.error(page.id);
        let text = match error {
            Some(error) => t!(
                "transcript-retranscribe-failed",
                error = error,
                when = when.as_str()
            ),
            None if transcript.from_cache => t!("transcript-from-cache", when = when.as_str()),
            None => t!("transcript-from-claude", when = when.as_str()),
        };
        let mut lines = vec![(text, error.map(|_| ui.visuals().error_fg_color))];
        let warn = Some(ui.visuals().warn_fg_color);
        let fit = page
            .loaded
            .as_ref()
            .map(|l| transcript.fit(l.report.crop()));
        let lines_cut_off = fit.map_or(0, |f| f.lines_cut_off);
        if lines_cut_off > 0 {
            lines.push((t!("transcript-lines-cut-off", count = lines_cut_off), warn));
        }
        if fit.is_some_and(|f| f.missed_page()) {
            lines.push((t!("transcript-missed-page"), warn));
        }
        Some(lines)
    }

    /// Page `index` being transcribed, or why it couldn't be, or a button to transcribe it.
    pub(super) fn show_transcription_progress(&mut self, ui: &mut egui::Ui, index: usize) {
        let Some(page) = self.pages.get(index) else {
            return;
        };
        let name = &page.name;
        let start = if page.missing_photo().is_some() {
            ui.label(t!("cant-transcribe-missing", name = name.as_str()));
            false
        } else if self.transcriptions.queued_or_running(page.id) {
            spinner(ui, &t!("transcribing-page", name = name.as_str()));
            false
        } else if let Some(err) = self.transcriptions.error(page.id) {
            ui.colored_label(
                ui.visuals().error_fg_color,
                t!("transcribe-failed", name = name.as_str(), error = err),
            );
            with_shortcut(ui.button(t!("transcribe-retry")), Command::Transcribe).clicked()
        } else if self.transcriptions.checking(page.id) {
            spinner(ui, &t!("checking-cache", name = name.as_str()));
            false
        } else if !self.can(Command::Transcribe) {
            ui.label(t!("not-cached", name = name.as_str()));
            ui.label(t!("no-transcriber-hint"));
            false
        } else {
            ui.label(t!("not-transcribed-yet", name = name.as_str()));
            self.command_button(ui, Command::Transcribe).clicked()
        };
        if start {
            self.start_transcribing(index);
        }
    }
}

/// `lines` one after another, each in its colour, if it has one.
fn layout_job(ui: &egui::Ui, lines: &[(String, Option<Color32>)]) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (i, (line, colour)) in lines.iter().enumerate() {
        let line = if i == 0 {
            line.clone()
        } else {
            format!("\n{line}")
        };
        let line = match colour {
            Some(colour) => RichText::new(line).color(*colour),
            None => RichText::new(line),
        };
        line.append_to(&mut job, ui.style(), FontSelection::Default, Align::Min);
    }
    job
}

/// When and by which model `transcript` was made: "2026-10-23 14:05 by claude-opus-5-5", in `tz`,
/// "at an unknown time by …" if that can't be shown, or just "by …" if it wasn't cached.
fn when_and_by(transcript: &Transcript, tz: &TimeZone) -> String {
    let model = transcript.model.as_str();
    match transcript.stored_at.map(|at| format_time(at, tz)) {
        Some(Some(time)) => t!("transcribed-when", at = "time", time = time, model = model),
        Some(None) => t!("transcribed-when", at = "unknown", time = "", model = model),
        None => t!("transcribed-when", at = "none", time = "", model = model),
    }
}

/// `time` to the minute, in `tz`, if it can be shown.
fn format_time(time: SystemTime, tz: &TimeZone) -> Option<String> {
    let at = jiff::Timestamp::try_from(time).ok()?;
    Some(
        at.to_zoned(tz.clone())
            .strftime("%Y-%m-%d %H:%M")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn times_are_shown_to_the_minute() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_761_225_959);
        assert_eq!(format_time(at, &TimeZone::UTC).unwrap(), "2025-10-23 13:25");
        let sydney = TimeZone::get("Australia/Sydney").unwrap();
        assert_eq!(format_time(at, &sydney).unwrap(), "2025-10-24 00:25");
    }
}
