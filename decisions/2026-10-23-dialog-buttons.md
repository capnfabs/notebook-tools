# Dialogs follow macOS conventions

Dialog buttons sit at the bottom right with the affirmative action rightmost and Cancel to its left. A safe affirmative action that the user most likely wants next is emphasised (accent fill, white text). A destructive one is red. Dialogs have generous padding and a fixed width that fits their content. Build dialogs with `crates/app/src/ui/dialog.rs` (`dialog::show` plus `dialog::buttons`) rather than raw `egui::Modal`, so they all stay consistent. `dialogs_put_the_affirmative_button_last` in `crates/app/tests/viewer/` checks the order. Enter clicks the affirmative button while it's enabled, even a destructive one, unless a widget has the keyboard focus, in which case the focused widget gets Enter as usual. Escape cancels. Neither has a menu item (added 2026-10-29).

Reason: Human Taste.
