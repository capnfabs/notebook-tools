# Decide: one page vs two-page spread detection

(maybe drop this? I'm pretty happy with what we've got).

Options: local heuristic (e.g. fold/gutter line via `imageproc`) vs asking Claude (page count + corners). User finds the Claude approach elegant but wants graceful degradation and to avoid leaning too hard on the remote service. Log the outcome in `decisions/`.

Then handle reading order on spreads, end to end. Today nothing does: the prompt only says "top to bottom", so Claude sometimes reads the left page then the right, and sometimes reads across both. The markdown and the PDF text layer follow `lines` order. A line box spanning the gutter also yields one PDF text run across both pages. Likely fix: transcribe the left page and then the right (prompt, or two images), and consider splitting a spread into two PDF pages. Check with the `spread-wood-*` fixtures.

Not seen in the cache yet (2026-10-30): all 13 cached spreads were read left page then right, no line across the gutter.
