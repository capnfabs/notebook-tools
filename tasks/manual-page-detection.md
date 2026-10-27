# Manual page detection

Page detection doesn't always work and _won't_ always work.
We're effectively drawing a... four-sided shape (trapezoid?) and then doing a simple perspective transform at the moment.

We _should_ instead support clicking in four points of the image to place the corners. The corners should have lines pointing to the _other_ corners (which implies that we can figure out the ordering of the points, the rule should basically be that no lines cross if you connect each point to two others). When you mouse over them, the cursor should change to show that you can drag them.

You draw the four-sided-shape / annotate the corners on the _original_ view. Probably need to change the modes from 'original' / 'cleaned' / 'transcribed' to be based on _task_ rather than state. That is, change to modes based on tasks:
- 'Clean Image': shows separate panes for original image and the cleaned image.
- 'Transcribe': as-is today.

Have an 'auto' mode but with corrections.
