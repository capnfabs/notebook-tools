# project management

- `spec.md` contains the core program specification.
- `tasks/` is a lightweight task tracker, use `tasks/index.md` as the hub and create _new_ tasks as separate markdown files there. Keep task descriptions extremely concise; prefer having to rediscover things to dumping all the context.
- `decisions/` contains a decision log, each entry is `yyyy-mm-dd-$summary.md`. each shouldn't be more than two paragraph long, but prefer to be concise. "Human Taste" is a perfectly good reason, as is "unquestioned agent recommendation" (when a human accepts your recommendation without questioning).
- Keep stale knowledge out of the way of search:
  - When a task is done, move its file to `tasks/done/` and its index line to the end of `tasks/done/index.md`.
  - When a decision replaces an earlier one, move the old entry to `decisions/superseded/` and end it with "Superseded by `<new entry>`". If it only changes part of one, fix that part in place and point to the new entry.
  - Link a decision from the code it governs, in a module or item doc comment.
- agents must not modify markdown files in the root directory for this project without explicit permission
- never ask to commit your changes in this repo.

# layout

- `crates/core` (`notebook_core`): everything that isn't UI. `decode` (files to pixels), `clean/` (find corners, straighten, relight), `geom` (corners as `Quad`), `imaging` (resize, JPEG), `loaded` (a page decoded and cleaned once), `page` (`Page`/`Pages`), `project` (`.notebook` files), `claude_transcribe` (the API request and parsing), `cache`, `transcript/` (Claude's answer, finding uncertain words, corrections), `reframe` (boxes onto new corners), `markdown_style`, `export` (markdown, PDF), `settings`, `llm_model`. Imported by module path; nothing is re-exported at the root.
- `crates/app` (`notebook_app`, binary `notebook-tools`): the egui UI. `app/` is `NotebookApp`, its methods split by job (see `app/mod.rs`); `command.rs` + `menu.rs` are commands, shortcuts and the macOS menu, and `quit.rs` asks to save when macOS quits the app; `ingest`, `transcriptions`, `export` run work off the UI thread; `ui/` holds widgets.
- A new command: add it to `Command` and `MENUS` in `crates/app/src/command.rs`, its label to the strings file, and what it can do to `NotebookApp::can`.
- Tests: UI tests in `crates/app/tests/viewer/` (one module per feature, helpers in `common.rs`); core's in `crates/core/tests/` with fixtures in `tests/fixtures/` (recorded API responses in `fixtures/claude/`).
- `scripts/check.sh` (fmt, clippy, tests on macOS, then clippy + tests in a Linux container) must pass before work is done; `--mac-only` is fine while iterating.

# personal content

- This repo is (or will be) public. Before committing anything personal (photos, recorded transcriptions, names, places, dates, anything from the user's notebooks or life), check with the user first and list exactly what's new or changed.
- The pre-commit hook (`.githooks/pre-commit`; `scripts/check.sh` fails until it's on: `git config core.hooksPath .githooks`) rejects images not in `scripts/reviewed-images.txt` and image metadata. Only the user approves images (`scripts/check-images.sh --approve PATH`). See `decisions/2026-11-01-image-guardrails.md`.

# impl

- user-facing text goes in `crates/app/i18n/en-US/notebook_app.ftl`, looked up with `t!` (`crates/app/src/i18n.rs`); tests use the same lookup. See `decisions/2026-10-30-strings-in-fluent.md`.
- build dialogs with `crates/app/src/ui/dialog.rs` (macOS button order: affirmative rightmost); see `decisions/2026-10-23-dialog-buttons.md`.
- new UI must work with keyboard focus: decide whether each widget belongs in the Tab order (egui makes anything that senses clicks or drags focusable; drop `Sense::FOCUSABLE` if not), make focus visible, and remember the arrow keys move focus: a widget that needs them keeps them with `ui/focus.rs`'s `keep_arrows`. Test it. See `decisions/2026-10-31-keyboard-focus.md`, `decisions/2026-10-31-arrow-keys-move-focus.md` and `tasks/done/keyboard-focus-audit.md`.
- transcriptions are cached by photo hash + `CACHE_VERSION` (`crates/core/src/cache.rs`) + model family. A schema change must bump it (a test enforces this). After changing the prompt, model or the image sent (crop/straighten), ask whether to keep the cached transcriptions; bump only if not. `scripts/clear-cache.sh` empties the cache.
