#!/usr/bin/env bash
# Runs the desktop app with transcription on (a fresh token from `ant`) and debug logs, including
# timing info. Override the logging with RUST_LOG; extra arguments go to `cargo run`.
set -euo pipefail
cd "$(dirname "$0")/.."

RUST_LOG="${RUST_LOG:-notebook_app=debug}" \
  exec scripts/with-claude-token.sh cargo run -p notebook_app "$@"
