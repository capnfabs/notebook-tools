# Outline detection: hard photos

Brightness-based detection fails on `spread-wood-2` (shadowed paper as bright as the wood beside it) and `page-grey-table` (light table); both fall back to lighting-only. Run `cargo test -p notebook_core -- --ignored`. Ideas: refine sides to the nearest strong edge, or ask Claude (see [spread detection](spread-detection.md)).
