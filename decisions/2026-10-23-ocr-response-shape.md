# OCR response shape: lines + uncertain words, pixel boxes

Claude returns `lines: [{text_md, bbox | null}]` and `uncertain: [{line, text, bbox, alternatives}]`, boxes as `[x0, y0, x1, y1]` in pixels of the image sent. The client joins lines with `\n` to get the page's markdown, and finds each uncertain word's offset by searching for `text` in its line.

Why: the text appears once, so nothing can disagree; the API docs say coordinates from Opus 4.7+ map 1:1 to pixels of the image sent; models can't count characters, so no offsets in the response; line boxes feed the line-level PDF text layer. claude.ai once told the human it couldn't do pixel coordinates. Likely causes: claude.ai resizes uploads, or the model was wrong about its own abilities. Whether pixels beat fractions is untested; the bbox spike will show. Reason: agent recommendation, discussed and accepted.
