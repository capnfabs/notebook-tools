#!/usr/bin/env bash
# Runs the desktop app with transcription on (a fresh token from `ant`) and debug logs, including
# timing info. Override the logging with RUST_LOG; extra arguments go to `cargo run`.
set -euo pipefail
cd "$(dirname "$0")/.."

# Fetched each run because access tokens expire.
if ! token=$(ant auth print-credentials --access-token); then
  echo "Couldn't get a token from ant; try \`ant auth login\`." >&2
  exit 1
fi

ANTHROPIC_AUTH_TOKEN="$token" RUST_LOG="${RUST_LOG:-notebook_app=debug}" \
  exec cargo run -p notebook_app "$@"
