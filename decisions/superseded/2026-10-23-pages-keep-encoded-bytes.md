# Pages keep encoded bytes; decode on demand

A `Page` holds the file's encoded bytes plus a small thumbnail, not decoded pixels: a 12 MP photo is ~48 MB decoded, too much for a notebook's worth on the wasm heap. The viewer decodes only the selected page. Dropped files are decoded one per frame so a big drop doesn't freeze the UI (no threads on wasm).

Reason: unquestioned agent recommendation.

Superseded by `2026-10-28-pages-cleaned-on-load.md`.
