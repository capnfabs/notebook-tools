# Remove the corner backfill

Once old cache entries have all been loaded once, delete `backfill_corners` in `crates/core/src/cache.rs` and make `Entry.corners` required (entries without it then fail to parse and are fetched again).

Also: make it so future cache entries are written as _formatted_ JSON please.
