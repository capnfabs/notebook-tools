# Cache key ignores CleanParams, for now

The image sent to Claude is straightened and cropped by `crop_to_page` with `CleanParams`, and Claude's boxes are in that image's pixels, but the cache key is only the photo's hash, `CACHE_VERSION` and the model. A transcript cached under other params would have boxes that don't line up with the page shown. We're not adding the params to the key: they're always `CleanParams::default()`, so nothing can change the crop between caching and loading, and the crop doesn't substantially change what Claude reads.

Revisit with `tasks/done/manual-page-detection.md`: once the user can place the corners, the crop depends on per-page input and could change the transcript meaningfully, so the key (or the entry) needs to capture it.

Reason: Human Taste.

Superseded by `2026-10-29-manual-corners.md`.
