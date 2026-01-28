# Release Notes: Cogitator v0.1.0

## What it is
Cogitator is a deterministic evaluation harness that emits cryptographic witness roots,
canonical JSON artifacts, and replayable agent traces for audit-ready experiments.

## Key commands

### Build
```bash
cargo build --release
```

### Non-agent run + verify
```bash
./target/release/cogitator run --seed 1 --runs 10 --out-dir out --clean
./target/release/cogitator verify --meta out/meta.json --trace out/trace.jsonl --expect "$(cat out/witness_root.txt)"
```

### Agent record → replay
```bash
./target/release/cogitator run --agent clawdbot --runs 1 --out-dir out --clean
./target/release/cogitator run --agent clawdbot --case 0 --replay out/run_0000 --out-dir replay --clean
./target/release/cogitator verify --witness out/run_0000
```

### Demo drift
```bash
./target/release/cogitator demo drift --seed 1 --threads 2 --fault-profile ci --out-dir demo_out --clean
./target/release/cogitator verify --witness demo_out/drift/baseline_faults
```

## Determinism guarantees
- Witness roots are stable across hardware and thread counts for identical inputs, seed,
  transcripts, and chaos schedules.
- Canonical JSON artifacts are byte-stable across runs.
- Agent record/replay uses tool transcripts to preserve determinism.

## Not guaranteed
- Live inference determinism for external models.
- Network tool determinism or external service availability.
- Timing and wall-clock latency beyond recorded provenance.
