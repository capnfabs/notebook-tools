# Markdown conversion improvements

- A line holding only a dash becomes `---`. Probably done locally rather than via the prompt.
- Underlined text alone on a line is a heading: convert it to `#`-style, picking the depth from context. The user can edit it.
- Underlined spans: encode somehow, e.g. as italics or bold, since handwriting has neither.

Probably needs a prompt change, so bump the prompt version (`transcription-cache.md`).
