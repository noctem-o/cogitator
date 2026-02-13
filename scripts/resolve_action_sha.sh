#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "usage: $0 <org/repo> <tag-or-ref> [uses-keyword]" >&2
  echo "example: $0 rustsec/audit-check v2.0.0" >&2
  exit 2
fi

repo="$1"
ref="$2"
uses_keyword="${3:-uses:}"

if [[ ! "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  echo "error: repo must be in org/repo format" >&2
  exit 2
fi

remote_url="https://github.com/${repo}.git"

# Try annotated-tag dereference first.
sha="$( (git ls-remote "$remote_url" "refs/tags/${ref}^{}" 2>/dev/null || true) | awk 'NR==1{print $1}')"
if [[ -z "$sha" ]]; then
  # Fallback to direct tag or branch/ref name.
  sha="$( (git ls-remote "$remote_url" "refs/tags/${ref}" "$ref" 2>/dev/null || true) | awk 'NR==1{print $1}')"
fi

if [[ -z "$sha" || ! "$sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo "error: unable to resolve ${repo}@${ref} to a commit SHA" >&2
  exit 1
fi

echo "repo=${repo}"
echo "ref=${ref}"
echo "sha=${sha}"
echo "${uses_keyword} ${repo}@${sha} # ${ref}"
