Notebook-tools is a tool for OCR-ing a handwritten notebook.

## Build

On macOS, needs only Rust: HEIC goes through the system's ImageIO. Elsewhere, also libheif (1.19+)
and pkg-config, e.g. `apt install libheif-dev pkg-config`. Building the `.app` (see Install) also
needs Xcode, for the icon; run `sudo xcodebuild -runFirstLaunch` once after installing it.

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


## Install + First run

macOS 11 or later, on Apple silicon.

1. Do a release build (`scripts/bundle-macos.sh`; needs Xcode, see Build). The app and DMG land in
   `target/bundle/`.
1. Open `Notebook Tools <version>.dmg` and drag Notebook Tools into Applications.
1. Open it.
1. Transcribing needs a Claude API key. The app asks for one the first
   time, and keeps it in your keychain. You can skip this if you've already got the `ant` CLI installed and logged in; we'll try and use that directly.

Open photos (JPEG, PNG, HEIC) by dropping them on the window or the Dock icon, or with Open With
in Finder; double-click a `.notebook` project to open it.

### self-signing
You can run `scripts/bundle-macos.sh --identity NAME` to sign with your own cert, so you don't need to re-grant keychain access after each install.

To make one: Keychain Access → Certificate Assistant → Create a Certificate…, name it (e.g.
`Notebook Tools Local`), Identity Type *Self-Signed Root*, Certificate Type *Code Signing*. It
doesn't need to be marked as trusted. Check it's there with `security find-identity -p codesigning`.
