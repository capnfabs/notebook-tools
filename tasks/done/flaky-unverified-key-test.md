# Flaky test: unverified API key warning

`api_key::a_key_that_couldnt_be_checked_is_saved_with_a_warning` (`crates/app/tests/viewer/api_key.rs`) failed once in the Linux container during `scripts/check.sh` and passed on rerun:

```
assertion failed: harness.query_by_label_contains(&warning).is_some()
```

The key check runs off the UI thread, so the warning may not be up yet when the test looks, or it may already be gone. Find out which, then make the test wait for it instead of checking once.

## Done

It was already gone. The warning is a 4 s hint, and kittest's clock moves 0.25 s a frame, so it lasts 16 frames. `save_key` settles, which steps through the transcription that starts once the key's saved; a slow transcription outlasts the hint (a 300 ms fake fetch failed it every time). The test now steps until the warning shows.
