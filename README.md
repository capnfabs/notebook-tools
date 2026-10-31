Notebook-tools is a tool for OCR-ing a handwritten notebook.

## Build

Needs Rust and libheif (1.19+) + pkg-config:

```sh
brew install libheif pkg-config
```

The app is the `notebook_app` package; its binary is called `notebook-tools`.

## Run

```sh
# run w/ logs (debug + eval) and a fresh anthropic token from `ant`
scripts/run.sh

# the same without llm transcription (and therefore without spending money).
# Still loads saved transcripts from cache.
scripts/run.sh --offline
```

## Test

```sh
# lo-fi CI: formatting, clippy and every test
scripts/check.sh

# update golden tests
UPDATE_GOLDEN=1 cargo test -p notebook_core --test golden

# photos corner detection can't handle yet
cargo test -p notebook_core --test golden -- --ignored

# snapshot (insta) changes: review and accept
cargo insta review

# send a real photo to Claude; costs money. OCR_LIVE_PHOTO and OCR_LIVE_MODEL=sonnet pick another
scripts/with-claude-token.sh cargo test -p notebook_core --test claude_transcribe -- --ignored live
```

## Tools

```sh
# clear caches
scripts/clear-cache.sh
```
