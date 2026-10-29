# The viewer keeps two full-size textures

The Cleaned view shows the photo and the cleaned page at once, so the viewer keeps the selected page's original and cleaned textures, not one as `2026-10-28-pages-cleaned-on-load.md` said. Each is keyed by its JPEG's `Arc`, so cleaning the page again makes the old texture stale; it stays up, with a spinner, until the new one is decoded. Cleaning again keeps the page's original JPEG, so the photo isn't decoded again.

Reason: unquestioned agent recommendation.
