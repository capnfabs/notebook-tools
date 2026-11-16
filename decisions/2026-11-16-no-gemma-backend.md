# Gemma 4 on Tinfoil isn't good enough to transcribe with

We tried Gemma 4 31B (`gemma4-31b`), served privately on Tinfoil, as a transcription backend with `examples/tinfoil_gemma.rs`. It used Claude's prompt and schema on one journal spread, sent at 1824×1344px with the 1120-token image budget (`mm_processor_kwargs.max_soft_tokens`, which Tinfoil honours; 1,066 image tokens). With thinking on, it cost about 1¢ a spread (9,019 output tokens, 6,317 of them thinking), roughly a tenth of Opus. It was much slower than Claude. Its boxes come back normalized to 0–1000, not in the image's pixels. Its text was slightly less accurate than Sonnet's cached transcript. About ten words differed, including two place names, and only two words were flagged as uncertain, neither of them a real misreading. It also normalized the author's spelling despite the prompt, and added headings. Without thinking, it lost paragraph breaks and underlines, emitted HTML attributes, and its boxes switched to [y0, x0, y1, x1] and were a mess, so thinking is required.

We only tried with one image, so like... results may vary, might be acceptable for the masses, etc.

Decision: transcription stays with Claude, since Gemma isn't good enough for this handwriting.
