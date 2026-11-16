# Logs when opened from Finder

Mac-only: an app opened from Finder has nowhere to write stderr, so `env_logger`'s output is lost. Decide where it goes (`~/Library/Logs/Notebook Tools/`, or `os_log`) and at what level.
