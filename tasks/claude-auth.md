# Claude auth approach

Today credentials come from `ANTHROPIC_API_KEY` or `ANTHROPIC_AUTH_TOKEN`; `scripts/with-claude-token.sh` fills the latter from `ant auth print-credentials --access-token`. Both bill the API, not a subscription.

Open: an API key in the OS keychain (`keyring`), and whether "log in with Claude.ai" in the app is possible at all (likely not for third-party apps without Anthropic approval — verify).
