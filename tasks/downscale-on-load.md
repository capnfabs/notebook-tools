# Downscale on load

Background step after a page is added (needs [background decoding](background-decoding.md)):
- Small copy (or the thumbnail) for fast page/spread detection.
- Once the page bounds are known, scale to a size that suits both Claude OCR input (check Opus 5.5 image limits) and faithful PDF reproduction.
- Replace the original with that image.
