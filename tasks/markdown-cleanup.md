# Markdown conversion improvements

- A line holding only a dash becomes `---`. Probably done locally rather than via the prompt.
- Underlined text alone on a line is a heading: convert it to `#`-style, picking the depth from context. The user can edit it.
- Underlined spans: encode somehow, e.g. as italics or bold, since handwriting has neither.
- Decide whether to keep task lists (`- [ ]`/`- [x]`): the prompt asks for checkboxes, and the PDF text layer drops their markers.
- Sometimes, 'alternatives' contain an alternate identical to the source word -> we should tell the prompt to not do this, but to keep highlighting things that we're not sure about. It's ok if there are zero alternatives, that means "get a person to look at this"... though I'm reluctant to fiddle with this too much because it could make the results worse.

Probably needs a prompt change, so bump the prompt version (`transcription-cache.md`).
