#!/usr/bin/env bash
#
# Reproduce a deterministic Cogitator run and print its witness root.
# Re-running with the same seed and run count yields the same witness_root.txt.
#
# Usage: ./repro.sh [--seed N] [--runs N] [--out-dir DIR]

set -euo pipefail

SEED=42
RUNS=5000
OUT_DIR=out

while [[ $# -gt 0 ]]; do
  case "$1" in
    --seed)
      SEED="$2"
      shift 2
      ;;
    --runs)
      RUNS="$2"
      shift 2
      ;;
    --out-dir)
      OUT_DIR="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

cargo run --release -- run --seed "$SEED" --runs "$RUNS" --out-dir "$OUT_DIR" --clean --no-tui

echo "witness_root: $(cat "${OUT_DIR}/witness_root.txt")"
