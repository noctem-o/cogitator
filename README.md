# Cogitator

Cogitator is a deterministic evaluation harness with cryptographic witness roots that make
agent runs replayable, auditable, and verifiable. Cogitator makes agent behavior
reproducible the way git makes code reproducible. It captures full causal traces, tracks
entropy usage where applicable, and packages run artifacts so that third parties can
recompute the same witness root from the same inputs and environment.

## What’s new in this repo

This implementation expands on the original paper with additional operational features:

- **Agent-mode execution** with deterministic tool transcripts and replay support.
- **Drift detection** that compares replayed tool calls against recorded transcripts.
- **Witness bundles** that package agent traces, tool transcripts, and hash chains for
  independent verification.
- **Hash-chain auditing** for agent traces + tool calls, separate from the main witness root.
- **Optional TUI** for inspecting run summaries and agent traces (feature-flagged).

## Key capabilities

- **Deterministic execution** with explicit entropy accounting (where applicable) and ordered
  trace emission.
- **Witness roots** (BLAKE3) that commit to every event in a run’s trace.
- **Reproducible run metadata** capturing seed, run counts, parallel strategy, and provenance.
- **Artifact manifests** for programmatic consumption of outputs.

## CLI overview

Build and run:

```bash
cargo build
./target/debug/cogitator --help
```

### Run deterministic evaluations

```bash
./target/debug/cogitator run --seed 42 --runs 100 --out-dir out
```

Outputs include:

- `meta.json` – run metadata (witnessed + provenance)
- `trace.jsonl` – canonical trace events
- `results.csv` / `results.json` – case-level results
- `summary.json` – aggregate metrics
- `analysis.json` – bundled metadata + summary + results
- `witness_root.txt` – final witness root for the run

A typical output layout looks like:

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
    └── witness_manifest.json
```

### Run agent mode (with tool transcripts)

```bash
./target/debug/cogitator run --agent clawdbot --runs 1 --out-dir out
```

Agent-mode produces a per-run directory (`out/run_0000/`) with:

- `agent_trace.json` – agent decisions per step
- `tool_transcript.json` – tool calls and deterministic stub outputs
- `hash_chain.txt` – chained hashes over agent traces + tool calls
- `drift_report.json` – drift status and mismatches
- `witness_manifest.json` – pointers to all per-run artifacts

The witness root commits to the run globally; the per-run hash chain provides local,
step-by-step provenance for drift analysis.

### Replay an agent run

```bash
./target/debug/cogitator run --agent clawdbot --case 0 --replay out/run_0000 --out-dir replay
```

Replay reuses the prior tool transcript and emits a drift report showing any deviations.

### Verify witness roots

Verify a trace against an expected witness root:

```bash
./target/debug/cogitator verify --meta out/meta.json --trace out/trace.jsonl --expect <root>
```

Verify a witness bundle (agent mode):

```bash
./target/debug/cogitator verify --witness out/run_0000
```

## TUI support

The TUI is feature-gated. Enable it with:

```bash
cargo run --features tui -- run --runs 10
```

Use `--no-tui` to suppress the interface when running in CI or headless contexts.

## Project layout

- `src/main.rs` – CLI entrypoint and artifact orchestration
- `src/eval.rs` – deterministic evaluation harness
- `src/witness.rs` – witness root builder
- `src/verify.rs` – trace verification
- `src/agent.rs` – example deterministic agent implementation
- `src/tooling.rs` – tool transcript recording/replay
- `src/drift.rs` – drift detection + witness bundle verification
- `src/tui.rs` – terminal UI (feature gated)

## License

MIT
