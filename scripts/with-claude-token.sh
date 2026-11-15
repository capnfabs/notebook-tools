#!/usr/bin/env bash
# For the live core tests, which only read the environment (the app finds `ant` itself): runs a
# command with a fresh Claude access token from `ant` in ANTHROPIC_AUTH_TOKEN, and no
# ANTHROPIC_API_KEY (which would win), so requests bill your `ant auth login` account.
set -euo pipefail

# Fetched each run because access tokens expire.
if ! token=$(ant auth print-credentials --access-token); then
  echo "Couldn't get a token from ant; try \`ant auth login\`." >&2
  exit 1
fi

unset ANTHROPIC_API_KEY
ANTHROPIC_AUTH_TOKEN="$token" exec "$@"
