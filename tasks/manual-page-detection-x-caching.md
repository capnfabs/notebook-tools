# manual page detection x caching

- when we make a transcription response, let's store the crop corners that we used in the cache.
- then, if we're using the cache but we've transformed the image since, we can compute a transform for each line and each correction box. This is (maybe)? a little janky but I expect that post-hoc cropping adjustments will be pretty minor, so I don't think it'll look super weird. For pages that undergo _significant_ cropping changes, it will be weird - some text might be missing - if there's a way to detect this and warn about it (maybe some threshold of transformed line box area excluded from the page dimensions, _beyond_ any occurrence of this on the cached transformed page?), then it would be great to add a warning about that.

Please validate that this strategy is somewhat sensible before building it.

For already existing cache entries, assume that the crop corners are the computed default and backfill those to the cache. Add a task to remove this backfilling functionality later.
