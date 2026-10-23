# `krilla` for PDF export

Searchable PDFs (page image plus invisible text layer) are built with `krilla`, falling back to `pdf-writer` if we need lower-level control. It's pure Rust and should work on WASM: leave the `rayon` feature off and bundle a font.

Reason: unquestioned agent recommendation.
