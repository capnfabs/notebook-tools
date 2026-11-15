#!/usr/bin/env bash
# Everything CI would run: formatting, lints and tests on macOS, then lints and tests on Linux
# (`scripts/check-linux.sh`). Runs every step, even after one fails, then ends with one line saying
# which failed. --mac-only skips Linux, for while you're iterating.
set -uo pipefail
cd "$(dirname "$0")/.."

linux=true
for arg in "$@"; do
    case $arg in
    --mac-only) linux=false ;;
    *)
        echo "usage: $0 [--mac-only]" >&2
        exit 2
        ;;
    esac
done

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
# The libheif decoder only compiles off macOS: `decisions/2026-11-15-local-linux-ci.md`.
if $linux; then
    step "linux" scripts/check-linux.sh
fi

if [ ${#failed[@]} -gt 0 ]; then
    printf 'check: FAILED: %s\n' "$(IFS=,; echo "${failed[*]}" | sed 's/,/, /g')" >&2
    exit 1
fi
echo "check: all passed"
