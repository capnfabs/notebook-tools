# Manual page detection

Automated page detection doesn't always work and _won't_ always work.
We're effectively drawing a... four-sided shape (trapezoid?) and then doing a simple perspective transform at the moment.

We _should_ instead support clicking in four points of the image to place the corners. The corners should have lines pointing to the _other_ corners (which implies that we can figure out the ordering of the points, the rule should basically be that no lines cross if you connect each point to two others). When you mouse over them, the cursor should change to show that you can drag them.

You draw the four-sided-shape / annotate the corners on the _original_ view. Probably need to change the 'cleaned' mode to a split pane. On the left: the original, with the bounds drawn. Importantly, the bounds are interactive; you can move the corners. On the right: the 'cleaned' view, which includes the crop.

User-placed corners change the crop sent to Claude per page, but let's _not_ do cache-busting, for now, that only happens if you force retranscription.

Also: remove the debug view from the 'original' Tab and the menu. We'll eventually reincorporate the image params that the debug display shows back into the UI (add a task for this; when we do that we want to be able to live-edit them too, so it's going to be a bit of work).

There's currently a couple problems that need to be resolved before marking this fixed:
1. the page outline ‘flickers’ when a node is released
2. it’s possible to make the app crash on certain shapes, we maybe need to constrain the angles somehow.
