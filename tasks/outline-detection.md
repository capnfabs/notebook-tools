# Corner detection: hard photos

Brightness-based detection fails on `page-grey-table` (light table) and falls back to lighting-only. Run `cargo test -p notebook_core --test golden -- --ignored` (plain `--ignored` also runs the paid live test). Ideas: refine sides to the nearest strong edge, or ask Claude (see [spread detection](spread-detection.md)).
