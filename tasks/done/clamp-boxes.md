# Clamp boxes to the image

Claude's boxes can overrun the image slightly (9px seen). Clamp them when parsing, then drop the slack in `tests/ocr.rs`'s `assert_boxes_in_bounds`.

(I'm not 100% sure about this task anymore, so investigate whether it's necessary and/or still makes sense).

Dropped 2026-10-30: unneeded. The UI clips boxes to the image, `Transcript::fit` already ignores overrun, and clamping would shrink bottom lines' PDF text.
