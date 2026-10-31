# Show pages whose photos are missing

When a project opens, a page whose photo is gone or changed is hidden: kept in `missing` (`crates/app/src/app/project.rs`) and saved again as it was, with an error in the status line (`decisions/2026-10-31-projects.md`). Show it in the strip instead, as a placeholder, in its place. Its saved transcript can be read and corrected (no page image beside it) and goes into Markdown export; it's left out of PDF export, and can't be transcribed or have its corners changed.
