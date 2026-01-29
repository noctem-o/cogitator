# Cogitator

Deterministic evaluation harnesses for agents with cryptographic witness roots and replayable runs.

![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust&logoColor=white)
![Deterministic](https://img.shields.io/badge/Deterministic-Yes-4c1)
![Witnessed](https://img.shields.io/badge/Witnessed-Yes-6a5acd)

## Why Cogitator

Cogitator captures full causal traces, records entropy usage (when applicable), and produces
byte-stable artifacts so third parties can verify the same results from the same inputs and
environment.

## Features

- Deterministic execution with explicit entropy accounting and ordered trace emission.
- Witness roots (BLAKE3) that commit to every event in a run’s trace.
- Agent runs with tool transcript recording and byte-stable replay.
- LLM-as-tool integration with record/replay tool calls.
- Drift detection that compares replayed tool calls against recorded transcripts.
- Witness bundles for offline verification (traces, hash chains, manifests).
- Hash-chain auditing for agent traces and tool calls.
- Canonical JSON artifacts to keep audit outputs stable across runs.
- Deterministic Simulation Testing (DST) for seeded fault injection.

## Quickstart

```bash
cargo build --release
./target/release/cogitator run --seed 42 --runs 10 --out-dir out --clean
./target/release/cogitator verify \
  --meta out/meta.json \
  --trace out/trace.jsonl \
  --expect "$(cat out/witness_root.txt)"
```

PowerShell (native Windows builds use `.exe` and backslashes):

```powershell
.\target\release\cogitator.exe run --seed 42 --runs 10 --out-dir out --clean
.\target\release\cogitator.exe verify --meta out\meta.json --trace out\trace.jsonl --expect (Get-Content out\witness_root.txt).Trim()
```

## CLI highlights

### Deterministic evaluation

```bash
./target/release/cogitator run --seed 42 --runs 100 --out-dir out
```

### Agent mode (tool transcripts)

```bash
./target/release/cogitator run --agent clawdbot --runs 1 --out-dir out
```

## Artifact layout

```
out/
├── analysis.json
├── meta.json
├── results.csv
├── results.json
├── summary.json
├── trace.jsonl
├── witness_root.txt
└── run_0000/
    ├── agent_trace.json
    ├── drift_report.json
    ├── hash_chain.txt
    ├── tool_transcript.json
    ├── witness_root.txt
    └── witness_manifest.json
```

## Verification workflow

```bash
./target/release/cogitator run --seed 1 --runs 10 --out-dir out --clean
./target/release/cogitator verify \
  --meta out/meta.json \
  --trace out/trace.jsonl \
  --expect "$(cat out/witness_root.txt)"
```

### Agent record → replay

```bash
./target/release/cogitator run --agent clawdbot --runs 1 --out-dir out --clean
./target/release/cogitator run \
  --agent clawdbot \
  --case 0 \
  --replay out/run_0000 \
  --out-dir replay \
  --clean
./target/release/cogitator verify --witness out/run_0000
```

## Build requirements

- Rust toolchain (stable)
- C toolchain for native dependencies

## Project layout

```
.
├── src/            # Rust source
├── tests/          # Test suite
├── schemas/        # JSON schemas for artifacts
├── README.md
```
