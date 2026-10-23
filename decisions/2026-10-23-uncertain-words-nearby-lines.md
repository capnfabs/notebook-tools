# Look for uncertain words in nearby lines too

In the first live response, 2 of 5 uncertain words named a line one off from the line holding their text (both directions), though their boxes were on the right line. So an uncertain word's text is looked for in its claimed line, then up to 2 lines either side, nearest first. Using the box to pick the line instead would rely on box accuracy, which the bbox spike hasn't checked yet.

Reason: agent recommendation, prompted by the live test.
