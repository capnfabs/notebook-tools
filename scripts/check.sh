#!/usr/bin/env bash
# Everything CI would run: formatting, lints (native + wasm), tests, and the web build.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p notebook_app --target wasm32-unknown-unknown -- -D warnings
# Browsers have no threads for rayon; it's native-only (decisions/2026-10-23-rayon-native-only.md).
# Transcription is native-only too, so the blocking HTTP client stays out of the web build.
for crate in rayon reqwest; do
  if cargo tree -p notebook_app --target wasm32-unknown-unknown -e normal -i "$crate" 2>/dev/null | grep -q "$crate"; then
    echo "$crate is in the wasm build" >&2
    exit 1
  fi
done
cargo test --workspace
(cd crates/app && trunk build)
