# Pages are cleaned on load, off the UI thread

When a file is dropped, the page goes into the strip straight away as a placeholder, holding its file's bytes. A background job (rayon's pool) then decodes and cleans it once, keeping the original and the cleaned page as JPEGs no longer than 2576px (q85), two thumbnails and the `CleanReport`. The viewer decodes those small JPEGs on a thread of its own and shows the matching thumbnail with a spinner until they're ready. The PDF embeds the cleaned JPEG as it is. Transcription still works from the file's bytes, which are the cache key and give Claude the full-resolution crop, so the image sent and the cache are unchanged. This supersedes `2026-10-23-pages-keep-encoded-bytes.md`.

About 1–1.5 MB of JPEGs per page on top of the file, against ~48 MB to decode a 12 MP photo, which used to happen on the UI thread for every click (~100 ms for a JPEG, ~170 ms for a HEIC). Reason: Human Taste (goal: a snappy UI).
