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
byte-stable artifacts so third parties can verify the same results from the same inputs and
environment.

![Rust](https://img.shields.io/badge/Rust-stable-orange?style=flat-square&logo=rust&logoColor=white)
![Deterministic](https://img.shields.io/badge/Deterministic-Yes-4c1?style=flat-square)
![Witnessed](https://img.shields.io/badge/Witnessed-Yes-6a5acd?style=flat-square)

Table of contents

Why Cogitator

Key capabilities

Quickstart

Install prerequisites

Build and run

CLI overview

Artifacts and verification

Commitment boundaries

Deterministic Simulation Testing (DST)

Verification workflow

Ordeal witness gate in CI

Nix (optional)

Project layout

Why Cogitator

Auditable agent evaluations should be reproducible by anyone, not just the original operator.

Cogitator makes runs replayable by committing trace events (and, in agent mode, tool-call
witness views) into a cryptographic witness root. Third parties can re-run the same inputs,
validate the same witness root, and pinpoint drift when something changes.

Key capabilities

Deterministic execution with explicit entropy accounting and ordered trace emission.

Parallel evaluation with stable ordering (witness hashing sorts by (run_id, step) so threads only change throughput).

Witness roots (BLAKE3) that commit to every event in a run’s canonical trace.

Deterministic agent mode with tool transcript recording + replay for byte-stable re-execution.

Drift detection + classification (tool request/outcome mismatches, count mismatches, and structured blame for ordeal).

Witness bundles that package agent traces, tool transcripts, hash chains, manifests, and per-artifact hashes for offline verification.

Hash-chain auditing for agent traces and tool calls (separate from the global witness root).

Canonical JSON artifacts (stable key ordering + formatting) to keep audit outputs byte-stable.

Atomic artifact writes to avoid partial/corrupt outputs on interruption.

DST-style fault injection with deterministic chaos profiles and witness-committed schedules.

Witness/provenance split so runtime details stay out of witness commitments while remaining discoverable.

Optional TUI: run summary + “agent observatory” views for step timelines and tool-call logs.

Set COGITATOR_PR_URL to display a PR link; press p to copy it when available.

Agents:

ordeal — deterministic task-suite agent designed for CI gating and drift diagnostics.

clawdbot — placeholder agent name for expansion.

gauntlet — legacy alias for ordeal.

Quickstart

Build:

cargo build --release


Run a deterministic evaluation:

./target/release/cogitator run --seed 42 --runs 10 --out-dir out --clean


Verify the witness root:

./target/release/cogitator verify \
  --meta out/meta.json \
  --trace out/trace.jsonl \
  --expect "$(cat out/witness_root.txt)"


PowerShell equivalent (native Windows builds use .exe and backslashes):

.\target\release\cogitator.exe run --seed 42 --runs 10 --out-dir out --clean
.\target\release\cogitator.exe verify --meta out\meta.json --trace out\trace.jsonl --expect (Get-Content out\witness_root.txt).Trim()


Optional TUI build (enables the summary cockpit / agent observatory):

cargo build --release --features tui

Install prerequisites
Linux (Debian/Ubuntu)
sudo apt-get update
sudo apt-get install -y build-essential curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Linux (Fedora/RHEL)
sudo dnf install -y gcc gcc-c++ make curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Linux (Arch)
sudo pacman -S --needed base-devel curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Linux (NixOS)

If you have flakes enabled:

nix develop


Without flakes:

nix-shell -p rustc cargo rustfmt

macOS
xcode-select --install
brew install rustup git
rustup-init

Windows

Option A: Native Windows

Install the Visual Studio Build Tools.

Install Rust via rustup.

Open a new PowerShell and verify:

rustc --version
cargo --version


Option B: WSL2 (recommended for a Linux-like workflow)

sudo apt-get update
sudo apt-get install -y build-essential curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Build and run
cargo build --release
./target/release/cogitator --help


For a clean CI-style run (no interactive UI):

./target/release/cogitator run --seed 42 --runs 100000 --out-dir out --clean --no-tui

CLI overview
Run deterministic evaluations
./target/release/cogitator run --seed 42 --runs 100 --out-dir out


Useful knobs:

--parallel true|false (default: true)

--created-at <string> (overrides provenance timestamp)

--nix-provenance auto|on|off (provenance only)

Run agent mode (record)
./target/release/cogitator run --agent ordeal --runs 1 --out-dir out --clean

Run agent mode (replay)
./target/release/cogitator run \
  --agent ordeal \
  --case 0 \
  --replay out/run_0000 \
  --out-dir replay \
  --clean


Agent/replay-only flags:

--threads <n> (affects throughput only; recorded in provenance)

--faults on|off

--fault-profile none|ci|stress

--fault-timeout-rate <f64>, --fault-drop-rate <f64>, --fault-corrupt-rate <f64>, --fault-latency-rate <f64>

--llm on|off, --llm-model <name>, --llm-seed <u64>

--pass-threshold <string> (ordeal uses a canonical string in witnessed metadata)

Demo drift

Runs a baseline and then a perturbed scenario, emitting a drift report you can verify:

./target/release/cogitator demo drift --seed 1 --threads 2 --fault-profile ci --out-dir demo_out --clean
./target/release/cogitator verify --witness demo_out/drift/baseline_faults

Artifacts and verification

A typical output layout looks like:

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


Artifact highlights

meta.json – run metadata (witnessed + provenance)

trace.jsonl – canonical trace events (NDJSON: one JSON object per line)

results.csv / results.json – case-level results

summary.json – aggregate metrics

analysis.json – bundled metadata + summary + results

witness_root.txt – witness root for the run

run_0000/* – agent witness bundle (trace + tool transcript + drift + hashes)

Note: verify expects trace.jsonl (NDJSON). agent_trace.json is a JSON array and is not accepted by verify.

Commitment boundaries

Cogitator draws a strict line between what is witnessed and what is provenance:

Witness root commits to canonical trace entries plus (in agent mode) agent trace entries and tool-call witness views in deterministic order.
Simulated latency and runtime environment details are excluded.

Provenance metadata captures run-time context (timestamps, toolchain versions, agent thread count, optional Nix details) and is not part of the witness root.

Bundle hash covers all artifacts listed in witness_manifest.json (including optional provenance artifacts) for offline verification.

Witness roots are stable across hardware and thread counts; environment details belong to provenance, not the witness commitment.

Deterministic Simulation Testing (DST)

Cogitator can deterministically inject tool faults (timeouts, corruptions, drops, and latency simulations).

Faults are driven by a seeded schedule and recorded in tool transcripts so that record + replay is byte-stable.
Simulated latency may be exposed to the agent but is excluded from witness commitments by default.

Example:

./target/release/cogitator run \
  --agent ordeal \
  --case 0 \
  --faults on \
  --fault-profile stress \
  --fault-timeout-rate 0.01 \
  --fault-corrupt-rate 0.001 \
  --fault-drop-rate 0.001

Verification workflow (no Makefile)
Non-agent runs
./target/release/cogitator run --seed 1 --runs 10 --out-dir out --clean
./target/release/cogitator verify \
  --meta out/meta.json \
  --trace out/trace.jsonl \
  --expect "$(cat out/witness_root.txt)"

Agent record → replay
./target/release/cogitator run --agent ordeal --runs 1 --out-dir out --clean

./target/release/cogitator run \
  --agent ordeal \
  --case 0 \
  --replay out/run_0000 \
  --out-dir replay \
  --clean

./target/release/cogitator verify --witness out/run_0000


verify --witness <dir> verifies:

witness_manifest.json bundle hash + per-artifact hashes

witness-root consistency

drift report integrity (and writes verify_report.json into the bundle directory)

Ordeal witness gate in CI

Cogitator includes a minimal ordeal agent case designed as a pinned CI gate.
It keeps CI costs low while still asserting a stable end-to-end witness bundle.

Run the same check locally:

scripts/check_ordeal_root.sh


The script compares the generated witness root with the golden value in
goldens/ordeal_witness_root.txt and prints drift diagnostics on mismatch.

Nix (optional)

Cogitator remains fully functional without Nix.

If you use Nix, a dev shell is provided:

nix develop


Nix metadata is captured as provenance only and never alters witness roots.
--nix-provenance=auto may resolve to off (windows) on native Windows builds while still recording that resolution in provenance.

For deterministic created_at, set SOURCE_DATE_EPOCH (the dev shell may do this automatically).

Project layout
.
├── src/            # Rust source
├── tests/          # Test suite
├── schemas/        # JSON schemas for artifacts
├── cogitator_paper.tex
└── README.md
