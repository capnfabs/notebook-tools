# Dev Workflows

## Build

Primary development happens on a mac, but we're also running tests on a Linux container with an eye to eventual compatibility.

On macOS:
- Ensure you've got Rust installed
- You'll also need [`apple/container`](https://github.com/apple/container/releases) if you wanna run the Linux tests (virtualized).
- Building the `.app` (see _Release builds_, below) also needs Xcode just to make the icon (lol).

On Linux:
- Ensure you've got Rust installed
- Install libheif (1.19+) and pkg-config, e.g. `apt install libheif-dev pkg-config`.

## Run locally

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
# lo-fi CI: check formatting, clippy and tests on both platforms
# (Linux testing needs apple/container, you might need to run `container system start`)
scripts/check.sh

# macOS only, quicker while iterating
scripts/check.sh --mac-only

# update golden tests
UPDATE_GOLDEN=1 cargo test -p notebook_core --test golden

# there's also some 'advanced case' photos corner detection tests that are
# ignored because they currently fail
cargo test -p notebook_core --test golden -- --ignored

# snapshot (insta) changes: review and accept
cargo insta review

# send a real photo to Claude (costs money!!).
# There's a default photo that's used (spread-wood-1.jpg) with Opus,
# OCR_LIVE_PHOTO and OCR_LIVE_MODEL=sonnet pick different photos / models
scripts/with-claude-token.sh cargo test -p notebook_core --test claude_transcribe -- --ignored live
```

## Tools

```sh
# clear caches
scripts/clear-cache.sh
```


## Release builds

Currently, Release builds only support macOS 11 or later, on Apple silicon.

Run `scripts/bundle-macos.sh`; needs Xcode, see Build (above).
The app and DMG land in `target/bundle/`.

### self-signing
You can run `scripts/bundle-macos.sh --identity NAME` to sign with your own cert, so you don't need to re-grant keychain access after each install.

To make one: Keychain Access → Certificate Assistant → Create a Certificate…, name it (e.g.
`Notebook Tools Local`), Identity Type *Self-Signed Root*, Certificate Type *Code Signing*. It
doesn't need to be marked as trusted. Check it's there with `security find-identity -p codesigning`.
