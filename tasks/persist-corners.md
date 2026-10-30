# Keep page corners across restarts

Corners the user places live in memory only (`decisions/2026-10-29-manual-corners.md`). After a restart the page goes back to detected corners. A transcript forced with the user's corners stays cached and its boxes follow the detected corners (`decisions/2026-10-30-boxes-follow-corners.md`), but the user has to place theirs again. Likely fix: store the corners by photo hash next to the cache, or in the cache entry.
