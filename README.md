Notebook-tools is a tool for OCR-ing a handwritten notebook.

## Setup instructions

```sh
# install wasm toolchain
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
```

## Run

```sh
# run desktop app
cargo run -p notebook_app
# with debug logs including timing info
RUST_LOG=notebook_app=debug cargo run -p notebook_app

# serve on http://127.0.0.1:8080
(cd crates/app && trunk serve)

# lo-fi CI
scripts/check.sh
```
