# Confirm before transcribing

Switching to the Transcribed view transcribes the page shown; selecting another page while there shows a Transcribe button instead. Every page is looked up in the cache as it loads, so cached pages show straight away and export without a request. "Transcribe all" queues every untranscribed page (failed ones too), asking first if that's more than 4. At most 4 requests run at once; a single page's Transcribe, Retry or Force jumps the queue. Supersedes `superseded/2026-10-23-transcribe-on-toggle.md`.

Reason: Human Taste (the cap, confirming above 4, eager lookup); agent recommendation accepted for the rest.
