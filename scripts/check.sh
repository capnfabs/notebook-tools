#!/usr/bin/env bash
# Everything CI would run: formatting, lints and tests.
set -euo pipefail
cd "$(dirname "$0")/.."

scripts/check-images.sh --installed
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/test-check-images.sh
