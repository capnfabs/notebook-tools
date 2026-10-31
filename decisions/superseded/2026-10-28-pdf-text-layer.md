# PDF: invisible text by opacity, stripped markdown, embedded `.md`

Each PDF page is the cleaned image as a JPEG at 300 DPI, which sets its physical size, so pages keep their own shape. Each line's text is stretched over its box at fill opacity 0, because krilla 0.8 has no invisible text mode. It searches, selects and copies like OCR text. The font is egui's bundled Ubuntu Light, which is already in the binary; characters it lacks may not search well. The text layer is plain text: list, heading and emphasis markers are stripped. The full markdown is embedded as an attachment, so nothing is lost. Export, markdown included, runs on a thread with a spinner, because a PDF cleans every page.

Reason: Human Taste (DPI, stripped text plus attachment, background export); unquestioned agent recommendation (opacity, font).

Superseded by `2026-10-31-pdf-export.md`.
