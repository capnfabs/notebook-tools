//! Modal dialogs laid out the macOS way: buttons at the bottom right, the affirmative one
//! rightmost, and a button that discards work, like Don't Save, at the bottom left. Enter picks the
//! affirmative button, unless a widget has the keyboard focus, ⌘⌫ the discarding one, and Escape
//! cancels.
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
    /// Go ahead without doing what the dialog offers, as Don't Save does.
    Discard,
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
    buttons_saying(ui, affirmative, &t!("dialog-cancel"), stakes, enabled)
}

/// [`buttons`], with Cancel saying `cancel` instead, such as Don't Delete.
pub fn buttons_saying(
    ui: &mut egui::Ui,
    affirmative: &str,
    cancel: &str,
    stakes: Stakes,
    enabled: bool,
) -> Option<Choice> {
    ui.add_space(PADDING / 2.0);
    right_buttons(ui, affirmative, cancel, stakes, enabled)
}

/// A button saying `text` that looks as `stakes` say: emphasised if safe, red if it destroys
/// something.
pub fn button(ui: &egui::Ui, text: &str, stakes: Stakes) -> egui::Button<'static> {
    let fill = match stakes {
        Stakes::Safe => ui.visuals().hyperlink_color,
        Stakes::Dangerous => DANGER_FILL,
    };
    egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(fill)
}

/// [`buttons`], plus `discard` at the bottom left, which ⌘⌫ clicks too.
pub fn buttons_with_discard(
    ui: &mut egui::Ui,
    affirmative: &str,
    discard: &str,
    stakes: Stakes,
) -> Option<Choice> {
    let pressed = ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Backspace));
    ui.add_space(PADDING / 2.0);
    ui.horizontal(|ui| {
        let discarded = ui.button(discard).clicked() || pressed;
        let choice = right_buttons(ui, affirmative, &t!("dialog-cancel"), stakes, true);
        if discarded {
            Some(Choice::Discard)
        } else {
            choice
        }
    })
    .inner
}

/// Cancel and the affirmative button, right to left.
fn right_buttons(
    ui: &mut egui::Ui,
    affirmative: &str,
    cancel: &str,
    stakes: Stakes,
    enabled: bool,
) -> Option<Choice> {
    let unfocused = ui.memory(|m| m.focused().is_none());
    let entered =
        enabled && unfocused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let button = button(ui, affirmative, stakes);
        // Right to left, so the first added is rightmost.
        let affirmative = ui.add_enabled(enabled, button).clicked() || entered;
        let cancel = ui.button(cancel).clicked();
        match (affirmative, cancel) {
            (true, _) => Some(Choice::Affirmative),
            (false, true) => Some(Choice::Cancel),
            (false, false) => None,
        }
    })
    .inner
}
