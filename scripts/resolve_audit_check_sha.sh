#!/usr/bin/env bash
set -euo pipefail

# Resolve and print the current commit for rustsec/audit-check v2.
# Intended for release/PR hygiene when updating SHA-pinned actions.
api_url="https://api.github.com/repos/rustsec/audit-check/git/ref/tags/v2"

if ! response="$(curl -fsSL --retry 3 --retry-delay 2 "$api_url")"; then
  echo "warning: unable to resolve rustsec/audit-check@v2 via GitHub API" >&2
  exit 0
fi

sha="$(printf '%s' "$response" | sed -n 's/.*"sha"[[:space:]]*:[[:space:]]*"\([0-9a-f]\{40\}\)".*/\1/p' | head -n1)"
if [[ -z "$sha" ]]; then
  echo "warning: API response did not include a 40-char SHA" >&2
  exit 0
fi

echo "rustsec/audit-check v2 resolves to: $sha"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    echo "### rustsec/audit-check ref"
    echo
    echo "- Tag: \\`v2\\`"
    echo "- Resolved commit: \\`$sha\\`"
  } >> "$GITHUB_STEP_SUMMARY"
fi
