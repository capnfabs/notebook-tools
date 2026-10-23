Notebook-tools is a tool for OCR-ing a handwritten notebook.

## Setup instructions

```sh
# install wasm toolchain
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
```

## Run

```sh
# run w/ logs and anthropic auth token
scripts/run.sh

# more manual steps below:

# run desktop app
cargo run -p notebook_app

# with a fresh anthropic token from `ant`, required for transcription
scripts/with-claude-token.sh cargo run -p notebook_app

# with debug logs including timing info
RUST_LOG=notebook_app=debug cargo run -p notebook_app

# serve on http://127.0.0.1:8080
(cd crates/app && trunk serve)

# lo-fi CI
scripts/check.sh
```

## Tools

```sh
# bbox spike: send photos to Claude at each effort level and image size, then draw the returned
# boxes over each photo into target/tmp/bbox-spike/. Costs money. `--only <variant>` (e.g.
# high-capped) sends just one variant; runs that already got an answer are skipped.
scripts/with-claude-token.sh cargo run -p notebook_core --example bbox_spike -- run sample_images/*.jpeg

# redraw the boxes from the recorded fixtures, no API calls
cargo run -p notebook_core --example bbox_spike -- \
  draw crates/core/tests/fixtures/ocr/bbox-spike --photos crates/core/tests/fixtures \
  --out target/tmp/bbox-spike-fixtures
```
