# Find uncertain words' line by box

Claude's line index can be ~15 lines off (`decisions/2026-10-23-bbox-accuracy.md`). In `Transcript::uncertain_spans`, pick the line whose box holds the word's box centre, then fall back to the index. Regression data was the bbox spike's responses, since removed (`tasks/done/audit-and-strip.md`).
