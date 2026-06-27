# Cogitator

**A Rust harness that turns an agent run into a tamper-evident, independently verifiable record.**

[![CI](https://img.shields.io/github/actions/workflow/status/noctem-o/cogitator/ci.yml?branch=main&label=CI&style=flat-square)](https://github.com/noctem-o/cogitator/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg?style=flat-square)](https://www.rust-lang.org/)
[![Release](https://img.shields.io/github/v/release/noctem-o/cogitator?style=flat-square)](https://github.com/noctem-o/cogitator/releases)
[![Protocol](https://img.shields.io/badge/protocol-draft-informational?style=flat-square)](spec/COGITATOR_WITNESS_PROTOCOL.md)

Cogitator records what an agent did, what it asked to do, and what a policy stopped it from doing — then seals the run under a single cryptographic fingerprint, the **witness root**. Anyone holding the run bundle can recompute that fingerprint and detect whether the record was altered after the fact.

## Brief Overview

Most agent "audit trails" are ordinary log files: mutable, easy to trim, and written by the same system you are trying to audit. Cogitator treats the run itself as evidence:

1. Every agent step and tool call is serialized as canonical JSON (sorted keys, integer-only numbers, byte-stable output).
2. Before any tool call is dispatched, a policy gate returns a verdict: **allow**, **block**, or **phantom** (observe the request without executing it). Blocked and phantom calls are recorded as first-class entries — refusals are part of the evidence, not silently dropped.
3. Each event is folded into a domain-separated BLAKE3 hash chain. The final digest is the witness root.
4. A verifier recomputes the chain from the recorded semantics. One changed byte in any committed field produces a different root.

Timestamps, hostnames, toolchain versions, thread counts, and other environment details are recorded as *provenance*, deliberately **outside** the commitment — so the same logical run hashes identically across machines.

```text
Agent step
  -> tool request
  -> policy gate
     -> allow:          executed ToolCall
     -> block/phantom:  PhantomEntry (recorded, not executed)
  -> canonical JSON witness events
  -> domain-separated BLAKE3 chain
  -> witness root
```

## What Cogitator is

Cogitator is the reference implementation of the [Cogitator Witness Protocol](spec/COGITATOR_WITNESS_PROTOCOL.md). In practice it's three pieces working together — a recorder that captures an agent run, a policy gate that sits in front of every tool call, and a verifier that recomputes and checks the result — along with deterministic fixtures that exercise the protocol and pin its output in CI.

It isn't a general agent framework, and there's no drop-in integration for an external agent yet. Everything it drives today is a deterministic stand-in. `clawdbot` is a small scripted demo agent. `ordeal` is a fixed 50-task suite with stubbed tool responses whose job is to catch accidental changes to the protocol — its witness root is pinned as a golden — not to benchmark anything, so passing it says nothing about how a real agent would behave. Tool calls are answered by deterministic stubs, and even the `llm.generate` backend is a stub, so no external model or service is ever contacted. Wiring in a real agent means driving the Rust APIs — `ToolTranscript` and the witness encoding — from your own code.

It's also worth being clear about what a matching witness root does and doesn't tell you. It establishes integrity relative to that root: the bundle's committed semantics — metadata, agent trace, tool and phantom operations — are exactly what the root commits to, with nothing edited, reordered, truncated, or injected after the run was sealed. It does not, on its own, prove the run ever happened, because a bundle and its root can be regenerated from scratch. Proving occurrence means anchoring the root somewhere outside the bundle: publishing it, timestamping it, or comparing it against a root you received out of band with `--expect`. And it says nothing about the integrity of the machine that produced the bundle, or about making a nondeterministic model backend reproducible.

## Quick start

```bash
cargo build --release
```

Run the drift demo (four scenarios: baseline, regressed, with and without injected faults) and verify the resulting bundle:

```bash
cargo run --release -- demo drift --seed 42 --threads 1 --fault-profile stress --out-dir demo_out --clean

cargo run --release -- verify --witness demo_out/drift/baseline_faults
cargo run --release -- verify --witness demo_out/drift/baseline_faults --recompute-witness-root
```

Expected: verification reports no bundle issues, and the recompute reports `matched=true`.

Record and verify a fixture run:

```bash
cargo run --release -- run --agent ordeal --runs 1 --out-dir out --clean
cargo run --release -- verify --witness out/run_0000 --recompute-witness-root
```

Notes:

- `--clean` refuses dangerous output paths (filesystem root, home, repo root, `.git`, symlinked dirs).
- `--nix-provenance=auto|on` may run bounded local Nix commands (closed stdin, 3s timeout, capped output) to capture diagnostic provenance. This never affects the witness root.

## Policy interception

Policy is evaluated before dispatch; the first matching rule wins. When a policy file is loaded, its SHA-256 digest is committed into the witness root, so the exact policy in force is part of the evidence. If no policy file is present, the current behavior is **allow-all** and no digest is committed.

```toml
schema_version = 1

[[rules]]
id = "trade-budget"
tool_pattern = "trade.*"           # '*' stays within a dot-segment; '**' crosses segments
history_tool_pattern = "trade.*"
history_max_calls = 2              # allow 2 matching calls, block the 3rd onward
verdict = "block"
reason = "trade call budget exceeded"

[[rules]]
id = "research-phantom"
tool_pattern = "research.**"
verdict = "phantom"
reason = "observe only"
```

Verdicts:

- `allow` — the call executes and is recorded as a real `ToolCall`.
- `block` — the call is denied; a `PhantomEntry` with disposition `blocked` is recorded and committed.
- `phantom` — the call is intercepted; a `PhantomEntry` with disposition `phantom` is recorded and committed.

## Verifying a bundle

Three layers, in increasing strength:

| Layer | Command | What it checks |
|---|---|---|
| Bundle self-consistency | `verify --witness <dir>` | Manifest paths resolve under strict confinement (no absolute paths, `..` escapes, or symlink escapes) and the SHA-256 artifact/bundle hashes match the recorded values. |
| Semantic recompute | `verify --witness <dir> --recompute-witness-root` | The witness root recomputes from the bundle's recorded semantics. Recompute fails closed on incomplete or inconsistent transcripts (missing steps, orphan or duplicate tool calls, trace/transcript mismatches). |
| Anchored verification | `verify --witness <dir> --recompute-witness-root --expect <root>` | The bundle's recomputed root matches a root you obtained from somewhere you trust. This is the only layer that defends against wholesale bundle replacement. |

Anchored verification requires `--recompute-witness-root`: in the plain bundle check (`verify --witness <dir>` without that flag), `--expect` is currently not consulted.

The `witness_root.txt` stored inside a bundle is a convenience for self-checks. It is not external anchoring — treat the first two layers as integrity checks and the third as the actual evidentiary comparison.

## Inside a bundle

```text
out/run_0000/
├── meta.json               # witnessed metadata + uncommitted provenance
├── agent_trace.json        # per-step agent entries (committed semantics)
├── tool_transcript.json    # executed calls, phantom entries, policy digest
├── chaos_profile.json      # deterministic fault-injection schedule
├── drift_report.json       # replay/fixture mismatch diagnostics (report-only)
├── hash_chain.txt          # human-readable debugging chain (not the root)
├── witness_manifest.json   # artifact hashes + bundle hash (diagnostics)
├── witness_root.txt        # the BLAKE3 witness root
├── verify_report.json      # written by verify
└── nix_provenance.json     # optional, provenance-only
```

Committed into the witness root: witnessed metadata, agent trace entries, executed tool-call witness views, phantom entries, and the policy digest. Everything else — artifact hashes, bundle hash, drift and verify reports, all provenance — is diagnostic and lives outside the commitment.

## Replay and drift

A recorded bundle can be replayed (`run --replay <bundle_dir> --runs 1`; replay requires a single run): tool responses are substituted from the transcript instead of re-executed, and divergences in the tool transcript — requests, outcomes, faults, phantom entries, and the policy digest — are localized per call in `drift_report.json`. The drift report covers the transcript only; a change in the agent trace itself (thoughts, actions, finality) is not diffed there and surfaces as a witness-root mismatch instead. Deterministic fault injection (`--faults on --fault-profile ci|stress`) is keyed by seed/run/step/call, so fault schedules are identical across runs and never a source of spurious drift.

## Development

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --locked -- ordeal check --golden goldens/ordeal_witness_root.txt
```

CI also enforces SHA-pinned GitHub Actions, a no-git build gate, RustSec `cargo audit`, and the pinned ordeal golden root. Releases use `cargo-dist` with GitHub artifact attestations.

A Nix dev shell is available via `nix develop`.

## Protocol and further reading

- [Witness protocol specification](spec/COGITATOR_WITNESS_PROTOCOL.md) — schema versions, commitment boundary, verification rules. Status: draft, intentionally implementation-coupled.
- `schemas/` — JSON Schemas for the bundle artifacts.
- `main.pdf` — design write-up of the commitment model and determinism experiments.

To produce a bundle you can verify, generate one locally with the quick-start commands above rather than relying on a pre-made example.

## License

Apache-2.0.
