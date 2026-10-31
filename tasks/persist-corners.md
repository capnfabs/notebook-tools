# Keep page corners across restarts

Corners the user places live in memory only (`decisions/2026-10-29-manual-corners.md`). After a restart the page goes back to detected corners. A transcript forced with the user's corners stays cached and its boxes follow the detected corners (`decisions/2026-10-30-boxes-follow-corners.md`), but the user has to place theirs again.

We'll fix this by adding a notion of _projects_. The "File" menu gets New Project, Open Project, Save, Save As... options. Projects are stored in files (`.notebook`). We need to store:
- anything / everything the user might've modified. this includes:
    - any transcripts (note that we still cache on the basis that it's useful for unsaved projects)
    - corners / image edits
- not the images themselves, but the hashes and the relative paths. Probably any unique IDs and/or creation time in the EXIF data. If the images _move_ then we'll be able to reassociate them later using this info (though this is out of scope for now).

Other notes:
- Exiting / closing without saving asks if the user wants to save.
- It's ok to only support one open project in the application for now.
