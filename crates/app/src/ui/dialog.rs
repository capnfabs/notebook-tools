//! Modal dialogs laid out the macOS way: buttons at the bottom right, the affirmative one
//! rightmost.

use eframe::egui;
use egui::{Align, Color32, Layout, RichText};

/// Space between a dialog's edge and its contents.
const PADDING: f32 = 20.0;

/// Behind a destructive button's white text. Darker than the theme's error colour, which is too
/// bright for white text to read well on.
const DANGER_FILL: Color32 = Color32::from_rgb(0xC4, 0x2B, 0x1C);

/// What a dialog's affirmative button does.
#[derive(Clone, Copy)]
pub enum Action {
    /// Safe, and what the user most likely wants next: emphasised.
    Default,
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
/// disabled unless `enabled`.
pub fn buttons(
    ui: &mut egui::Ui,
    affirmative: &str,
    action: Action,
    enabled: bool,
) -> Option<Choice> {
    ui.add_space(PADDING / 2.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let visuals = ui.visuals();
        let (fill, text) = match action {
            Action::Default => (visuals.hyperlink_color, Color32::WHITE),
            Action::Dangerous => (DANGER_FILL, Color32::WHITE),
        };
        let button = egui::Button::new(RichText::new(affirmative).color(text)).fill(fill);
        // Right to left, so the first added is rightmost.
        let affirmative = ui.add_enabled(enabled, button).clicked();
        let cancel = ui.button("Cancel").clicked();
        match (affirmative, cancel) {
            (true, _) => Some(Choice::Affirmative),
            (false, true) => Some(Choice::Cancel),
            (false, false) => None,
        }
    })
    .inner
}
