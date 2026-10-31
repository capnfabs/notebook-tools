# Open projects by drop and from the command line

Projects only open with Open Project… (`decisions/2026-10-31-projects.md`). Also open one:
- dropped on the window: a `.notebook` among the dropped files opens it, after asking to save as Open does (`After::OpenProject` in `crates/app/src/app/project.rs`), instead of being added as a page. Decide what a drop of a project plus photos does.
- given as the command-line argument: `notebook-tools path/to/notes.notebook` starts with it open (`crates/app/src/main.rs`). Handy for agents driving the app.
