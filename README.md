```
   ██████╗  ██████╗  ██████╗  ██╗████████╗ █████╗ ████████╗ ██████╗ ██████╗
  ██╔════╝ ██╔═══██╗ ██╔════╝ ██║╚══██╔══╝██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗
  ██║      ██║   ██║ ██║  ███╗██║   ██║   ███████║   ██║   ██║   ██║██████╔╝
  ██║      ██║   ██║ ██║   ██║██║   ██║   ██╔══██║   ██║   ██║   ██║██╔══██╗
  ╚██████╗ ╚██████╔╝ ╚██████╔╝██║   ██║   ██║  ██║   ██║   ╚██████╔╝██║  ██║
   ╚═════╝  ╚═════╝   ╚═════╝ ╚═╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝  ╚═╝
```

> **Tamper-evident AI agent audit harness with a cryptographic witness chain, pre-call policy interception, and byte-stable replay. Written in Rust.**

[![CI](https://img.shields.io/github/actions/workflow/status/noctem-o/COGITATOR/ci.yml?branch=main&label=CI&style=flat-square)](https://github.com/noctem-o/COGITATOR/actions)
[![License: BUSL-1.1](https://img.shields.io/badge/license-BUSL--1.1-green.svg?style=flat-square)](https://github.com/noctem-o/COGITATOR/blob/main/LICENSE)
[![Spec: Apache-2.0](https://img.shields.io/badge/spec-Apache--2.0-blue.svg?style=flat-square)](https://github.com/noctem-o/COGITATOR/blob/main/spec/COGITATOR_WITNESS_PROTOCOL.md)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg?style=flat-square)](https://www.rust-lang.org)
[![IETF Draft](https://img.shields.io/badge/IETF-draft--noctem--cogitator--witness--protocol--00-lightgrey?style=flat-square)](https://datatracker.ietf.org/wg/scitt/about/)

-----

> 📄 **[Companion paper](https://github.com/noctem-o/COGITATOR/blob/main/paper/main.pdf)** covers the cryptographic commitment model (BLAKE3 domain-separated hash chains, JCS/RFC 8785 canonical JSON), the witness/provenance split, DST fault-injection design, and reproducibility guarantees. It documents the v1.0 architecture; v2.0 builds pre-call policy interception on top of that foundation.

-----

## Table of Contents

- [The Problem](#the-problem)
- [Quick Start](#quick-start)
- [Architecture](#architecture)
- [v2.0: Pre-Call Policy Interception](#v20-pre-call-policy-interception)
- [Artifact Layout](#artifact-layout)
- [Deterministic Simulation Testing (DST)](#deterministic-simulation-testing-dst)
- [Verification Workflow](#verification-workflow)
- [Ordeal CI Gate](#ordeal-ci-gate)
- [CLI Reference](#cli-reference)
- [Install Prerequisites](#install-prerequisites)
- [Why This Matters Now](#why-this-matters-now)
- [Research Directions](#research-directions)
- [Protocol Specification](#protocol-specification)
- [Project Layout](#project-layout)
- [Release Engineering](#release-engineering)
- [Nix](#nix)
- [Contributing](#contributing)
- [Contact](#contact)
- [License](#license)

-----

## The Problem

AI agents are being deployed into regulated environments like financial trading, medical triage, legal research, and critical infrastructure, where tool calls have real-world consequences. The current state of the art offers almost no verifiable answer to the three questions that actually matter:

1. **Did the agent do exactly what the log says it did?** Logs are mutable. Post-hoc summaries are reconstructions. There is no standard way for a third party to verify an operator’s account of what their agent actually did.
1. **Was the agent operating within its sanctioned boundaries at the time of each call?** Runtime policy is typically advisory, unenforced, or checked after dispatch. By that point the side effect has already happened.
1. **Can an independent auditor reproduce the same result from the same inputs?** Without deterministic replay, audit is interpretation, not verification. “We reviewed the logs” is not the same as “we re-ran the execution and got the same witness root.”

COGITATOR is a direct engineering answer to all three. Every run produces a cryptographic witness root, a BLAKE3 hash chain over RFC 8785 canonical JSON, that any third party can independently recompute to confirm the record was not altered after the fact. As of v2.0, COGITATOR also intercepts every tool call *before* dispatch, evaluates it against a declarative policy, and records the decision as an auditable artifact committed into the witness chain.

-----

## Quick Start

```bash
# Build (or grab the release binary from the Releases page)
cargo build --release

# Run a single agent evaluation with policy enforcement.
# Produces a full witness bundle at out/run_0000/
cargo run --release -- run --agent clawdbot --policy fixtures/policy_clawdbot_block.toml

# Verify the witness root of that run from scratch
cargo run --release -- verify --recompute-witness-root --witness out/run_0000 \
  --expect "$(cat out/run_0000/witness_root.txt)"

# Replay the same run -- you should get the identical witness root
cargo run --release -- run --replay out/run_0000
```

**End-to-end demo (no policy file needed):**

```bash
# Build, run a deterministic drift demo, verify the bundle
cargo build --release
./target/release/cogitator demo drift --seed 42 --threads 1 --fault-profile stress --out-dir demo_out --clean
./target/release/cogitator verify --witness demo_out/drift/baseline_faults
```

**PowerShell:**

```powershell
cargo build --release
.\target\release\cogitator.exe demo drift --seed 42 --threads 1 --fault-profile stress --out-dir demo_out --clean
.\target\release\cogitator.exe verify --witness demo_out\drift\baseline_faults
```

**Optional TUI build** (neon/cyan/mono themes):

```bash
cargo build --release --features tui
./target/release/cogitator run --agent ordeal --runs 1 --theme neon
./target/release/cogitator run --agent ordeal --runs 1 --theme cyan --no-color
```

-----

## Architecture

```
+------------------------------------------------------------------+
|                          Agent Loop                              |
|   AgentInput -> Agent::step() -> AgentOutput (tool_requests)     |
+----------------------------+-------------------------------------+
                             |  tool_requests
                             v
+------------------------------------------------------------------+
|               ToolTranscript::execute()    <-- v2.0 INTERCEPT    |
|                                                                  |
|  +-------------------------------------------------------------+ |
|  |  PolicyEngine::evaluate(request, &CallHistory)              | |
|  |                                                             | |
|  |  Allow   -> dispatch to tool, record ToolCall               | |
|  |  Block   -> record PhantomEntry(Blocked), return synthetic  | |
|  |  Phantom -> record PhantomEntry(Phantom), return synthetic  | |
|  +-------------------------------------------------------------+ |
|                                                                  |
|  CallHistory updated after every verdict (Block counts too)      |
+----------------------------+-------------------------------------+
                             |
                             v
+----------------------------------------------------------------------+
|                      Witness Chain                                   |
|                                                                      |
|  BLAKE3( RFC-8785-canonical( AgentTrace                              |
|                            + ToolCalls                               |
|                            + PhantomEntries   <-- v2.0               |
|                            + policy_digest    <-- v2.0               |
|                            + WitnessedMetadata ) )                   |
|                                                                      |
|  -> witness_root.txt  (single hex string, independently verifiable)  |
+----------------------------------------------------------------------+
```

### Design principles

|Property                    |How it is achieved                                                                                                                                           |
|----------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------|
|**Tamper-evidence**         |BLAKE3 hash chain over RFC 8785 canonical JSON; any mutation changes the witness root                                                                        |
|**Deterministic replay**    |Fixed seed + chaos profile + policy file produces an identical witness root                                                                                  |
|**Pre-call interception**   |`PolicyEngine::evaluate` runs before any tool dispatch; blocked calls produce no side effects                                                                |
|**Policy auditability**     |SHA-256 digest of the policy file is committed into the witness root                                                                                         |
|**Blocked-call provability**|Every blocked or phantomed call gets a `PhantomEntry` committed into the chain                                                                               |
|**Witness/provenance split**|Runtime environment details (timestamps, thread counts, Nix metadata) go into provenance, never into the witness commitment; roots are stable across hardware|
|**Chaos fault injection**   |Timeout, drop, corrupt, and latency faults are seeded, reproducible, and committed into the transcript                                                       |
|**Parallel stability**      |Parallel execution changes throughput but not witness semantics; ordering is explicitly stabilised after collection                                          |
|**Nix reproducibility**     |`nix_provenance.json` captures the closed Nix store path used to build the binary                                                                            |

-----

## v2.0: Pre-Call Policy Interception

v2.0 puts a policy layer between the agent’s intent and tool execution. It is declarative, TOML-defined, and zero-config: a missing or empty policy file means allow-all, with no overhead for basic usage.

### Policy file format

```toml
# policy.toml
schema_version = 1

# Block any financial tool call once more than 2 have been dispatched this run.
[[rules]]
id                   = "trade-budget"
tool_pattern         = "trade.*"
history_tool_pattern = "trade.*"
history_max_calls    = 2
verdict              = "block"
reason               = "trade call budget exceeded"

# Observe-only: record the agent's intent to call research tools
# but do not dispatch them. The agent gets a synthetic response.
[[rules]]
id           = "research-phantom"
tool_pattern = "research.**"
verdict      = "phantom"
reason       = "research tools are observe-only in this deployment"
```

Rules are evaluated top-to-bottom; the first match wins.

### Verdicts

|Verdict  |Tool dispatched?|Agent sees                        |Witness chain          |
|---------|----------------|----------------------------------|-----------------------|
|`allow`  |Yes             |Real outcome                      |`ToolCall` entry       |
|`block`  |No              |`{ blocked: true, reason: "..." }`|`PhantomEntry(Blocked)`|
|`phantom`|No              |`{ blocked: true, reason: "..." }`|`PhantomEntry(Phantom)`|

### History guards

Rules can condition on cumulative call history within a run. The `CallHistory` is updated after every verdict, so blocked calls count against the budget too. This prevents an agent from circumventing a call limit by interleaving allowed calls:

```toml
[[rules]]
id                   = "llm-budget"
tool_pattern         = "llm_generate"
history_tool_pattern = "llm_generate"
history_max_calls    = 5
verdict              = "block"
reason               = "LLM call budget exceeded"
```

-----

## Artifact Layout

Every agent run emits a self-contained, independently verifiable bundle:

```
out/
├── meta.json                 # Run metadata (witnessed + provenance)
├── trace.jsonl               # Canonical trace events (NDJSON, strict JSON)
├── results.csv / .json       # Case-level results (non-witnessed)
├── summary.json              # Aggregate metrics (non-witnessed)
├── analysis.json             # Bundled metadata + summary + results (non-witnessed)
├── witness_root.txt          # The tamper-evident root; publish this for third-party verification
└── run_0000/
    ├── agent_trace.json      # Every agent step: inputs, tool requests, outputs
    ├── tool_transcript.json  # Every tool call (real + phantom) with outcomes
    ├── chaos_profile.json    # Fault injection schedule (seeded, reproducible)
    ├── drift_report.json     # Replay mismatch report (empty if clean)
    ├── hash_chain.txt        # Per-call BLAKE3 hashes
    ├── meta.json             # Witnessed metadata (seed, policy_digest, ...)
    ├── witness_manifest.json # Cross-file artifact hashes + bundle hash
    ├── witness_root.txt      # Per-run tamper-evident root
    └── verify_report.json    # Written by `verify --witness <dir>`
```

A few things worth knowing:

- `trace.jsonl` and `agent_trace.json` are committed into the witness root via RFC 8785 canonical JSON with stable key ordering and I-JSON integers only.
- `results.json`, `summary.json`, and `analysis.json` use standard JSON serialization and are explicitly non-witnessed.
- Provenance metadata (timestamps, toolchain versions, thread counts, Nix details) is recorded separately and not committed to the witness root. Roots are stable across environments.
- `witness_root.txt` is the only value a third party needs to independently verify the entire bundle.
- `verify` expects `trace.jsonl` (NDJSON); it does not accept `agent_trace.json`. In agent mode, the bundle directory is the unit of verification.

-----

## Deterministic Simulation Testing (DST)

Cogitator can deterministically inject tool faults: timeouts, corruptions, drops, and latency. Faults are driven by a seeded schedule and recorded in tool transcripts so that record + replay stays byte-stable. Simulated latency is exposed to the agent but excluded from witness commitments.

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

Available fault profiles: `none`, `ci`, `stress`.

-----

## Verification Workflow

### Non-agent runs

```bash
# Run evaluation
./target/release/cogitator run --seed 1 --runs 10 --out-dir out --clean

# Verify witness root
./target/release/cogitator verify \
  --meta out/meta.json \
  --trace out/trace.jsonl \
  --expect "$(cat out/witness_root.txt)"
```

### Agent record and replay

```bash
# Record
./target/release/cogitator run --agent ordeal --runs 1 --out-dir out --clean

# Replay -- should produce the identical witness root
./target/release/cogitator run \
  --agent ordeal \
  --case 0 \
  --replay out/run_0000 \
  --out-dir replay \
  --clean

# Verify the bundle (checks manifest hash, per-artifact hashes, witness root consistency)
./target/release/cogitator verify --witness out/run_0000

# Semantic recompute mode -- validates the committed root against bundle semantics.
# On mismatch, prints expected vs computed root and a committed-component hint.
./target/release/cogitator verify --witness out/run_0000 --recompute-witness-root
```

### Drift demo

```bash
# Runs a baseline and a perturbed scenario, emits a drift report
./target/release/cogitator demo drift --seed 1 --threads 2 --fault-profile ci --out-dir demo_out --clean

# Verify the baseline bundle
./target/release/cogitator verify --witness demo_out/drift/baseline_faults

# Semantic recompute verify
./target/release/cogitator verify --witness demo_out/drift/baseline_faults --recompute-witness-root
```

-----

## Ordeal CI Gate

`ordeal` is a minimal, deterministic agent designed as a pinned CI gate. It keeps CI costs low while still asserting end-to-end witness bundle stability. Any change to agent logic, policy evaluation, or the serialization path that touches the witness commitment will cause this gate to fail.

```bash
# Check against the pinned golden root
./target/release/cogitator ordeal check --golden goldens/ordeal_witness_root.txt

# Run the same check locally via the helper script
scripts/check_ordeal_root.sh

# Maintainers only -- intentional witness changes require an explicit golden update
./target/release/cogitator ordeal check --golden goldens/ordeal_witness_root.txt --update-golden
```

-----

## CLI Reference

### Run evaluations

```bash
# Deterministic evaluation, n runs
./target/release/cogitator run --seed 42 --runs 100 --out-dir out

# Agent mode (record)
./target/release/cogitator run --agent ordeal --runs 1 --out-dir out --clean

# Agent mode (replay)
./target/release/cogitator run --agent ordeal --case 0 --replay out/run_0000 --out-dir replay --clean

# With policy enforcement
./target/release/cogitator run --agent clawdbot --policy fixtures/policy_clawdbot_block.toml
```

**Flags:**

|Flag                            |Description                                                           |
|--------------------------------|----------------------------------------------------------------------|
|`--seed <u64>`                  |Entropy seed for determinism                                          |
|`--runs <n>`                    |Number of evaluation runs                                             |
|`--threads <n>`                 |Parallelism (recorded in provenance; does not affect the witness root)|
|`--parallel true|false`         |Enable or disable parallel execution                                  |
|`--faults on|off`               |Enable fault injection                                                |
|`--fault-profile none|ci|stress`|Fault injection profile                                               |
|`--fault-timeout-rate <f64>`    |Per-call timeout probability                                          |
|`--fault-drop-rate <f64>`       |Per-call drop probability                                             |
|`--fault-corrupt-rate <f64>`    |Per-call corruption probability                                       |
|`--fault-latency-rate <f64>`    |Per-call simulated latency probability                                |
|`--llm on|off`                  |Enable or disable LLM integration                                     |
|`--llm-model <n>`               |LLM model selector                                                    |
|`--llm-seed <u64>`              |LLM entropy seed                                                      |
|`--pass-threshold <string>`     |Ordeal pass threshold (committed into witnessed metadata)             |
|`--policy <path>`               |Path to TOML policy file (absent means allow-all)                     |
|`--replay <dir>`                |Replay a prior agent run bundle                                       |
|`--out-dir <dir>`               |Output directory                                                      |
|`--clean`                       |Delete and recreate the output directory before the run               |
|`--no-tui`                      |Disable TUI for headless/CI runs                                      |
|`--theme neon|cyan|mono`        |TUI colour theme (requires `--features tui`)                          |
|`--no-color`                    |Disable ANSI colour output                                            |
|`--created-at <string>`         |Provenance timestamp override                                         |
|`--nix-provenance auto|on|off`  |Nix provenance capture mode                                           |

-----

## Install Prerequisites

### Linux (Debian/Ubuntu)

```bash
sudo apt-get update && sudo apt-get install -y build-essential curl git
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
brew install rustup git && rustup-init
```

### Windows

**Native:**

1. Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/).
1. Install Rust via [rustup](https://rustup.rs/).
1. Verify with `rustc --version && cargo --version`.

**WSL2 (recommended if you want a Linux-like workflow):**

```bash
sudo apt-get update && sudo apt-get install -y build-essential curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### NixOS

```bash
nix develop                        # with flakes
nix-shell -p rustc cargo rustfmt   # without flakes
```

-----

## Why This Matters Now

AI agents are already being deployed in regulated industries like financial services, healthcare, and legal without any standardised way to prove the agent behaved as claimed. Existing approaches, things like log ingestion, RAG-based audit tools, and model cards, are all reconstructive. They describe what *probably* happened, not what *provably* happened.

COGITATOR starts from the position that agent execution should be as auditable as a compiled binary in a reproducible build system. The witness root is the runtime equivalent of a Nix store path: a cryptographic commitment that ties a specific output to a specific, verifiable execution. The v2.0 policy layer extends this further. It is not enough to prove what an agent *did*; a complete audit also needs to prove what the agent *tried to do and was blocked from doing*.

### Where it applies

- **EU AI Act compliance.** Articles 12 and 9 require tamper-evident record-keeping and risk management for high-risk AI systems. COGITATOR is a ready technical substrate for both.
- **Regulated AI deployment.** Financial regulators like the FCA and SEC, as well as healthcare frameworks like FDA SaMD guidance, are moving toward mandatory audit trails for high-autonomy systems. The witness root is a concrete artifact that satisfies tamper-evident log requirements without needing to trust operator infrastructure.
- **Safety-critical robotics.** The policy layer acts as a capability firewall. The AI decision system can operate non-deterministically internally, but it cannot dispatch a command outside the pre-approved operational envelope. Every blocked command is committed into the witness chain, provably. This applies directly to surgical robotics, autonomous vehicles, and industrial automation under IEC 61508.
- **AI liability and insurance.** Insurers and legal counsel currently rely on manufacturer-provided logs to reconstruct AI incidents, which is an inherent conflict of interest. The witness root gives any third party an independent verification path from the same inputs, with no need to trust the operator’s infrastructure.
- **AI red-teaming and safety evaluation.** Security researchers can define capability restrictions via policy, run an agent, and produce a tamper-evident record of what the agent attempted or was blocked from attempting.
- **Multi-party AI contracting.** When an AI agent acts on behalf of one party against a third-party service, both sides can independently verify the same witness root.
- **Benchmark integrity.** Public AI benchmarks are vulnerable to cherry-picking and post-hoc result manipulation. COGITATOR makes benchmark runs independently replayable and verifiable.
- **Model-level compliance testing.** Organisations deploying frontier models can run COGITATOR as a CI gate; if the witness root drifts across model versions for a given policy file, the deployment gets flagged.

-----

## Research Directions

- **Multi-agent witness composition.** Extending the hash chain model to orchestrated multi-agent runs where sub-agent witness roots are nested into a parent root.
- **Formal policy verification.** Using the TOML policy grammar as input to a formal model checker to prove policy rules are consistent and non-bypassable.
- **Formal decision layer verification.** Replacing probabilistic AI components in safety-critical deployments with TLA+-specified decision logic, with COGITATOR providing the audit envelope. This would extend provable bounded behaviour from the policy layer into the decision layer itself.
- **Hardware attestation integration.** Binding the witness root to a TPM attestation quote so tamper-evidence extends from software into silicon.
- **Differential privacy for traces.** Adding noise-injection mechanisms to agent traces that preserve witness root integrity while reducing privacy leakage from sensitive tool arguments.
- **SCITT Transparency Service registration.** Carrying the witness root as a SCITT Signed Statement payload for externally verifiable timestamping and proof of inclusion on a Transparency Service.

-----

## Protocol Specification

The COGITATOR Witness Protocol is specified as a standalone document under Apache 2.0, separate from this implementation. Anyone is free to implement, cite, or build on the protocol without restriction from this codebase.

- **Spec:** [`spec/COGITATOR_WITNESS_PROTOCOL.md`](https://github.com/noctem-o/COGITATOR/blob/main/spec/COGITATOR_WITNESS_PROTOCOL.md)
- **IETF Internet-Draft:** `draft-noctem-cogitator-witness-protocol-00` (independent submission; relates to the [IETF SCITT working group](https://datatracker.ietf.org/wg/scitt/about/))

The protocol is designed to be carried as the payload of a SCITT Signed Statement, with the witness root registered on a Transparency Service for an externally verifiable timestamp and proof of inclusion.

-----

## Project Layout

```
.
├── src/            # Rust source
├── tests/          # Integration test suite
├── schemas/        # JSON schemas for artifacts
├── scripts/        # CI gates and local check helpers
├── goldens/        # Pinned witness roots for the ordeal CI gate
├── tasks/          # Task definitions
├── replay/         # Example replay bundle (run_0000)
├── paper/          # Companion paper (v1.0 architecture)
└── spec/           # COGITATOR Witness Protocol specification (Apache 2.0)
```

-----

## Release Engineering

- **`cargo-dist`** handles release artifacts and installers via a tag-triggered GitHub Action (`.github/workflows/release.yml`) with a pinned `CARGO_DIST_VERSION`.
- **`git-cliff`** generates changelogs from commit history (`cliff.toml`).
- **`--version` / `--help`** includes git SHA metadata at build time, with a deterministic `unknown` fallback when git metadata is unavailable.
- **CI gates** cover format, clippy (`-D warnings`), tests, determinism smoke checks, verify-recompute checks, release dry-runs, and a true no-git build gate (no `.git/` directory and no `git` on `PATH`).
- **RustSec advisory check** via `rustsec/audit-check` is configured by `audit.toml` and wired as a fail-closed release trust gate.
- **Immutable GitHub Action pinning** is enforced by `scripts/check_action_pins.sh`. For protected actions (`actions/checkout`, `actions/upload-artifact`, `actions/attest-build-provenance`, `rustsec/audit-check`), version tags are rejected and only full 40-hex commit SHAs are allowed. To refresh a pin: `scripts/resolve_action_sha.sh <org/repo> <tag>`.
- **GitHub Artifact Attestations** via `actions/attest-build-provenance` cover release artifacts produced by `cargo-dist`.
- **Workflow permissions** are least-privilege: `contents: write` for GitHub Release publishing, `id-token: write` for OIDC, and `attestations: write` for artifact attestations.

-----

## Nix

Cogitator works fine without Nix. If you do use Nix, a dev shell is available:

```bash
nix develop
```

Nix metadata is captured as provenance only and never alters witness roots. On native Windows builds, `--nix-provenance=auto` resolves to `"off (windows)"` while still recording the resolution in provenance. When `RAYON_NUM_THREADS` is set, Cogitator records both the requested env value and the resolved Rayon thread count in provenance only. Set `SOURCE_DATE_EPOCH` for a deterministic `created_at` timestamp (the dev shell may handle this automatically).

-----

## Contributing

See [CONTRIBUTING.md](https://github.com/noctem-o/COGITATOR/blob/main/CONTRIBUTING.md).

-----

## Contact

[george.g@tuta.io](mailto:george.g@tuta.io) – questions, regulated deployment enquiries, or if you’re building audit trails for AI agents and want to compare notes.

-----

## License

**Implementation:** [Business Source License 1.1](https://github.com/noctem-o/COGITATOR/blob/main/LICENSE). Free for non-production use; commercial production use requires a licence. Converts to Apache 2.0 on 2029-04-09.

**Protocol specification** at [`spec/COGITATOR_WITNESS_PROTOCOL.md`](https://github.com/noctem-o/COGITATOR/blob/main/spec/COGITATOR_WITNESS_PROTOCOL.md): Apache 2.0. Free to implement, cite, and build on independently of this implementation.