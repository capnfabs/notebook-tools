# Spike: Claude bounding-box accuracy

Before building UI on it: on a few real cleaned-up pages, check how accurate `claude-opus-5-5`'s line boxes and uncertain-word boxes are. Decide whether they're good enough, or whether we need a crop/zoom tool, tiling, or local detection.

Run pages through the [Claude OCR](claude-ocr.md) client. Need a way to draw returned boxes over the image to judge them. Save responses as test fixtures. Also try effort levels, comparing recorded token usage.
