# Manual cache clean

Today, we'll automatically delete old (unsupported) cache versions from the cache directory. But, two notes:

1. I don't think we'll bump cache versions often
2. What if I want to work on the dev version of the app, and have a prod version of the app installed at the same time? I don't want them to fight.
3. What if users want to clear or disable the cache themselves for privacy reasons?

So, let's _drop_ the automatic cache clearing, and replace it with a 'Caching' section in the Settings, which includes:

> **Transcript Caching**
> Cache location: ~/Users/fabian/Library/notebook-tools/transcript-cache
> [ ] Enable System Transcript Caching
> Transcripts of OCR'd pages are cached both in .notebook project files, and in your system's cache storage (so that it's possible to reuse pages / transcripts acros projects, and so that you don't need to retranscribe if you throw away a draft project). Switching this off won't affect .notebook project files.
>
> [Clear Cache]


Design notes:
- The 'cache location' path should be selectable / copyable.
- When you _save_ the settings and you've switched off the cache (i.e. it was set before but not now), if there's anything in the cache we should offer to clear it. "You've switched off system transcript caching, but there's N transcripts in the system cache currently. Delete them?"
- Clearing the cache is destructive, so we should show a dialog for confirmation. "There are N transcripts in the system cache currently. Delete them?"
- On both these dialogs, the action button is red, "Clear Cache / Delete".

Move the functionality to clear the caches (there's a separate program for it atm) into notebook-tools, invokable from the command line. e.g. `notebook-tools --clear-caches-only`. It clears the caches and returns immediately.
