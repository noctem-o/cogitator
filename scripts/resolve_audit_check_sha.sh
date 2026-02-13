#!/usr/bin/env bash
set -euo pipefail

# Deprecated helper kept for compatibility.
# Prefer: ./scripts/resolve_action_sha.sh rustsec/audit-check v2.0.0
exec "$(dirname "$0")/resolve_action_sha.sh" rustsec/audit-check v2.0.0
