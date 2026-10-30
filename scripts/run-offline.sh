#!/usr/bin/env bash
# Runs the desktop app without Claude credentials, so only cached transcripts load and nothing is
# sent, with debug logs. Override the logging with RUST_LOG; extra arguments go to `cargo run`.
set -euo pipefail
cd "$(dirname "$0")/.."

unset ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN
RUST_LOG="${RUST_LOG:-notebook_app=debug,eval=info}" exec cargo run -p notebook_app "$@"
