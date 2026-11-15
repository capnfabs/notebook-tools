#!/usr/bin/env bash
# Everything CI would run: formatting, lints and tests. Runs every step, even after one fails,
# then ends with one line saying which failed.
set -uo pipefail
cd "$(dirname "$0")/.."

failed=()

# Runs a step called $1: the command after it.
step() {
    local name=$1
    shift
    "$@" || failed+=("$name")
}

step "image hook installed" scripts/check-images.sh --installed
# Every committed image, approved and without metadata: `decisions/2026-11-15-check-all-images.md`.
step "committed images" scripts/check-images.sh --all
step "cargo fmt" cargo fmt --all --check
step "cargo clippy" cargo clippy --workspace --all-targets -- -D warnings
step "cargo test" cargo test --workspace
step "check-images tests" scripts/test-check-images.sh

if [ ${#failed[@]} -gt 0 ]; then
    printf 'check: FAILED: %s\n' "$(IFS=,; echo "${failed[*]}" | sed 's/,/, /g')" >&2
    exit 1
fi
echo "check: all passed"
