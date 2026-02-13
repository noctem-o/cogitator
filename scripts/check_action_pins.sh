#!/usr/bin/env bash
set -euo pipefail

workflows_dir=".github/workflows"
if [[ ! -d "$workflows_dir" ]]; then
  echo "error: ${workflows_dir} does not exist" >&2
  exit 1
fi

# Repos for which version tags (e.g. @v2) are forbidden in CI workflows.
FORBID_TAG_REPOS=(
  "rustsec/audit-check"
  "actions/checkout"
  "actions/upload-artifact"
  "actions/attest-build-provenance"
)

failed=0

for repo in "${FORBID_TAG_REPOS[@]}"; do
  # Match "uses:" lines that reference the configured repo with a v-style tag.
  pattern="^[[:space:]]*uses:[[:space:]]*${repo}@v[0-9A-Za-z._-]+([[:space:]]*(#.*)?)?$"
  while IFS=: read -r file line text; do
    [[ -n "${file:-}" ]] || continue
    failed=1
    echo "pin policy violation: ${file}:${line}" >&2
    echo "  offending line: ${text}" >&2
    echo "  suggested fix: uses: ${repo}@<40-hex-commit-sha> # vX.Y.Z" >&2
  done < <(rg -n --no-heading "$pattern" "$workflows_dir"/*.yml || true)
done

if (( failed )); then
  echo >&2
  echo "One or more workflow actions are pinned to tags instead of immutable commit SHAs." >&2
  echo "Resolve tags in a PR with scripts/resolve_action_sha.sh; never resolve tags at runtime in CI." >&2
  exit 1
fi

echo "All configured workflow actions are pinned to commit SHAs."
