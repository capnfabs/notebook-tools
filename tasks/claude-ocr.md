# Claude OCR

Transcribe the selected page via the Messages API; keep the result on the page. Display is Viewer actions' job.

- Image: original photo straightened + cropped to the detected outline, no lighting fix. ≤2576px long edge, JPEG (5 MB API limit). Split `detect_outline` + `straighten` out of `clean()` into a shared pub fn rather than duplicating.
- Request: `claude-opus-5-5`, effort `high`, structured output per `decisions/2026-10-23-ocr-response-shape.md`, `fallbacks: "default"` (beta `server-side-fallback-2026-07-01`). Check `stop_reason` (`refusal`, `max_tokens`) before parsing.
- Auth: `ANTHROPIC_API_KEY`, else `ANTHROPIC_AUTH_TOKEN` as Bearer + `anthropic-beta: oauth-2025-04-20`. Transcribe disabled without one, and on wasm.
- Runs in the background (thread on native).
- Keep the response's `usage` and `model` with the page's result, and `log::info!` them per call.
- Tests: snapshot request body; parse recorded responses; unit-test derived markdown + offsets; opt-in live test.

Draft prompt:

```
Transcribe this photo of a handwritten notebook page.
- Write exactly what's on the page: keep the author's spelling, abbreviations and line breaks. Don't correct or complete anything.
- One `lines` entry per handwritten line, top to bottom, with its markdown and box. Use markdown only for structure the page shows (headings, bullets, numbered lists, checkboxes). An empty `text_md` with a null box marks a paragraph break.
- Crossed-out text: ~~strikethrough~~. Drawings: `[drawing: short description]`.
- Add an `uncertain` entry for each word or phrase you're unsure of: line index, the text exactly as it appears in that line, its box, and 1–3 other likely readings.
- Boxes are [x0, y0, x1, y1] in pixels of this image.
```
