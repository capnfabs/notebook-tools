//! Keyboard focus across the app: Tab and the arrow keys move it, as egui has them. A widget that
//! wants the arrows for itself (a corner handle, the correction popup) keeps them with
//! [`keep_arrows`]; text fields keep them already. One that keeps the focus in itself (the
//! correction popup) takes Tab with [`keep_tab`].
//!
//! See `decisions/2026-10-31-keyboard-focus.md` and `decisions/2026-10-31-arrow-keys-move-focus.md`.

use eframe::egui;
use egui::{Event, FocusDirection, Key};

/// The bare arrow keys, which egui moves the focus with.
const ARROWS: [Key; 4] = [
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
];

/// Stops this frame's bare arrow presses from moving the keyboard focus, for a widget that uses
/// them itself. Call any time before the frame ends.
pub fn keep_arrows(ctx: &egui::Context) {
    let (mut arrow, mut tab) = (false, false);
    ctx.input(|i| {
        for event in &i.events {
            if let Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                arrow |= ARROWS.contains(key) && !modifiers.any();
                tab |= *key == Key::Tab;
            }
        }
    });
    if arrow && !tab {
        ctx.memory_mut(|m| m.move_focus(FocusDirection::None));
    }
}

/// Stops this frame's Tab or Shift-Tab from moving the keyboard focus, for a widget that moves it
/// itself. Returns how many places it would have moved: 1 for Tab, -1 for Shift-Tab, else 0. Call
/// before the focused widget is added, which is when egui moves it.
pub fn keep_tab(ctx: &egui::Context) -> isize {
    let step = ctx.input(|i| {
        let mut step = 0;
        for event in &i.events {
            if let Event::Key {
                key: Key::Tab,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                if modifiers.is_none() {
                    step = 1;
                } else if modifiers.shift_only() {
                    step = -1;
                }
            }
        }
        step
    });
    if step != 0 {
        ctx.memory_mut(|m| m.move_focus(FocusDirection::None));
    }
    step
}
