#!/usr/bin/env bash
# Runs the desktop app with debug logs, including timing info and eval loglines, and transcription
# on (a fresh token from `ant`). With --offline, runs without Claude credentials in the
# environment instead, so only cached transcripts load and nothing is sent, unless an API key is
# saved in the keychain. Override the logging with RUST_LOG; other arguments go to `cargo run`.
set -euo pipefail
cd "$(dirname "$0")/.."

export RUST_LOG="${RUST_LOG:-notebook_app=debug,eval=info}"
if [[ "${1:-}" == --offline ]]; then
  shift
  unset ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN
  exec cargo run -p notebook_app "$@"
fi
exec scripts/with-claude-token.sh cargo run -p notebook_app "$@"
