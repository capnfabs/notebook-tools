# Edit menu

Add Undo/Redo/Cut/Copy/Paste/Select All to the menu bar (`command.rs` `MENUS`) for transcript editing. Check that muda's predefined items don't break egui's own copy/paste, since NSMenu would take ⌘C/⌘V first.
