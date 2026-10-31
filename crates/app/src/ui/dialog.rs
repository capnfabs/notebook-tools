//! Modal dialogs laid out the macOS way: buttons at the bottom right, the affirmative one
//! rightmost. Enter picks the affirmative button, unless a widget has the keyboard focus, and
//! Escape cancels.
//!
//! See `decisions/2026-10-23-dialog-buttons.md`.

use eframe::egui;
use egui::{Align, Color32, Key, Layout, Modifiers, RichText};

use crate::t;

/// Space between a dialog's edge and its contents.
const PADDING: f32 = 20.0;

/// Behind a destructive button's white text. Darker than the theme's error colour, which is too
/// bright for white text to read well on.
const DANGER_FILL: Color32 = Color32::from_rgb(0xC4, 0x2B, 0x1C);

/// What's at stake if the user confirms, which sets how the affirmative button looks.
#[derive(Clone, Copy)]
pub enum Stakes {
    /// Safe, and what the user most likely wants next: emphasised.
    Safe,
    /// Destroys something: red.
    Dangerous,
}

/// Which button the user clicked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Affirmative,
    Cancel,
}

/// A modal dialog `width` points wide. `content` adds the dialog's body, then its buttons with
/// [`buttons`]. Returns the button clicked, with Cancel for Escape or a click outside.
pub fn show(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    content: impl FnOnce(&mut egui::Ui) -> Option<Choice>,
) -> Option<Choice> {
    let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(PADDING);
    let modal = egui::Modal::new(egui::Id::unique(id))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            content(ui)
        });
    match modal.inner {
        Some(choice) => Some(choice),
        None if modal.should_close() => Some(Choice::Cancel),
        None => None,
    }
}

/// Cancel and the affirmative button, bottom right with the affirmative one rightmost. It's
/// disabled unless `enabled`. Enter clicks it, even when it's dangerous, as on macOS, unless
/// something has the keyboard focus: then Enter is that widget's, as usual.
pub fn buttons(
    ui: &mut egui::Ui,
    affirmative: &str,
    stakes: Stakes,
    enabled: bool,
) -> Option<Choice> {
    let unfocused = ui.memory(|m| m.focused().is_none());
    let entered =
        enabled && unfocused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    ui.add_space(PADDING / 2.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let visuals = ui.visuals();
        let (fill, text) = match stakes {
            Stakes::Safe => (visuals.hyperlink_color, Color32::WHITE),
            Stakes::Dangerous => (DANGER_FILL, Color32::WHITE),
        };
        let button = egui::Button::new(RichText::new(affirmative).color(text)).fill(fill);
        // Right to left, so the first added is rightmost.
        let affirmative = ui.add_enabled(enabled, button).clicked() || entered;
        let cancel = ui.button(t!("dialog-cancel")).clicked();
        match (affirmative, cancel) {
            (true, _) => Some(Choice::Affirmative),
            (false, true) => Some(Choice::Cancel),
            (false, false) => None,
        }
    })
    .inner
}
