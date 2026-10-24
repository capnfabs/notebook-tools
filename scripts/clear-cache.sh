#!/usr/bin/env bash
# Deletes every cached transcription, so pages are sent to Claude again. The path matches
# `Cache::default_dir` in crates/core/src/cache.rs; keep the two in step.
set -euo pipefail

if [[ "$(uname)" == Darwin ]]; then
  cache_root="$HOME/Library/Caches"
else
  cache_root="${XDG_CACHE_HOME:-$HOME/.cache}"
fi
dir="$cache_root/notebook-tools/transcriptions"

if [[ -d "$dir" ]]; then
  count=$(find "$dir" -type f | wc -l | tr -d ' ')
  rm -rf "$dir"
  echo "Removed $count cached transcriptions from $dir"
else
  echo "No cache at $dir"
fi
