# Look for uncertain words in nearby lines too

In the first live response, 2 of 5 uncertain words named a line one off from the line holding their text (both directions), though their boxes were on the right line. So an uncertain word's text is looked for in its claimed line, then up to 2 lines either side, nearest first. Since the bbox spike, the line whose box holds the word's box is tried first, and nearby lines are the fallback (`2026-10-23-transcript-corrections.md`).

Reason: agent recommendation, prompted by the live test.
