# notebook-tools

Notebook-tools is a tool for OCR-ing a handwritten notebook for desktop computers. It transforms / cleans photos locally, sends them to Claude for transcription, and offers a correction / proofreading interface and export to Markdown and PDF.

Note that this tool _costs money_ to use; you need a Claude developer account and to be logged in on the machine. When I was testing in Oct 2026 using Opus 5.5, it was about $0.07 per two-page spread. For an 80-page A5 notebook (like the Moleskine Cahier journals that I take hiking) you're looking at $2.80 in transcription fees.

## Basic flow:
- Drop a bunch of photos of your notebook onto the UI (either single page photos or spreads, both are ok)
- The tool will attempt to detect the pages / spreads in each photo, and mess with the levels to make it look like a scan. Switch to the 'Cleaned' view and check the borders on all the pages; the page detection's not amazing yet but you can set / adjust them manually.
- Once the pages are all detected and tidy, hit "Transcribe all" to send them all to Claude in batches (costs money!!)
- Correct all the transcripts in the _Transcribe_ view. Uncertain words are highlighted with alternatives.
- Once you're done, export everything to PDF and/or Markdown. If you export to PDF, you get half-decent copy-paste and search, and the full markdown transcript is embedded as an attachment so you can pull it out again later if you need it ✨
- Transcriptions are cached so you can use them again if you need to.

## Extra tips:
- You can switch to Sonnet if your handwriting is neat and get faster / cheaper transcriptions for little loss in quality (my handwriting is shit though)
- You can customize the markdown export a little in Settings; but by default it converts inline underlined text to _italics_ with underscores, and underlined headings as `## headings`.
- Keyboard nav is super useful for the transcripts.
