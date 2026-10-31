# Keyboard focus audit

Some things to fix:
- The thumbnail view takes focus, but doesn't show it (probably shouldn't take focus at all?)
- pressing arrow keys when in keyboard focus mode doesn't allow navigation between pages. Let's switch off arrow keys for keyboard focus control so that we can continue to use it to move between pages.
- Corner placement doesn't work with keyboard, and the corners don't show focus when selected (if this is going to be super hard, it's ok to punt on this).
- Tabbing through the transcript view is _awesome_ but:
     - it doesn't scroll the page as the focus changes, and probably should
     - selecting a correction doesn't focus the correction popover and should. Pressing esc from a correction dialog should move the focus back to where you were beforehand.
