#!/usr/bin/env bash
# Everything CI would run: formatting, lints (native + wasm), tests, and the web build.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p notebook_app --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace
(cd crates/app && trunk build)
