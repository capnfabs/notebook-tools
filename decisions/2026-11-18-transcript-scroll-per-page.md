# Transcript scroll position per page

The transcript's `ScrollArea` takes its Id from the page (`id_salt(("transcript", page.id))`), so egui keeps each page's scroll position: a page not seen yet opens at the top, one seen before comes back where it was, for as long as the app runs. Nothing else is remembered. `PageId`s are never reused and eframe persistence is off, so an opened or new project starts at the top. Force Retranscribe keeps the old position, clamped to the new transcript. Changing page with the focus in the transcript still focuses the new page's top line, which scrolls it to the top.

Reason: Human Taste. The task first asked to forget a position after an hour and to bring back the keyboard focus as well. Letting egui keep the positions made those two the only code left, and they weren't worth it.
