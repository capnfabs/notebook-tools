# Show pages whose photos are missing

When a project opens, a page whose photo is gone or changed is hidden: kept in `missing` (`crates/app/src/app/project.rs`) and saved again as it was, with an error in the status line (`decisions/2026-10-31-projects.md`). Show it in the strip instead, as a placeholder, in its place. Its saved transcript can be read and corrected (no page image beside it); it can't be transcribed or have its corners changed. What export does with it: `missing-page-export.md`.

The placeholder says why the photo is missing (not found, changed since saved, unreadable): `relink-photos.md` adds its Relink… and Use Changed File buttons there.
