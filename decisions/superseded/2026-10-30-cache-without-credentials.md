# Cached transcripts without credentials

Without credentials the app still opens the cache (and prunes it), so cached transcripts load, and can be viewed, corrected and exported. Transcribe, Transcribe All and Force retranscription stay disabled, with the credentials hint on hover. In the Transcribed view, an uncached page says it has no cached transcript and shows the hint in place of the Transcribe button. Only without a cache directory as well are the Transcribed view and export disabled.

Reason: Human Taste (inline message for uncached pages); agent recommendation accepted for the rest.

Superseded by `2026-10-31-transcription-cache.md`.
