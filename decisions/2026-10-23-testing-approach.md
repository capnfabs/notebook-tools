# Testing approach

`insta` snapshots for Claude response parsing and export output; golden-image tests with a tolerance for the cleanup steps; recorded API responses so tests never call the live API, plus an opt-in check against real Claude; `egui_kittest` for UI.

Reason: unquestioned agent recommendation.
