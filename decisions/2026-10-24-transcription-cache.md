# Cache transcriptions by photo hash + version

Claude's raw responses are cached in the OS cache dir (`~/Library/Caches/notebook-tools/transcriptions`), one file per photo and model, named by the SHA-256 of its file bytes, `CACHE_VERSION` and the model family (`2026-10-24-model-selector.md`), with the size of the image sent. They're parsed on load like a live response, so parsing changes need no bump; failures aren't cached. Effort isn't in the key. A schema change must bump the version (old answers won't parse; a test fingerprints the schema). Prompt, model or send-image changes bump it only if the developer wants pages redone, since that re-pays for every page. `scripts/clear-cache.sh` deletes the lot.

Reason: Human Taste (bump rule, key); agent recommendation accepted for the rest.
