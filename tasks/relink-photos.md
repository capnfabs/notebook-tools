# Relink moved photos

Projects name photos by relative path + SHA-256 + size (`notebook_core::project::Photo`); a moved or changed photo leaves its page missing (`tasks/missing-pages.md`). Deferred from `decisions/2026-10-31-projects.md`:
- Save each photo's EXIF ImageUniqueID / DateTimeOriginal (needs an EXIF reader; HEIC through libheif) to recognise it after it's moved or re-encoded.
- Find a missing page's photo again: search near the old path, or let the user point at it or a folder, matching by hash, then EXIF.
- Relink a page whose file changed, if the user says it's the same photo.
- Pages dropped without a file location can't be saved (`SaveError::NoPath`); macOS drops always have one.
