# Background decoding

Move decode/thumbnailing off the UI thread (thread on desktop, web worker on wasm). ~100 ms per click/drop today in release.
