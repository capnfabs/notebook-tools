# Claude auth approach

Today credentials come from `ANTHROPIC_API_KEY` or `ANTHROPIC_AUTH_TOKEN`; `scripts/with-claude-token.sh` fills the latter from `ant auth print-credentials --access-token`. Both bill the API, not a subscription.

What I _really_ want is something as _close_ to zero-configuration for an end-user as possible.

Open: an API key in the OS keychain (`keyring`), and whether "log in with Claude.ai" in the app is possible at all (likely not for third-party apps without Anthropic approval — verify).

Done: `decisions/2026-11-15-api-key-in-keychain.md`; `ant` fallback split into `tasks/claude-auth-ant.md`.
