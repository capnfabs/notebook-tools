# Export with missing pages

Missing pages are placeholders in the strip (`decisions/2026-11-15-missing-pages-shown.md`), and for now export refuses every format while one is in scope ("Photo missing: …", `in_scope` in `crates/app/src/export.rs`). That stays right for **PDF**, which needs the photo. What's left:

- **Markdown** (file and clipboard) needs only the transcript: a missing page with a saved transcript exports like any other, in its place. One without a transcript blocks, saying it can't be transcribed until its photo is found (not "Not transcribed yet").
- `Snapshot` needs the loaded page (cleaned JPEG, size, `reframed` boxes): Markdown shouldn't. Use the saved transcript as it is.

Tests (`crates/app/tests/viewer/export.rs`, and `missing.rs`'s `exporting_needs_the_photos`): PDF refused with a missing page in scope, and allowed for the current page when that's a found page; Markdown file and clipboard include a missing page's saved transcript in order; a missing page with no transcript blocks Markdown with the right message.

Really specifically for exports:

- PDF:
  - Missing Photo (only): 'Need to relink these photos for export: '
  - Missing Transcript (only): 'Not transcribed yet:' (as today)
  - Missing both: 'Need to relink and transcribe these photos for export: '
- Markdown:
  - Missing Photo: (no text, it just works)
  - Missing Transcript (only): 'Not transcribed yet:' (as today)
  - Missing both: 'Not transcribed yet:' (as today, I think?)
