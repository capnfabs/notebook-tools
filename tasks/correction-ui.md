# Correction UI improvements

In the Transcribed view (`crates/app/src/ui/transcript_view.rs`):

- (a) Struck-through text (`~~…~~`): an option to delete it entirely (sometimes it's just a typo).
- (b) Hovering a box whose word was edited out of its line, or can't be found at all, explains why.
- (c) A "Reset" button that drops manual edits and goes back to the original transcription.
- (d) Make double-click-to-edit discoverable, e.g. a tooltip on single click.
- (e) Hovering a text line highlights its line box on the image.
- (f) Scroll the page and the text in sync while correcting.
