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

## The window, before and after pages are added

drop-zone-title = Drop image files here
drop-zone-formats = JPEG, PNG or HEIC
drop-overlay = Drop to add pages
# Why the Transcribed view and exporting are unavailable.
no-transcriber-hint = Transcription needs ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN, and the desktop app
all-transcribed-hint = Every page is transcribed
# The selected page, in the toolbar.
page-position = { $name } · { $number } of { $count }
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
already-transcribed = { $name } is already transcribed. { $shortcut } retranscribes it.

## Transcription progress

transcribing-page = Transcribing { $name }…
transcribe-failed = Couldn't transcribe { $name }: { $error }
transcribe-retry = Retry
checking-cache = Looking for { $name } in the cache…
not-transcribed-yet = { $name } isn't transcribed yet.

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
transcript-lines-cut-off = { $count ->
    [one] { $count } transcribed line falls outside this page's new bounds.
   *[other] { $count } transcribed lines fall outside this page's new bounds.
}
transcript-missed-page = The bounds of this page have changed since last transcription; retranscribe to ensure you've got all text.

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
unfound-edited-out = "{ $word }" was edited out of its line, so it can't be corrected here. { command-reset-transcript } brings it back.
unfound-not-in-transcript = Claude was unsure of "{ $word }" here, but it isn't in the transcript. Double-click its line to fix it by hand.
# $alternatives are Claude's other readings, joined with " / ".
unfound-not-in-transcript-alternatives = Claude was unsure of "{ $word }" (or { $alternatives }) here, but it isn't in the transcript. Double-click its line to fix it by hand.

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
# Between what to export and the formats.
export-as = as
export-markdown-clipboard = Markdown to clipboard
export-markdown-file = Markdown to file
export-pdf = PDF
export-go = Go
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

## Settings

settings-title = Settings
settings-model = Transcription model
model-opus = Opus
model-sonnet = Sonnet
# What each model is good for. From `decisions/2026-10-24-sonnet-quality.md`.
model-opus-tradeoffs = Slower (20–45s a page), about $0.07 a page. Flags more of the words it's unsure of. Best for pages you'll correct closely.
model-sonnet-tradeoffs = 2–3× faster, about $0.03 a page, and reads words about as well. Flags fewer of its mistakes, so more are left to find by reading, and may drop separators, underlines and crossed-out words. Good for a quick first pass.
settings-model-hint = For new and forced transcriptions. Cached transcripts by either model are used; this one's is preferred.
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
settings-save = Save

## Transcription errors

ocr-decode = couldn't decode the page: { $error }
ocr-network = couldn't reach Claude: { $error }
ocr-api = Claude API error { $status } ({ $kind }): { $message }
ocr-refused = Claude declined to transcribe this page
ocr-refused-category = Claude declined to transcribe this page ({ $category })
ocr-refused-explanation = Claude declined to transcribe this page: { $explanation }
ocr-refused-category-explanation = Claude declined to transcribe this page ({ $category }): { $explanation }
ocr-max-tokens = the transcription was cut off (too long)
ocr-parse = unexpected response from Claude: { $error }

## Photo errors

decode-unrecognized = not a JPEG, PNG or HEIC image
decode-heif = couldn't decode HEIC: { $error }
