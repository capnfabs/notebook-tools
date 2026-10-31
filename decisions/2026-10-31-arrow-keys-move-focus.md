# Arrow keys move the keyboard focus; page commands take ⌘

The arrow keys move the keyboard focus, as egui has them, because after trying the Tab-only version that felt more natural in the transcript. Previous/Next Page move to ⌘←/→, and Move Left/Right to ⇧⌘←/→. A text field already keeps its arrows (egui's `TextEdit` claims all four). Elsewhere, a widget that needs the arrows keeps them with `keep_arrows` (`crates/app/src/ui/focus.rs`). A focused corner handle keeps them to nudge itself. The correction popup keeps them to move between its own choices, so focus never leaves it for the transcript behind (it keeps Tab too, with `keep_tab`). With nothing focused, a bare arrow does nothing. Reason: Human Taste (arrows for focus, the new shortcuts); agent recommendation accepted for text fields, the popup and corners.

This replaces the "bare arrows are the page commands" part of `2026-10-31-keyboard-focus.md`.
