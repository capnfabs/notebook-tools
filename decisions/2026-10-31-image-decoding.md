# Image decoding: one `Decoder` per format, HEIC through the system libheif

Everything past decoding works on the `image` crate's `RgbaImage`; nothing downstream knows whether a page was a JPEG, PNG or HEIC. Each format has a `Decoder` in `crates/core/src/decode.rs` (the `image` crate for JPEG and PNG, `libheif-rs` for HEIC), and the first that recognizes a file's bytes decodes it, turned upright: EXIF orientation for JPEG, libheif's own rotation for HEIC, which is how iPhones record it.

`libheif-rs` links the system libheif found by pkg-config (`brew install libheif`, 1.19+). Its `embedded-libheif` feature only builds libheif itself; its codecs (libde265 and others) would still be Homebrew dylibs, so it wasn't worth needing cmake and slower clean builds. A self-contained app needs the dylibs bundled: `tasks/heic-distribution.md`. Reason: unquestioned agent recommendation (the abstraction); Human Taste (system libheif).
