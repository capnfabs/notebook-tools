#!/usr/bin/env bash
# Deletes every cached transcription, so pages are sent to Claude again. The cache lives where
# `Cache::default_dir` in crates/core/src/cache.rs says.
set -euo pipefail
cd "$(dirname "$0")/.."

exec cargo run -q -p notebook_core --bin clear-cache
