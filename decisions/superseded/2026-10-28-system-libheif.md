# HEIC through the system libheif

HEIC is decoded by `libheif-rs` behind the `Decoder` trait in `crates/core/src/decode.rs`, linked to the system libheif found by pkg-config (`brew install libheif`, 1.19+). libheif applies the file's rotation, which is how iPhones record orientation in HEIC. The crate's `embedded-libheif` feature only builds libheif itself: its codecs (libde265 and others) would still be Homebrew dylibs, so it wasn't worth needing cmake and slower clean builds. A self-contained app needs the dylibs bundled: `tasks/heic-distribution.md`.

Reason: Human Taste.

Superseded by `2026-10-31-image-decoding.md`.
