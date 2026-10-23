# Claude auth approach

Spec wants "log in with Claude.ai". Likely not permitted for third-party apps without Anthropic approval — verify.

Fallbacks: API key in OS keychain (`keyring`); reuse `ant auth login` Console profile via `ant auth print-credentials --access-token` (Bearer + `anthropic-beta: oauth-2025-04-20`). Both bill the API, not a subscription.
