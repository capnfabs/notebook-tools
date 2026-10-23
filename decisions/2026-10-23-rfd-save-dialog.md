# `rfd` for the save dialog

Markdown export asks where to save with `rfd`'s native dialog, native only (`scripts/check.sh` keeps it out of the wasm tree). The app takes it as an injected `SavePath`, so tests stand in for it. The clipboard uses egui's `copy_text`.

Reason: unquestioned agent recommendation.
