# Claude auth: `ant` login as a fallback

After the keychain (`decisions/2026-11-15-api-key-in-keychain.md`), use `ant auth print-credentials --access-token` if `ant` is installed and logged in.

Apps launched from Finder don't get the shell's PATH, so look in `/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin` too. `--env` may also give an `ANTHROPIC_BASE_URL`, which we ignore today.

Audit and update scripts/run.sh so that it makes sense with the new token-management paradigms.

Priority order should be:
- explicit key env variable (if set)
- explicitly set key in settings
- `ant` command.

Only attempt with the _first_ detected key, don't do anything like "try to fallback to the first working key", which feels like a recipe for spending money from the wrong account.

Open questions:

- Call it per request, or once on startup? Probs per-request w/ some caching, or on startup but refresh if it errors, but I want to talk about it a little.

Done: `decisions/2026-11-15-ant-login-fallback.md`. Cached until near expiry; dev runs skip the keychain with `--no-keychain`.
