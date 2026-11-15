# Export with missing pages

Depends on `missing-pages.md` (missing pages shown in the strip as placeholders). Export already refuses until every page in scope is ready, listing the ones that aren't (`in_scope`, `crates/app/src/export.rs`; `decisions/2026-10-23-export-rules.md`). Extend that rule to missing pages, by what each format needs:

- **PDF** needs the photo: refuse while any page in scope is missing, listing them (e.g. "Photo missing: IMG_1, IMG_2"), alongside the existing "Not transcribed yet" / "Still loading" reasons. The user relinks or deletes them, or exports the current page only.
- **Markdown** (file and clipboard) needs only the transcript: a missing page with a saved transcript exports like any other, in its place. One without a transcript blocks, as untranscribed pages do now; the message should say it can't be transcribed until its photo is found.
- A missing page never loads, so it mustn't count as "Still loading" for Markdown. `Snapshot` currently needs the loaded page (cleaned JPEG, size, `reframed` boxes): Markdown shouldn't. Use the saved transcript as it is.

Tests (`crates/app/tests/viewer/export.rs`): PDF refused with a missing page in scope, and allowed for the current page when that's a found page; Markdown file and clipboard include a missing page's saved transcript in order; a missing page with no transcript blocks Markdown with the right message.
