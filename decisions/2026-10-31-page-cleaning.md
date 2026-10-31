# Page cleaning with `imageproc`

`notebook_core::clean` finds the page (or whole spread) as the largest region of "paper" (bright, colourless, judged against local brightness so shadows don't break it), fits four corners that enclose it, warps it upright, then divides out a low-res paper and lighting estimate and stretches levels. Colour is kept. Output is capped at 2576px, Claude's image limit. If no trustworthy corners are found (too small a region, or it runs along the photo's edge), it skips straightening and only fixes lighting. Tunables live in `CleanParams`. It runs once per page on load (`2026-10-28-pages-cleaned-on-load.md`), and the user can place the corners themselves (`2026-10-29-manual-corners.md`).

It's built on pure-Rust `imageproc`; the `opencv` crate, a heavy C++ dependency, is the escape hatch only if `imageproc` proves insufficient. Reason: Human Taste (a full pass, keeping colour); unquestioned agent recommendation (the algorithm, `imageproc`).
