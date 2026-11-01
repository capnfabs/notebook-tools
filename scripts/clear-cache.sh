#!/usr/bin/env bash
# Deletes every cached transcription, so pages are sent to Claude again. The cache lives where
# `Cache::default_dir` in crates/core/src/cache.rs says; `notebook-tools --clear-caches-only` does
# the clearing.
set -euo pipefail
cd "$(dirname "$0")/.."

exec cargo run -q -p notebook_app --bin notebook-tools -- --clear-caches-only
