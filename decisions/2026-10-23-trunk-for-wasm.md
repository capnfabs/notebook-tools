# Trunk for the WASM build

The web build uses Trunk (`crates/app/index.html` + `Trunk.toml`) instead of a hand-rolled `wasm-bindgen-cli` script. It's the standard eframe path, gives `trunk serve` live reload, and keeps the `wasm-bindgen` CLI version in sync with the crate. Automated WASM coverage is a build check only (clippy + `trunk build` in `scripts/check.sh`); running in a browser is manual for now.

Reason: unquestioned agent recommendation.
