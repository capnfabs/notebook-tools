# Cache housekeeping

On startup the app prunes cache entries from versions before `CACHE_VERSION`, on a background thread, logging how many went. Newer versions' entries are kept, so running an older build (another branch, say) doesn't throw away transcriptions a newer one paid for; files that aren't entries are left alone. Entries are written as formatted JSON, for reading by hand. Corners are required: the backfill for entries stored before they were is gone, so such an entry fails to parse and its page is fetched again.

Reason: Human Taste (when to prune, older versions only, formatted JSON); unquestioned agent recommendation for the rest.
