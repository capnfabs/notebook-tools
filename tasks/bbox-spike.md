# Spike: Claude bounding-box accuracy

Before building UI on it: on a few real cleaned-up pages, check how accurate `claude-opus-5-5`'s line boxes and uncertain-word boxes are. Decide whether they're good enough, or whether we need a crop/zoom tool, tiling, or local detection.

No app/auth dependency: draft the prompt + JSON schema, the user runs it via claude.ai, paste responses back. Need a way to draw returned boxes over the image to judge them. Keep images ≤2576px long edge (coords then map 1:1 to pixels). Save prompt + responses as future test fixtures.
