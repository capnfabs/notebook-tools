# Optimize image code in dev builds

Dependencies and `notebook_core` build at `opt-level = 2` in dev. Unoptimized, a 24 MP iPhone JPEG took ~1.5 s to decode and thumbnail (~3 s for deps alone); now ~225 ms. Core is included because generic image code (e.g. `imageops::thumbnail`) is compiled in the crate that calls it. If debugging core gets painful, move the pixel code into its own crate with concrete (non-generic) signatures and optimize only that.

Reason: human taste, after measuring.
