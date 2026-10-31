# Keyboard focus audit

Keyboard focus is a bit of a mess right now. As examples:
- The thumbnail view takes focus, but doesn't show it (probably shouldn't take focus at all?)
- pressing arrow keys when in keyboard focus mode doesn't allow navigation. I don't _think_ iOS uses arrow keys for keyboard focus control generally and maybe we shouldn't here.
- Corner placement doesn't work with keyboard, and the corners don't show focus when selected
- Tabbing through the transcript view is _awesome_ but:
     - it doesn't scroll the page as the focus changes, and probably should
     - selecting a correction doesn't focus the correction popover and should. Pressing esc from a correction dialog should move the focus back to where you were beforehand.
