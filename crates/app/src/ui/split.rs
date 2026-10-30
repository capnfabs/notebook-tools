//! Splits a view in two, with a handle between them to drag: the Transcribed view's page and its
//! transcript, or the Cleaned view's photo and cleaned page. When the window resizes, the
//! transcript gives up width first, down to what it needs not to wrap; growing, it gets that back
//! first, then the page grows until its height limits it. See
//! `decisions/2026-10-24-transcript-split.md`.

use eframe::egui;
use egui::accesskit::Role;
use egui::{CursorIcon, Rect, Sense, Vec2, WidgetInfo};

/// Narrower than this, the transcript goes under the page instead of beside it.
const SIDE_BY_SIDE_MIN_WIDTH: f32 = 600.0;

/// Neither pane gets narrower than this, in points.
const MIN_PANE_PT: f32 = 200.0;

/// What each pane would like, in points.
#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    /// Widest the transcript needs to be not to wrap, if there is one.
    pub text_natural: Option<f32>,
    /// Width at which the page fills the pane's height; wider is just margin.
    pub image_cap: Option<f32>,
}

/// The split, which stays put when another page is shown.
#[derive(Default)]
pub struct Split {
    /// The transcript's width, once there's been a frame.
    text: Option<f32>,
    /// The panes' combined width last frame.
    total: f32,
}

impl Split {
    /// Where the page and the transcript go in `rect`: side by side with a handle of `gap` points
    /// between them, called `label` and told apart from other handles by `id_salt` (which, unlike
    /// the label, doesn't change with the language), or the page above the transcript when `rect`
    /// is narrow.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        gap: f32,
        limits: Limits,
        id_salt: &str,
        label: &str,
    ) -> (Rect, Rect) {
        if rect.width() < SIDE_BY_SIDE_MIN_WIDTH {
            let (page, text) = rect.split_top_bottom_at_fraction(0.5);
            return (
                page.shrink2(Vec2::new(0.0, gap / 2.0)),
                text.shrink2(Vec2::new(0.0, gap / 2.0)),
            );
        }
        let total = rect.width() - gap;
        let text = match self.text {
            Some(text) if total != self.total => resize(text, self.total, total, limits),
            Some(text) => text,
            None => total / 2.0,
        };
        self.total = total;

        let handle = Rect::from_x_y_ranges(
            rect.right() - text - gap..=rect.right() - text,
            rect.y_range(),
        );
        let response = ui
            .interact(handle, ui.scope_id().with(id_salt), Sense::drag())
            .on_hover_cursor(CursorIcon::ResizeHorizontal);
        response.widget_info(|| WidgetInfo::labeled(Role::Splitter, true, label));
        let text = clamp(text - response.drag_delta().x, total);
        self.text = Some(text);

        let visuals = ui.visuals();
        let stroke = if response.hovered() || response.dragged() {
            visuals.widgets.hovered.fg_stroke
        } else {
            visuals.widgets.noninteractive.bg_stroke
        };
        let x = rect.right() - text - gap / 2.0;
        ui.painter().vline(x, rect.y_range(), stroke);

        let (page, rest) = rect.split_left_right_at_x(rect.right() - text - gap);
        (page, rest.with_min_x(rest.left() + gap))
    }
}

/// The transcript's new width when the panes' combined width goes from `old_total` to
/// `new_total`. Shrinking takes from the transcript down to its natural width, then from both
/// panes in proportion. Growing gives to the transcript up to its natural width, then to the page
/// up to its cap, then to the transcript. Without a natural width, both change in proportion.
fn resize(text: f32, old_total: f32, new_total: f32, limits: Limits) -> f32 {
    let image = old_total - text;
    let mut change = new_total - old_total;
    let mut text = text;
    if let Some(natural) = limits.text_natural {
        // Towards the natural width: down to it when shrinking, up to it when growing.
        let towards = (natural - text).clamp(change.min(0.0), change.max(0.0));
        text += towards;
        change -= towards;
        if change > 0.0 {
            let to_image = limits
                .image_cap
                .map_or(change, |cap| (cap - image).clamp(0.0, change));
            return clamp(text + change - to_image, new_total);
        }
    }
    // What's left, in proportion.
    let before = new_total - change;
    let text = if before > 0.0 {
        text + change * text / before
    } else {
        text
    };
    clamp(text, new_total)
}

/// `text` limited so neither pane is narrower than [`MIN_PANE_PT`], or half each if there isn't
/// room for both.
fn clamp(text: f32, total: f32) -> f32 {
    if total < 2.0 * MIN_PANE_PT {
        total / 2.0
    } else {
        text.clamp(MIN_PANE_PT, total - MIN_PANE_PT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NATURAL: Limits = Limits {
        text_natural: Some(400.0),
        image_cap: None,
    };

    #[test]
    fn shrinking_takes_from_the_transcript_down_to_its_natural_width() {
        assert_eq!(resize(500.0, 1000.0, 950.0, NATURAL), 450.0);
        assert_eq!(resize(500.0, 1000.0, 900.0, NATURAL), 400.0);
    }

    #[test]
    fn shrinking_past_the_natural_width_takes_from_both_in_proportion() {
        // 100 from the transcript, then 400:500 of the remaining 90.
        assert_eq!(resize(500.0, 1000.0, 810.0, NATURAL), 360.0);
        // Already wrapping.
        assert_eq!(resize(300.0, 900.0, 600.0, NATURAL), 200.0);
    }

    #[test]
    fn growing_gives_to_the_transcript_up_to_its_natural_width_then_the_page() {
        assert_eq!(resize(300.0, 800.0, 850.0, NATURAL), 350.0);
        assert_eq!(resize(300.0, 800.0, 1000.0, NATURAL), 400.0);
    }

    #[test]
    fn growing_past_the_pages_cap_gives_to_the_transcript() {
        let limits = Limits {
            image_cap: Some(550.0),
            ..NATURAL
        };
        // The page is 500, so it takes 50; the transcript takes the other 50.
        assert_eq!(resize(400.0, 900.0, 1000.0, limits), 450.0);
        // Already past its cap.
        assert_eq!(resize(400.0, 1000.0, 1100.0, limits), 500.0);
    }

    #[test]
    fn without_a_transcript_both_change_in_proportion() {
        let none = Limits::default();
        assert_eq!(resize(300.0, 900.0, 1200.0, none), 400.0);
        assert_eq!(resize(300.0, 900.0, 600.0, none), 200.0);
    }

    #[test]
    fn panes_keep_a_minimum_width() {
        assert_eq!(resize(250.0, 900.0, 450.0, NATURAL), MIN_PANE_PT);
        assert_eq!(clamp(100.0, 1000.0), MIN_PANE_PT);
        assert_eq!(clamp(900.0, 1000.0), 800.0);
        assert_eq!(clamp(100.0, 300.0), 150.0);
    }
}
