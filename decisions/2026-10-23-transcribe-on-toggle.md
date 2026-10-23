# Transcribe when the Transcribed view is opened

Selecting the Transcribed toggle transcribes the selected page once; the result stays on the page, and a failure waits for Retry. There's no separate Transcribe button. Requests run on a thread per page with reqwest's blocking client, native only: the web build has no threads, so Transcribed is disabled there, and `scripts/check.sh` keeps reqwest out of the wasm tree.

Reason: agent recommendation, accepted (wireframe: "should never retranscribe after first attempt").
