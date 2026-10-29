# Claude via raw HTTP, structured outputs, Opus 5.5

There's no official Rust SDK, so the Claude client is plain `reqwest` + `serde` against the Messages API. Transcription and boxes come back via structured outputs (`output_config.format`) from `claude-opus-5-5`, or Sonnet 5.5 since `2026-10-24-model-selector.md`.

Reason: unquestioned agent recommendation.
