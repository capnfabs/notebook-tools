# Keep page corners across restarts

Corners the user places live in memory only (`decisions/2026-10-29-manual-corners.md`). After a restart the page goes back to detected corners, but a transcript forced with the user's corners stays cached, so its boxes no longer line up and nothing says "Bounds changed". Likely fix: store the corners by photo hash next to the cache, or in the cache entry.
