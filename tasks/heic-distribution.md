# HEIC through ImageIO on macOS

Make the macOS binary link nothing outside `/System` and `/usr/lib`. Design: `decisions/2026-11-01-heic-through-imageio.md`.

- `ImageIo` `Decoder` in `crates/core/src/decode.rs` (objc2-image-io / objc2-core-graphics). Draw into a bitmap context in the image's own colour space (raw values). Map `kCGImagePropertyOrientation` through `Orientation::from_exif` → `apply_orientation`, shared with the JPEG path.
- libheif `Heif` decoder and `libheif-rs` move behind `cfg(not(target_os = "macos"))`. README: no `brew install libheif` on mac.
- Error variant/strings: one HEIC error, whichever decoder.
- Don't bump `CACHE_VERSION`.
- Tests: existing `heic.rs` on ImageIO; a rotated fixture: `~/Downloads/notebook-tools-test/IMG_7277.HEIC` (irot 270° CCW, ImageIO orientation 6) via `sips -s format heic -Z 800`, then `exiftool -gps:all= -xmp:all= -exif:all= -makernotes:all=` (not `-all=`: it breaks the file for ImageIO), compared to an upright JPEG reference from `heif-convert`; a test that `otool -L` on the binary shows only system libraries.
