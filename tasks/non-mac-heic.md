# HEIC on non-mac builds

For the first non-mac build (`decisions/2026-11-01-heic-through-imageio.md`): ship libheif + libde265 as separate shared libraries, built without x265, with LGPL notices and source offer. CI there, since `scripts/check.sh` on a Mac never compiles the cfg'd libheif decoder. Fedora strips libde265 from its libheif for patent reasons; Windows could use WIC + HEVC extension instead. A cross `cargo check --target x86_64-unknown-linux-gnu` from a Mac needs a Linux C toolchain (aws-lc-sys), so it isn't in `scripts/check.sh`.
