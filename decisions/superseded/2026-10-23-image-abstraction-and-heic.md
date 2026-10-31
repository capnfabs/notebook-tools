# Image abstraction and HEIC decoding

Everything past decode works on the `image` crate's types (`RgbaImage`/`DynamicImage`), wrapped in our own page type with source and orientation info; nothing downstream knows about HEIC or JPEG. Decoding sits behind a small trait, starting with `libheif-rs`, with macOS/iOS ImageIO as an alternative. Apply EXIF orientation at decode. HEIC details: `2026-10-28-system-libheif.md`.

Reason: unquestioned agent recommendation.

Superseded by `2026-10-31-image-decoding.md`.
