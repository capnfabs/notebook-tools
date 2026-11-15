# CI

We need something like CI. As much as possible, I want to be able to run _locally_, but I want to be able to check that stuff works ok on other platforms. In particular: the libheif decoder (`crates/core/src/decode/libheif.rs`) only compiles off macOS, and cross-checking from a Mac needs a Linux C toolchain (aws-lc-sys).

Open questions:
- Something virtualized maybe? Using https://github.com/apple/container?
- Should we run this as part of the preflight script (scripts/check.sh)? Probably if it's cheap?
- `decisions/2026-11-01-image-guardrails.md` says no CI copy of the image check; revisit?

Done: `scripts/check-linux.sh`, run by `scripts/check.sh`. See `decisions/2026-11-15-local-linux-ci.md`.
