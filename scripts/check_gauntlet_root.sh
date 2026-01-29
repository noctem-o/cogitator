#!/usr/bin/env bash
set -euo pipefail

# Always run relative to repo root (works from CI or anywhere)
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}/.."

BIN="./target/release/cogitator"
OUT_DIR="out_ci"
RUN_DIR="${OUT_DIR}/run_0000"
GOLDEN="goldens/gauntlet_witness_root.txt"

cargo build --release --locked

rm -rf "$OUT_DIR"
RUN_FLAGS=(
  --agent gauntlet
  --seed 42
  --runs 1
  --case 0
  --out-dir "$OUT_DIR"
  --clean
  --no-tui
  --faults off
  --fault-profile none
  --pass-threshold 0.5
  --parallel false
  --nix-provenance off
)
"$BIN" run "${RUN_FLAGS[@]}"

ROOT="$(tr -d '\r\n' < "${RUN_DIR}/witness_root.txt")"

if [[ ! -f "$GOLDEN" ]]; then
  printf '%s\n' "$ROOT" > "$GOLDEN"
  echo "Missing golden file: $GOLDEN"
  echo "Initialized golden from ${RUN_DIR}/witness_root.txt"
  exit 2
fi

EXPECTED="$(tr -d '\r\n' < "$GOLDEN")"

if [[ "$ROOT" != "$EXPECTED" ]]; then
  echo "Witness root changed!"
  printf 'expected: %q\n' "$EXPECTED"
  printf 'actual:   %q\n' "$ROOT"
  echo
  echo "sha256sum:"
  if [[ -f "$GOLDEN" ]]; then
    sha256sum "$GOLDEN"
  else
    echo "missing $GOLDEN"
  fi
  if [[ -f "${RUN_DIR}/witness_root.txt" ]]; then
    sha256sum "${RUN_DIR}/witness_root.txt"
  else
    echo "missing ${RUN_DIR}/witness_root.txt"
  fi
  echo
  echo "ls -la goldens:"
  ls -la goldens
  echo
  echo "ls -la ${RUN_DIR}:"
  ls -la "${RUN_DIR}"
  echo
  echo "meta.json:"
  if [[ -f "${RUN_DIR}/meta.json" ]]; then
    cat "${RUN_DIR}/meta.json"
  else
    echo "missing ${RUN_DIR}/meta.json"
  fi
  echo
  echo "chaos_profile.json:"
  if [[ -f "${RUN_DIR}/chaos_profile.json" ]]; then
    cat "${RUN_DIR}/chaos_profile.json"
  else
    echo "missing ${RUN_DIR}/chaos_profile.json"
  fi
  echo
  exit 1
fi

echo "OK: witness root matches $ROOT"
