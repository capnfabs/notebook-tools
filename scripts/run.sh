#!/usr/bin/env bash
# Runs the desktop app with debug logs, including timing info and eval loglines, and
# `--no-keychain`, so macOS doesn't ask for keychain access with every new build: credentials come
# from the environment, else `ant`'s login. Arguments go to the app, so `--offline` runs it without
# any, sending nothing. Override the logging with RUST_LOG.
set -euo pipefail
cd "$(dirname "$0")/.."

export RUST_LOG="${RUST_LOG:-notebook_app=debug,notebook_tools=debug,eval=info}"
exec cargo run -p notebook_app -- --no-keychain "$@"
