# Relink moved photos

Depends on `missing-pages.md`: a missing page is already a placeholder in the strip that says why its photo is missing. Projects name photos by relative path + SHA-256 + size (`notebook_core::project::Photo`); deferred from `decisions/2026-10-31-projects.md`. Finding many at once by scanning folders: `relink-many.md`.

## 1. Remember each photo's EXIF IDs
Fields, chosen from the user's library (17.6k photos, 2026-11-15):
- **PhotoIdentifier** (Apple MakerNote): on 44%, every iPhone HEIC. Repeats were always the same photo (edits `IMG_E…`, Photos library derivatives, HEIC beside its JPEG); never two different photos.
- **DateTimeOriginal + SubSecTimeOriginal + OffsetTimeOriginal**: whenever PhotoIdentifier is set, these are too, and they agreed with it exactly. About 2k older JPEG/PNGs have whole seconds only; their few shared times were mostly the same photo, but not always.
- **ImageUniqueID**: on 4 of the user's photos, but cheap and standard for other cameras.
- Not ContentIdentifier: nearly always alongside PhotoIdentifier, and an edit gets a new one, so it adds nothing.
- ~7.7k files (mostly JPEG) have none of these: only the hash finds them.

Save them in `Photo` as optional fields, so older projects still open (no `PROJECT_VERSION` bump if older builds ignore unknown fields; check). Read them when a page is added; fill them in for found photos when a project opens (that alone isn't an unsaved change). Read metadata only; don't decode the image.

PhotoIdentifier is in Apple's MakerNote, not standard EXIF: on mac, ImageIO gives it in `kCGImagePropertyMakerAppleDictionary`, keyed by tag number: PhotoIdentifier is `0x002b` (43), a UUID string (`exiftool -a -G1 -H -Apple:PhotoIdentifier FILE` on an iPhone HEIC); elsewhere it needs a small MakerNote parser. Let's try to do it on Mac _and_ non-mac platforms but if it's too hard `non-mac-heic.md` can pick up the slack.

## 2. Relink one page
- The placeholder gets **Relink…**: a file chooser that opens in the photo's old folder, or its nearest parent that still exists. No automatic search when a project opens.
- A photo **changed in place** (same path, different bytes) also gets **Use Changed File**: the same checks as below, without the chooser.
- Compare the chosen file with what the project saved:
  - **Same SHA-256**: a perfect match. Relink without asking, whatever its filename.
  - **EXIF matches, bytes differ** (re-encoded or edited): a light confirm, e.g. "Not the same file, but looks like the same photo. Relink?", saying whether corners will be kept, scaled or dropped. EXIF matches when:
    - both have a PhotoIdentifier (or both an ImageUniqueID) and they're equal; if both have one and they differ, it's not a match, whatever the time says;
    - otherwise, DateTimeOriginal is equal, and so are SubSecTimeOriginal and OffsetTimeOriginal where both have them. Whole seconds alone count too (user's call), so this confirm must show enough to tell (e.g. both filenames, thumbnails if cheap).
  - **Nothing matches**: warn that they don't match, and let the user relink anyway.
  - The filename never decides a match on its own (IMG_0001 repeats across folders and cameras); a warning can mention it's the same or different.
- When the bytes differ, corners and the transcript:
  - same pixel size: kept;
  - same aspect, different size: corners and transcript boxes scaled (`reframe` may help);
  - otherwise: corners dropped (re-detected), transcript kept but flagged as made from the old photo.
- The page keeps its name: it's the page's ID (unique, sort order, job/texture keys, `Missing::after`, export). Only path, hash, size and EXIF change. A relink is an unsaved change ("Edited").

## Tests
- Core: the match tiers (hash, EXIF only, none), including a differing PhotoIdentifier beating an equal time and whole-second times; EXIF (PhotoIdentifier too) read from JPEG and HEIC. The pre-commit hook rejects committed images with metadata, so build the EXIF in test code (as `decode`'s orientation test does) rather than committing photos; ask the user if a real HEIC fixture is unavoidable, corner and box scaling, old projects without EXIF still open.
- UI (`crates/app/tests/viewer/`): Relink… with a stand-in chooser for each tier, Use Changed File, the warning's cancel/proceed, saving after a relink, and keyboard focus on the new buttons and dialogs.
