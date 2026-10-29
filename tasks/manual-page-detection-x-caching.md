# manual page detection x caching

- when we make a transcription response, let's store the crop corners that we used in the cache.
- then, if we're using the cache but we've transformed the image since, we can compute a transform for each line and each correction box. This is (maybe)? a little janky but I expect that post-hoc cropping adjustments will be pretty minor, so I don't think it'll look super weird,.

Please validate that this strategy is somewhat sensible before building it.
