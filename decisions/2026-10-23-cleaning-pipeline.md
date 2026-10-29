# Cleaning pipeline

`notebook_core::clean`: find the page (or whole spread) as the largest region of "paper" (bright, colourless, judged against local brightness so shadows don't break it), fit a quad that encloses it, warp it upright, then divide out a low-res paper/lighting estimate and stretch levels. Colour is kept. Output is capped at 2576px (Claude's image limit). If no trustworthy outline is found (too small, runs along the photo's edge), skip straightening and only fix lighting. Tunables live in `CleanParams`. It now runs once per page on load (`2026-10-28-pages-cleaned-on-load.md`), the user can place the corners (`2026-10-29-manual-corners.md`), and the debug overlay is gone.

Reason: human choices (full pass, keep colour, on demand, debug overlay, read-only params); unquestioned agent recommendation for the algorithm.
