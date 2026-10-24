# Cache status and forced retranscription

Cache entries record when they were stored (`stored_at`, unix seconds, required). Entries without it are unreadable and get fetched again; none existed outside the developer's machine, so there's no version bump. The time shows in local time via `jiff` (std has no time zones; jiff was already in the tree via env_logger). "Force retranscription" asks first only if the page has corrections, shows progress in place of the transcript, and replaces the cache entry only if the new answer parses: on failure the old transcript, corrections and all, comes back with the error.

Reason: Human Taste (stored time, hiding the old transcript, keeping the cache on failure); agent recommendation accepted for the rest.
