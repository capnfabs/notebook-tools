# Relink many photos

Depends on `relink-photos.md` (match tiers, EXIF fields, corner handling).

- When more than one page is missing, show **Relink All…**: a dialog where the user adds folders to scan, then presses **Scan**.
- Scan the folders recursively off the UI thread. Show a progress bar, or a spinner if progress can't be accurate. Filter by file size before hashing, so few files are hashed (re-encoded files won't match by size, so EXIF needs its own cheap pass).
- Each plausible match becomes a suggestion in a table: page, file found, how it matched.
- Matches by SHA-256 apply. Once every missing page has a SHA-256 match, the user can stop the scan early.

Open questions:
- Several candidates for one page; one file matching several pages.
- Whether EXIF-only matches apply without asking here, or need ticking in the table (single relink asks).
- Which folders, if any, to offer by default (the old folders? the project's?).
