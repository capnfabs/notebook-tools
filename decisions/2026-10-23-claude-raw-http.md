# Claude via raw HTTP, structured outputs, Opus 5.5

There's no official Rust SDK, so the Claude client is plain `reqwest` + `serde` against the Messages API (also works on WASM). Transcription and boxes come back via structured outputs (`output_config.format`) from `claude-opus-5-5`.

Reason: unquestioned agent recommendation.
