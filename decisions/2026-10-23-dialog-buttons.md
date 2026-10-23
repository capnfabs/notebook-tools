# Dialogs follow macOS conventions

Dialog buttons sit at the bottom right with the affirmative action rightmost and Cancel to its left. A safe affirmative action that the user most likely wants next is emphasised (accent fill, white text). A destructive one is red. Dialogs have generous padding and a fixed width that fits their content. Build dialogs with `crates/app/src/dialog.rs` (`dialog::show` plus `dialog::buttons`) rather than raw `egui::Modal`, so they all stay consistent. `dialogs_put_the_affirmative_button_last` in `crates/app/tests/viewer.rs` checks the order.

Reason: Human Taste.
