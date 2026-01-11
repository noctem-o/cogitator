#!/bin/bash
SEED=${1:-42}
RUNS=${2:-5000}
RUST_LOG=info cargo run --release -- --seed $SEED --runs $RUNS
