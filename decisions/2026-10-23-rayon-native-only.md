# Rayon on native only

Image code runs multithreaded on desktop via rayon (imageproc's `rayon` feature plus our own row loops), enabled through a `cfg(not(target_arch = "wasm32"))` dependency table in `crates/core/Cargo.toml`. The web build stays single-threaded (no threads without extra setup); `scripts/check.sh` fails if rayon reaches the wasm dependency tree.

Reason: human taste.
