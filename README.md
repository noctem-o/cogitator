```text
   ██████╗  ██████╗  ██████╗ ██╗████████╗ █████╗ ████████╗ ██████╗ ██████╗
  ██╔════╝ ██╔═══██╗ ██╔════╝ ██║╚══██╔══╝██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗
  ██║      ██║   ██║ ██║  ███╗██║   ██║   ███████║   ██║   ██║   ██║██████╔╝
  ██║      ██║   ██║ ██║   ██║██║   ██║   ██╔══██║   ██║   ██║   ██║██╔══██╗
  ╚██████╗ ╚██████╔╝ ╚██████╔╝██║   ██║   ██║  ██║   ██║   ╚██████╔╝██║  ██║
   ╚═════╝  ╚═════╝   ╚═════╝ ╚═╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝  ╚═╝
```

Deterministic evaluation harnesses, cryptographic witness roots, and replayable agent runs.

Cogitator captures full causal traces, records entropy usage (when applicable), and produces
byte-stable artifacts so third parties can reproduce and verify the same results from the
same inputs and environment constraints.

![Rust](https://img.shields.io/badge/Rust-stable-orange?style=flat-square&logo=rust&logoColor=white)
![Deterministic](https://img.shields.io/badge/Deterministic-Yes-4c1?style=flat-square)
![Witnessed](https://img.shields.io/badge/Witnessed-Yes-6a5acd?style=flat-square)

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
- [Verification workflow (no Makefile)](#verification-workflow-no-makefile)
- [Ordeal witness gate in CI](#ordeal-witness-gate-in-ci)
- [Nix (optional)](#nix-optional)
- [Project layout](#project-layout)

---

## Why Cogitator

Auditable agent evaluations should be reproducible by anyone, not just the original operator.

Cogitator makes runs replayable by committing trace events (and, in agent mode, tool-call
witness views) into a cryptographic witness root. Third parties can re-run the same inputs,
validate the same witness root, and pinpoint drift when something changes.

---

## Key capabilities

- **Deterministic execution** with explicit entropy accounting and ordered trace emission.
- **Parallel evaluation** with stable ordering (threads change throughput, not witness semantics).
- **Witness roots (BLAKE3)** that commit to every event in a run’s canonical trace.
- **Deterministic agent mode** with tool transcript recording + replay for byte-stable re-execution.
- **Drift detection + classification** (tool request/outcome mismatches, count mismatches, and structured blame for `ordeal`).
- **Witness bundles** that package agent traces, tool transcripts, hash chains, manifests, and per-artifact hashes for offline verification.
- **Hash-chain auditing** for agent traces and tool calls (separate from the global witness root).
- **Canonical JSON artifacts** (stable key ordering + formatting) to keep audit outputs byte-stable.
- **Atomic artifact writes** to avoid partial/corrupt outputs on interruption.
- **DST-style fault injection** with deterministic chaos profiles and witness-committed schedules.
- **Witness/provenance split** so runtime details stay out of witness commitments while remaining discoverable.
- **Optional TUI**: run summary + “agent observatory” views for step timelines and tool-call logs.
  - Set `COGITATOR_PR_URL` to display a PR link; press `p` to copy it when available.

Agents:
- `ordeal` — deterministic task-suite agent designed for CI gating and drift diagnostics.
- `openclaw` — placeholder agent name for expansion.

---

## Quickstart

Build:

```bash
cargo build --release
```

Run a deterministic evaluation:

```bash
./target/release/cogitator run --seed 42 --runs 10 --out-dir out --clean
```

Verify the witness root:

```bash
./target/release/cogitator verify \
  --meta out/meta.json \
  --trace out/trace.jsonl \
  --expect "$(cat out/witness_root.txt)"
```

PowerShell equivalent (native Windows builds use `.exe` and backslashes):

```powershell
.\target\release\cogitator.exe run --seed 42 --runs 10 --out-dir out --clean
.\target\release\cogitator.exe verify --meta out\meta.json --trace out\trace.jsonl --expect (Get-Content out\witness_root.txt).Trim()
```

Optional TUI build:

```bash
cargo build --release --features tui
```

---

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

### Linux (NixOS)

If you have flakes enabled:

```bash
nix develop
```

Without flakes:

```bash
nix-shell -p rustc cargo rustfmt
```

### macOS

```bash
xcode-select --install
brew install rustup git
rustup-init
```

### Windows

**Option A: Native Windows**

1. Install the Visual Studio Build Tools.
2. Install Rust via rustup.
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

For a clean CI-style run (no interactive UI):

```bash
./target/release/cogitator run --seed 42 --runs 100000 --out-dir out --clean --no-tui
```

---

## CLI overview

### Run deterministic evaluations

```bash
./target/release/cogitator run --seed 42 --runs 100 --out-dir out
```

Useful toggles (exact flags may vary by build):

- `--parallel true|false`
- `--created-at <string>` (provenance override)
- `--nix-provenance auto|on|off` (provenance only)

### Run agent mode (record)

```bash
./target/release/cogitator run --agent ordeal --runs 1 --out-dir out --clean
```

### Run agent mode (replay)

```bash
./target/release/cogitator run \
  --agent ordeal \
  --case 0 \
  --replay out/run_0000 \
  --out-dir replay \
  --clean
```

Agent/replay-only toggles:

- `--threads <n>` (affects throughput only; recorded in provenance)
- `--faults on|off`
- `--fault-profile none|ci|stress`
- `--fault-timeout-rate <f64>`, `--fault-drop-rate <f64>`, `--fault-corrupt-rate <f64>`, `--fault-latency-rate <f64>`
- `--llm on|off`, `--llm-model <name>`, `--llm-seed <u64>`
- `--pass-threshold <string>` (ordeal uses a canonical string in witnessed metadata)

### Demo drift

Runs a baseline and then a perturbed scenario, emitting a drift report you can verify:

```bash
./target/release/cogitator demo drift --seed 1 --threads 2 --fault-profile ci --out-dir demo_out --clean
./target/release/cogitator verify --witness demo_out/drift/baseline_faults
./target/release/cogitator verify --witness demo_out/drift/baseline_faults --recompute-witness-root
```

---

## Artifacts and verification

A typical output layout looks like:

```text
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
    ├── witness_manifest.json
    └── verify_report.json            (written by `verify --witness <dir>`)
```

**Artifact highlights**

- `meta.json` – run metadata (witnessed + provenance)
- `trace.jsonl` – canonical trace events (NDJSON: one JSON object per line; strict JSON parser rejects duplicate keys and non-integer numbers)
- `results.csv` / `results.json` – case-level results
- `summary.json` – aggregate metrics
- `analysis.json` – bundled metadata + summary + results
- `witness_root.txt` – witness root for the run
- `run_0000/*` – agent witness bundle (trace + tool transcript + drift + hashes)

Notes:
- `verify` expects `trace.jsonl` (NDJSON). `agent_trace.json` is not accepted by `verify`.
- In agent mode, the bundle directory is the unit of verification.

---

## Commitment boundaries

Cogitator draws a strict line between what is **witnessed** and what is **provenance**:

- **Witness root** commits to RFC 8785-style canonical JSON (JCS key ordering) over a strict I-JSON subset (integers only) for trace entries plus (in agent mode) agent trace entries and tool-call witness views in deterministic order.
  Simulated latency and runtime environment details are excluded.
  Tool-call commitments are computed from explicit witness-view types (not full transcript structs), so provenance-only fields cannot be accidentally pulled into witnessed bytes.
- **Provenance metadata** captures run-time context (timestamps, toolchain versions, agent thread count, platform notes, optional Nix details) and is **not** part of the witness root.
- **Bundle hash** covers all artifacts listed in `witness_manifest.json` (including optional provenance artifacts) for offline verification.

Witness roots are stable across hardware and thread counts; environment details belong to provenance, not the witness commitment.

---

## Deterministic Simulation Testing (DST)

Cogitator can deterministically inject tool faults (timeouts, corruptions, drops, and latency simulations).

Faults are driven by a seeded schedule and recorded in tool transcripts so that record + replay is byte-stable.
Simulated latency may be exposed to the agent but is excluded from witness commitments by default.

Example:

```bash
./target/release/cogitator run \
  --agent ordeal \
  --case 0 \
  --faults on \
  --fault-profile stress \
  --fault-timeout-rate 0.01 \
  --fault-corrupt-rate 0.001 \
  --fault-drop-rate 0.001
```

---

## Verification workflow (no Makefile)

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
./target/release/cogitator run --agent ordeal --runs 1 --out-dir out --clean

./target/release/cogitator run \
  --agent ordeal \
  --case 0 \
  --replay out/run_0000 \
  --out-dir replay \
  --clean

./target/release/cogitator verify --witness out/run_0000
```

`verify --witness <dir>` verifies:
- `witness_manifest.json` bundle hash + per-artifact hashes
- witness-root consistency
- drift report integrity (and writes `verify_report.json` into the bundle directory)

Use semantic witness recompute mode to validate the committed root against bundle semantics:

```bash
./target/release/cogitator verify --witness out/run_0000 --recompute-witness-root
```

On mismatch, Cogitator prints expected vs computed root and a committed-component hint.

---

## Ordeal witness gate in CI

Use one deterministic command for golden-root drift checks:

```bash
./target/release/cogitator ordeal check --golden goldens/ordeal_witness_root.txt
# Intentional witness change:
./target/release/cogitator ordeal check --golden goldens/ordeal_witness_root.txt --update-golden
```


Cogitator includes a minimal `ordeal` agent case designed as a pinned CI gate.
It keeps CI costs low while still asserting a stable end-to-end witness bundle.

Run the same check locally:

```bash
scripts/check_ordeal_root.sh
```

The script compares the generated witness root with the golden value in
`goldens/ordeal_witness_root.txt` and prints drift diagnostics on mismatch.

---

## Nix (optional)

Cogitator remains fully functional without Nix.

If you use Nix, a dev shell is provided:

```bash
nix develop
```

Nix metadata is captured as **provenance only** and never alters witness roots.
On native Windows builds, `--nix-provenance=auto` may resolve to “off (windows)” while still recording the resolution in provenance.

For deterministic `created_at`, set `SOURCE_DATE_EPOCH` (the dev shell may do this automatically).

---

## Project layout

```text
.
├── src/            # Rust source
├── tests/          # Test suite
├── schemas/        # JSON schemas for artifacts
├── scripts/        # helper scripts (CI gates, local checks)
├── goldens/        # pinned witness roots for CI
└── README.md
```
