# Cogitator

Deterministic evaluation harnesses, cryptographic witness roots, and replayable agent runs.
Cogitator captures full causal traces, records entropy usage (when applicable), and produces
byte-stable artifacts so third parties can verify the same results from the same inputs and
environment.

![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust&logoColor=white)
![Deterministic](https://img.shields.io/badge/Deterministic-Yes-4c1)
![Witnessed](https://img.shields.io/badge/Witnessed-Yes-6a5acd)

## Table of contents

- [Why Cogitator](#why-cogitator)
- [Key capabilities](#key-capabilities)
- [Quickstart](#quickstart)
- [Install prerequisites](#install-prerequisites)
- [Build and run](#build-and-run)
- [CLI overview](#cli-overview)
- [Artifacts and verification](#artifacts-and-verification)
- [Commitment boundaries](#commitment-boundaries)
- [Deterministic Simulation Testing (DST)](#deterministic-simulation-testing-dst)
- [Project layout](#project-layout)

---

## Why Cogitator

Auditable agent evaluations should be reproducible by anyone, not just the original
operator. Cogitator makes runs replayable by committing every trace event into a
cryptographic witness root. This allows third parties to re-run the same inputs and
validate the exact same witness root, even across different machines.

## Key capabilities

- **Deterministic execution** with explicit entropy accounting and ordered trace emission.
- **Witness roots (BLAKE3)** that commit to every event in a run’s trace.
- **Deterministic agent mode** with tool transcript recording + replay for byte-stable
  re-execution.
- **LLM-as-tool integration** where inference is just another tool call; live mode records
  responses and replay reuses them.
- **Drift detection** that compares replayed tool calls against recorded transcripts and
  emits machine-readable drift reports.
- **Witness bundles** that package agent traces, tool transcripts, hash chains, and
  manifests for offline verification workflows.
- **Hash-chain auditing** for agent traces and tool calls, separate from the global witness
  root.
- **Reproducible run metadata** (seed, run counts, parallel strategy, provenance).
- **DST-style fault injection** with deterministic chaos testing and witness-committed
  schedules.
- **Witness/provenance split** so runtime details stay out of witness commitments while
  remaining discoverable.
- **Canonical JSON artifacts** to keep audit outputs byte-stable across runs.

---

## Quickstart

```bash
cargo build --release
./target/release/cogitator run --seed 42 --runs 10 --out-dir out --clean
./target/release/cogitator verify \
  --meta out/meta.json \
  --trace out/trace.jsonl \
  --expect "$(cat out/witness_root.txt)"
```

## Install prerequisites

### Linux (Debian/Ubuntu)

```bash
sudo apt-get update
sudo apt-get install -y build-essential curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Linux (Fedora/RHEL)

```bash
sudo dnf install -y gcc gcc-c++ make curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Linux (Arch)

```bash
sudo pacman -S --needed base-devel curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### macOS

```bash
xcode-select --install
brew install rustup git
rustup-init
```

### Windows

**Option A: Native Windows**

1. Install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/).
2. Install Rust via [rustup](https://rustup.rs/).
3. Open a new PowerShell and verify:

```powershell
rustc --version
cargo --version
```

**Option B: WSL2 (recommended for a Linux-like workflow)**

```bash
sudo apt-get update
sudo apt-get install -y build-essential curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

---

## Build and run

```bash
cargo build --release
./target/release/cogitator --help
```

## CLI overview

### Run deterministic evaluations

```bash
./target/release/cogitator run --seed 42 --runs 100 --out-dir out
```

### Run agent mode (with tool transcripts)

```bash
./target/release/cogitator run --agent clawdbot --runs 1 --out-dir out
```

Agent-only flags such as `--threads` and `--fault-*` are rejected in non-agent runs.

---

## Artifacts and verification

A typical output layout looks like:

```
out/
├── analysis.json
├── meta.json
├── nix_provenance.json
├── results.csv
├── results.json
├── summary.json
├── trace.jsonl
├── witness_root.txt
└── run_0000/
    ├── agent_trace.json
    ├── chaos_profile.json
    ├── drift_report.json
    ├── hash_chain.txt
    ├── tool_transcript.json
    ├── witness_root.txt
    └── witness_manifest.json
```

**Artifact highlights**

- `meta.json` – run metadata (witnessed + provenance)
- `trace.jsonl` – canonical trace events
- `results.csv` / `results.json` – case-level results
- `summary.json` – aggregate metrics
- `analysis.json` – bundled metadata + summary + results
- `witness_root.txt` – final witness root for the run
- `nix_provenance.json` – optional Nix metadata (provenance only)

---

## Commitment boundaries

Cogitator draws a strict line between what is **witnessed** and what is **provenance**:

- **Witness root** commits to canonical trace entries plus agent traces + tool call witness
  views in deterministic order (agent step, then tool calls by `tool_call_idx`). Simulated
  latency and runtime environment details are excluded.
- **Provenance metadata** captures run-time context (timestamps, toolchain versions, agent
  thread count, optional Nix details) and is **not** part of the witness root.
- **Bundle hash** covers all artifacts listed in the witness manifest (including optional
  provenance artifacts like `nix_provenance.json`) for offline verification.

Witness roots are stable across hardware and thread counts; environment details belong to
provenance, not the witness commitment.

---

## Deterministic Simulation Testing (DST)

Cogitator can deterministically inject tool faults (timeouts, corruptions, drops, and
latency simulations). Faults are driven by a seeded schedule and recorded in tool
transcripts so that record + replay is byte-stable. Simulated latency is exposed to the
agent but excluded from witness commitments by default. Fault selection uses a single
seeded draw per tool call, with cumulative per-million weights applied in a fixed
priority order (timeout → drop → corrupt → latency_sim). The total fault probability is
the sum of configured rates, capped at 1,000,000 per million.

Example:

```bash
./target/release/cogitator run \
  --agent clawdbot \
  --case 0 \
  --faults on \
  --fault-profile stress \
  --fault-timeout-rate 0.01 \
  --fault-corrupt-rate 0.001 \
  --fault-drop-rate 0.001
```

---

## Verification workflow (no Makefile)

Use the release binary to reproduce runs and verify witnesses:

### Non-agent runs

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

### Demo drift (baseline verify)

```bash
./target/release/cogitator demo drift \
  --seed 1 \
  --threads 2 \
  --fault-profile ci \
  --out-dir demo_out \
  --clean
./target/release/cogitator verify --witness demo_out/drift/baseline_faults
```

---

## Nix (optional)

If you use Nix, a dev shell is provided but not required for normal builds:

```bash
nix develop
```

Cogitator remains fully functional without Nix; any Nix provenance data is recorded only
in provenance artifacts and never alters witness roots.

---

## Project layout

```
.
├── src/            # Rust source
├── tests/          # Test suite
├── schemas/        # JSON schemas for artifacts
├── cogitator_paper.tex
└── README.md
```

---

If you build on Cogitator, please cite the project and include the witness root in any
reported results so others can independently verify your run.
