# Finding uncertain words: by box first, then nearby lines

Claude names the line each uncertain word is on, but that index can be off by up to ~15 lines; its boxes rarely are. So a word's text is looked for first in the lines whose box holds the centre of the word's box (vertically nearest first), then in its claimed line and up to 2 lines either side, nearest first (`LINE_SEARCH_RADIUS`). The first match in a line wins. On the bbox-spike fixtures that finds all 123 words, against 99 by line alone; a test keeps it so.

Reason: agent recommendation, prompted by the first live test (2 of 5 words one line off) and the bbox spike (`2026-10-23-bbox-accuracy.md`).
