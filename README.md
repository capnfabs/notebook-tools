Notebook-tools is a tool for OCR-ing a handwritten notebook.

## Build

On macOS, needs only Rust: HEIC goes through the system's ImageIO. Elsewhere, also libheif (1.19+)
and pkg-config, e.g. `apt install libheif-dev pkg-config`.

The app is the `notebook_app` package; its binary is called `notebook-tools`.

## Run

```sh
# run w/ logs (debug + eval).
scripts/run.sh

# the same without llm transcription (and therefore without spending money).
# Still loads saved transcripts from cache.
scripts/run.sh --offline
```

## Test

These scripts are a bit cursed / have a bit of tech debt; they assume that they're always going to be running on OSX.

```sh
# lo-fi CI: formatting, clippy and every test, on macOS then Linux
# (Linux needs apple/container: once per boot, `container system start`)
scripts/check.sh

# macOS only, quicker while iterating
scripts/check.sh --mac-only

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
