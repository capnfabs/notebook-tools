# Claude auth: `ant` login as a fallback

After the keychain (`decisions/2026-11-15-api-key-in-keychain.md`), use `ant auth print-credentials --access-token` if `ant` is installed and logged in. Call it per request (it refreshes the token itself). Apps launched from Finder don't get the shell's PATH, so look in `/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin` too. `--env` may also give an `ANTHROPIC_BASE_URL`, which we ignore today.

Audit and update scripts/run.sh so that it makes sense with the new token-management paradigms.
