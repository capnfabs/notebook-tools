# Sonnet as a faster, cheaper transcription option

Opus 5.5 and Sonnet 5.5 (effort `high`) transcribed the same 5 journal pages and were corrected by hand, with `eval` loglines recording every correction. Sonnet is about 2–3× faster (9–15s a page; Opus took 20–45s in the bbox spike) and costs about $0.03 a page against Opus's $0.07, since it costs half as much per token and wrote about 40% fewer output tokens, with no thinking. Both read words about equally well (98–99% the same text). Sonnet is worse at telling you where it's wrong. It flagged fewer uncertain words (16 against 27), so more of its mistakes went unflagged (about 10 against 7) and had to be found by reading. Its alternatives often just repeat its own reading. It drops `---` separators (7 missed against 1), underlines, superscripts and crossed-out words, and misreads drawn arrows. Opus caught more typos and alternative spellings among its flagged words.

Both models stay available, with Opus the default: Sonnet suits a quick first pass, Opus pages you'll correct closely. Some of Sonnet's gaps may close with prompt changes (`tasks/markdown-cleanup.md`). Five pages is a small sample. Reason: Human Taste (keep both, Sonnet is good enough), from the agent's measurements.

Superseded by `2026-10-31-transcription-models.md`.
