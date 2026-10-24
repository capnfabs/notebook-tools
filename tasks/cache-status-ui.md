# Cache status + force retranscription

On a page served from the cache (`Transcript::from_cache`), show a status line: when the entry was stored and the model that transcribed it. Add a "Force retranscription" button that skips the cache and overwrites the entry. Builds on `transcription-cache.md`.

This UI should be in the _leftmost_ pane of the two-pane transcription view, under the image.

While we're here, let's also add a flexible slider between the two panes of the transcription view, and make it 'clever' when the window resizes:
- when _shrinking_ the window, space is first taken from the transcription view, until taking any more would cause it to start wrapping. we'd need a bit of extra padding for modifications etc (maybe another 20%).
- when _expanding_ the window, similarly prefer to add space to the image once beyond that threshold.
- i don't have any suggestions of what to do if the text is already wrapping; feel free to suggest something.
