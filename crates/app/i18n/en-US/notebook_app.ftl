# Everything the app says to the user. Looked up with `t!` (`src/i18n.rs`); see
# `decisions/2026-10-30-strings-in-fluent.md`. Syntax: https://projectfluent.org/fluent/guide/

app-name = notebook-tools

## The menu bar. The app menu is named after the app.

menu-file = File
menu-view = View
menu-page = Page
menu-window = Window

## Commands: menu items, and the buttons that do the same.

command-settings = Settings…
command-quit = Quit { app-name }
command-new-project = New Project
command-open-project = Open Project…
command-save = Save
command-save-as = Save As…
command-add-pages = Add Pages…
command-export = Export
# Export, once there's a last export to repeat.
command-reexport = Re-export
command-export-as = Export…
command-view-original = Original
command-view-cleaned = Cleaned
command-view-transcribed = Transcribed
command-previous-page = Previous Page
command-next-page = Next Page
command-transcribe = Transcribe
command-force-retranscribe = Force Retranscription
command-reset-transcript = Reset Transcript
command-transcribe-all = Transcribe All
command-move-left = Move Left
command-move-right = Move Right
command-sort-by-name = Sort by Name
command-place-corners = Place Corners
command-reset-corners = Reset Corners
command-delete = Delete

## Dialogs

dialog-cancel = Cancel
confirm-delete = Delete { $name } and its transcript?
confirm-delete-button = Delete page
confirm-transcribe-all = { $count ->
    [one] Transcribe { $count } page?
   *[other] Transcribe { $count } pages?
}
confirm-transcribe-all-button = Transcribe
confirm-force = Retranscribe { $name }? Your corrections will be lost.
confirm-force-button = Retranscribe
confirm-reset = Reset { $name } to Claude's transcription? Your corrections will be lost.
confirm-reset-button = Reset
# Before closing, quitting, or starting or opening another project.
unsaved-title = Save changes to { $name }?
unsaved-detail = Your changes will be lost if you don't save them.
unsaved-save = Save
unsaved-discard = Don't Save

## The window, before and after pages are added

drop-zone-title = Drop image files here
drop-zone-formats = JPEG, PNG or HEIC
drop-overlay = Drop to add pages
# While a project is dragged over the window: dropping it opens it. `decisions/2026-10-31-open-by-drop-and-argument.md`.
drop-overlay-open = Drop to open { $name }
drop-overlay-one-project = Drop one project at a time
drop-several-projects = Couldn't open the projects dropped: drop one at a time.
# Files dropped with a project aren't added to it.
drop-others-left-out = { $count ->
    [one] The other file dropped wasn't added.
   *[other] The { $count } other files dropped weren't added.
}
# Why transcribing is unavailable (and, without a cache, the Transcribed view and exporting).
no-transcriber-hint = Transcription needs a Claude API key: add one in Settings.
all-transcribed-hint = Every page is transcribed
# The selected page, in the toolbar.
page-position = { $name } · { $number } of { $count }
# The handles dragged to resize the panes side by side, for screen readers.
resize-cleaned = Resize the cleaned page
resize-transcript = Resize the transcript
# The page's image, for screen readers.
current-page = Current: { $name }
current-page-loading = Current: { $name } (loading)
current-page-cleaned = Current: { $name } (cleaned)
current-page-cleaned-loading = Current: { $name } (cleaned, loading)

## The status line

status-loading = Loading { $count }…
status-transcribing = { $count ->
    [one] Transcribing { $count } page…
   *[other] Transcribing { $count } pages…
}
status-exporting = { $count ->
    [one] Exporting { $count } page…
   *[other] Exporting { $count } pages…
}
open-failed = Couldn't open { $name }: { $error }
settings-save-failed = Couldn't save settings: { $error }
already-transcribed = { $name } is already transcribed. Retranscribe with { $shortcut }.

## Transcription progress

transcribing-page = Transcribing { $name }…
transcribe-failed = Couldn't transcribe { $name }: { $error }
transcribe-retry = Retry
checking-cache = Looking for { $name } in the cache…
not-transcribed-yet = { $name } isn't transcribed yet.
# In place of the transcript when there's nothing to transcribe with.
not-cached = No cached transcript for { $name }.

## Under a transcript

# When and by which model the transcript was made, for the messages after it. $at is "time" when
# it was cached at $time, "unknown" when it was cached at a time that can't be shown, and "none"
# when it wasn't cached.
transcribed-when = { $at ->
    [time] { $time } by { $model }
    [unknown] at an unknown time by { $model }
   *[none] by { $model }
}
transcript-from-claude = Transcribed { $when }
transcript-from-cache = From cache: transcribed { $when }
transcript-retranscribe-failed = Retranscription failed: { $error }. Using the cached version, transcribed { $when }.
transcript-lines-cut-off = The bounds of this page have shrunk since last transcription; { $count ->
    [one] { $count } transcribed line falls outside this page's new bounds.
   *[other] { $count } transcribed lines fall outside this page's new bounds.
}
transcript-missed-page = The bounds of this page have grown since last transcription; retranscribe to ensure you've got all text.

## Correcting a transcript

edit-hint = Double-click to edit
# The text field for a line being edited, when it's empty. Also names the field for screen readers.
edit-empty-line = Empty line
# An uncertain word's box on the page, for screen readers.
uncertain-box = Box: { $word }
deleted-word-hint = Deleted "{ $word }": click to put it back or change it
delete-struck = Delete struck text
keep-word = Keep "{ $word }"
custom-reading = Something else
custom-reading-ok = OK
unfound-edited-out = "{ $word }" was edited out of its line, so it can't be automatically corrected. Correct it manually, or bring it back with { command-reset-transcript }.
unfound-not-in-transcript = Claude was unsure of "{ $word }" here, but it isn't in the transcript. Fix it by hand by double-clicking on the transcript.
# $alternatives are Claude's other readings, joined with " / ".
unfound-not-in-transcript-alternatives = Claude was unsure of "{ $word }" (or { $alternatives }) here, but it isn't in the transcript. Fix it by hand by double-clicking on the transcript..

## Page corners

corner-top-left = Top-left corner
corner-top-right = Top-right corner
corner-bottom-right = Bottom-right corner
corner-bottom-left = Bottom-left corner
# The area clicked while placing corners, for screen readers.
corners-placing-label = Click the page's corners
corners-placing-prompt = Click the page's four corners. Esc cancels.
corners-refused-prompt = Those corners don't outline a page. Click another fourth corner, or Esc to cancel.

## Export

export-title = Export
export-everything = Everything
export-current-page = Current page
# Between what to export and the formats,
# e.g. "Export Everything as PDF" or "Export Current page as Markdown to file"
export-as = as
export-markdown-clipboard = Markdown to clipboard
export-markdown-file = Markdown to file
export-pdf = PDF
export-go = Export
export-crashed = Couldn't export: it crashed
# $reasons are the export-not-… messages below, joined with ". ".
export-blocked = Couldn't export. { $reasons }
# $names are the pages', joined with ", ".
export-not-transcribed = Not transcribed yet: { $names }
export-still-loading = Still loading: { $names }
export-no-pages = No pages to export
# $name is the page's when there's one.
export-copied = { $count ->
    [1] Copied { $name } to the clipboard
   *[other] Copied { $count } pages to the clipboard
}
export-saved = { $count ->
    [1] Exported { $name } to { $path }
   *[other] Exported { $count } pages to { $path }
}
export-failed = Couldn't export: { $error }
export-write-failed = Couldn't write { $path }: { $error }
# Suggested file name, without its extension, when exporting more than one page.
export-default-name = notebook
pdf-empty-image = { $page } is an empty image
pdf-embed-failed = Couldn't embed { $page }: { $error }
pdf-build-failed = Couldn't build the PDF: { $error }

## File types, in the system's open and save dialogs

file-type-pdf = PDF
file-type-markdown = Markdown
file-type-photos = Photos
file-type-project = notebook-tools project

## Projects. See `decisions/2026-10-31-projects.md`.

# A project that hasn't been saved yet.
project-untitled = Untitled
# The window's title, which is otherwise the project's name, when it has unsaved changes.
window-title-edited = { $name } — Edited
project-open-failed = Couldn't open { $path }: { $error }
project-not-a-project = { $path } isn't a notebook-tools project, or it's damaged: { $error }
project-newer-version = { $path } was saved by a newer version of { app-name }. Update to open it.
project-save-failed = Couldn't save { $path }: { $error }
# Pages dropped without a file location (rare) can't be saved in a project.
project-no-path = Couldn't save: { $names } { $count ->
    [one] was
   *[other] were
} added without a file location. Delete and add { $count ->
    [one] it
   *[other] them
} again with { command-add-pages }.
# A page whose photo wasn't found when the project opened. It's kept, and saved again as it was.
photo-not-found = Couldn't find { $name } at { $path }. Its page is kept in the project, but hidden.
photo-changed = { $path } has changed since the project was saved. { $name }'s page is kept in the project, but hidden.
photo-unreadable = Couldn't read { $path }: { $error }. { $name }'s page is kept in the project, but hidden.

## The command line, in the terminal. See `decisions/2026-10-31-open-by-drop-and-argument.md`
## and `decisions/2026-11-01-manual-cache-clearing.md`.

cli-usage = Usage: { app-name } [--offline] [PROJECT.notebook] | { app-name } --clear-caches-only
cli-no-keychain-release = --no-keychain is only for development builds.
cli-not-a-project = { $path } isn't a .notebook project. { cli-usage }
cli-no-cache-dir = No cache directory on this system
cli-no-cache = No cache at { $dir }
cli-cache-cleared = Removed { $count ->
    [one] 1 cached transcript
   *[other] { $count } cached transcripts
} from { $dir }
cli-clear-failed = Couldn't clear { $dir }: { $error }

## Settings

settings-title = Settings
settings-model = Transcription model
model-opus = Opus
model-sonnet = Sonnet
# What each model is good for. From `decisions/2026-10-31-transcription-models.md`.
model-opus-tradeoffs = More accurate, and handles formatting better, but slower and more expensive. Best if you're in a hurry and want to skip proofreading, or your handwriting is terrible.
model-sonnet-tradeoffs = Not quite as accurate (but still really good). Faster, half the price.
settings-model-hint = For new transcriptions. Cached transcripts from either model are retained and used; force retranscription if you want to switch.
settings-markdown = Markdown
# The markdown style only reaches transcripts parsed from now on, and unedited ones.
settings-future-only = Future transcripts only
settings-future-only-hint = Applies to future and unedited transcripts only, so we don't clobber your edits. "{ command-reset-transcript }" on edited pages to pick up changes.
settings-underlines = Underlines
# How each mark that can wrap underlined text looks.
underline-underscores = _underscores_
underline-asterisks = *asterisks*
settings-underlined-lines = Underlined lines
# A heading level, written as markdown: $marks is one "#" per level.
settings-heading = { $marks } Heading
# The system transcript cache. See `decisions/2026-11-01-manual-cache-clearing.md`.
settings-caching = Transcript caching
settings-cache-location = Cache location
settings-cache-enable = Enable system transcript caching
settings-cache-explanation = Transcripts of OCR'd pages are cached both in .notebook project files and in your system's cache storage, so that pages and their transcripts can be reused across projects, and you don't need to retranscribe if you throw away a draft project. Switching this off won't affect .notebook project files.
settings-cache-clear = Clear cache
settings-cache-count = { $count ->
    [0] The cache is empty
    [one] 1 transcript cached
   *[other] { $count } transcripts cached
}
settings-cache-clear-failed = Couldn't clear the cache: { $error }
confirm-clear-cache = { $count ->
    [one] There is 1 transcript in the system cache currently. Delete it?
   *[other] There are { $count } transcripts in the system cache currently. Delete them?
}
confirm-cache-off = { $count ->
    [one] You've switched off system transcript caching, but there is 1 transcript in the system cache currently. Delete it?
   *[other] You've switched off system transcript caching, but there are { $count } transcripts in the system cache currently. Delete them?
}
confirm-cache-off-button = Delete
confirm-cache-off-keep = Don't delete
settings-save = Save
settings-api-key = Claude API key
settings-api-key-none = Add one to enable transcription.
# $key is masked, as "sk-ant-…a1B2".
settings-api-key-keychain = { $key }, saved in the keychain
settings-api-key-env = { $key }, from { $variable }
settings-api-key-env-note = The environment's credentials are used over a saved key while they're set, but you can still enter a key for future use.
# The login of the `ant` command line, found at launch.
settings-api-key-ant = Logged in with `ant` as { $email } ({ $org })
settings-api-key-ant-note = You can enter an API key to use that instead.
settings-api-key-enter = Enter Key…
settings-api-key-remove = Remove
settings-api-key-remove-failed = Couldn't remove the API key from the keychain: { $error }
confirm-remove-key = Remove the saved API key? The Claude Console won't show it again, so you'd need to create a new one.
confirm-remove-key-button = Remove key

## Asking for a Claude API key. See `decisions/2026-11-15-api-key-in-keychain.md`.

api-key-title = Claude API key
api-key-explanation = Transcribing sends pages to Claude, billed to your Claude Console account. Create an API key there and paste it here; it's kept in your system's keychain.
api-key-console-link = Create a key in the Claude Console
api-key-field = API key
api-key-checking = Checking the key…
api-key-save = Save Key
api-key-rejected = Claude didn't accept that key. Check that it's copied whole and hasn't been deleted.
api-key-save-failed = Couldn't save the key in the keychain: { $error }
# Saved anyway: the key may be fine, and the next transcription says if it isn't.
api-key-unverified = Saved the API key, but couldn't check it: { $error }
api-key-env-note = { $variable } is set, so it's used instead of a saved key until it's unset.
api-key-no-keychain = This system has no keychain to keep an API key in. Set ANTHROPIC_API_KEY in the environment instead.
api-key-no-keychain-why = The keychain said: { $error }
api-key-close = OK

## Transcription errors

ocr-decode = couldn't decode the page: { $error }
ocr-network = couldn't reach Claude: { $error }
# Getting a token from the `ant` command line failed; $error is what it said.
ocr-credentials = couldn't get a token from ant: { $error }. Try `ant auth login`, or enter an API key in Settings.
ocr-api = Claude API error { $status } ({ $kind }): { $message }
ocr-refused = Claude declined to transcribe this page
ocr-refused-category = Claude declined to transcribe this page ({ $category })
ocr-refused-explanation = Claude declined to transcribe this page: { $explanation }
ocr-refused-category-explanation = Claude declined to transcribe this page ({ $category }): { $explanation }
ocr-max-tokens = the transcription was cut off (too long)
ocr-parse = unexpected response from Claude: { $error }

## Photo errors

decode-unrecognized = not a JPEG, PNG or HEIC image
decode-heic = couldn't decode HEIC: { $error }
