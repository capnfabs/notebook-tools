# Try Sonnet to save cost

Run the bbox spike with Sonnet 5.5 and compare transcription and boxes against the Opus fixtures (`tests/fixtures/ocr/bbox-spike/`). Check Sonnet's image resolution tier first: on a lower tier, `MAX_IMAGE_PIXELS` is too big and the API rejects the image.
