# project management

- `spec.md` contains the core program specification.
- `tasks/` is a lightweight task tracker, use `tasks/index.md` as the hub and create _new_ tasks as seperate markdown files there. Keep task descriptions extremely concise; prefer having to rediscover things to dumping all the context.
- `decisions/` contains a decision log, each entry is `yyyy-mm-dd-$summary.md`. each shouldn't be more than two paragraph long, but prefer to be concise. "Human Taste" is a perfectly good reason, as is "unquestioned agent recommendation" (when a human accepts your recommendation without questioning).
- agents must not modify markdown files in the root directory for this project without explicit permission
- never ask to commit your changes in this repo.

# impl

- build dialogs with `crates/app/src/ui/dialog.rs` (macOS button order: affirmative rightmost); see `decisions/2026-10-23-dialog-buttons.md`.
- transcriptions are cached by photo hash + `CACHE_VERSION` (`crates/core/src/ocr.rs`). A schema change must bump it (a test enforces this). After changing the prompt, model or the image sent (crop/straighten), ask whether to keep the cached transcriptions; bump only if not. `scripts/clear-cache.sh` empties the cache.
