#!/usr/bin/env bash
# Everything CI would run: formatting, lints (native + wasm), tests, and the web build.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p notebook_app --target wasm32-unknown-unknown -- -D warnings
# Browsers have no threads for rayon; it's native-only (decisions/2026-10-23-rayon-native-only.md).
if cargo tree -p notebook_app --target wasm32-unknown-unknown -e normal -i rayon 2>/dev/null | grep -q rayon; then
  echo "rayon is in the wasm build" >&2
  exit 1
fi
cargo test --workspace
(cd crates/app && trunk build)
