# Cache transcriptions

Cache API responses on disk, keyed by source file hash + a prompt version; a changed file or prompt version means re-transcribing. Bump the version whenever the prompt or schema changes.

- Dev helper (script) to clear the cache.
- Add a `CLAUDE.md` instruction (permission granted for this): when the prompt version changes, ask whether to clear the cache. A schema change needs a clear. For a prompt-only change it's optional.
