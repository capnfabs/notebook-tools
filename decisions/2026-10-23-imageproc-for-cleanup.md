# Pure-Rust `imageproc` for page cleanup

Page detection, perspective correction, cropping and lighting fixes use `imageproc`. The `opencv` crate is the escape hatch only if `imageproc` proves insufficient, since it's a heavy C++ dependency that rules out WASM.

Reason: unquestioned agent recommendation.
