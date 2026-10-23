# Cleaning pipeline

`notebook_core::clean`: find the page (or whole spread) as the largest region of "paper" (bright, colourless, judged against local brightness so shadows don't break it), fit a quad that encloses it, warp it upright, then divide out a low-res paper/lighting estimate and stretch levels. Colour is kept. Output is capped at 2576px (Claude's image limit). If no trustworthy outline is found (too small, runs along the photo's edge), skip straightening and only fix lighting. Runs on demand for the selected page; tunables live in `CleanParams`, shown read-only in the debug overlay.

Reason: human choices (full pass, keep colour, on demand, debug overlay, read-only params); unquestioned agent recommendation for the algorithm.
