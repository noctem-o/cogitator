# Cogitator

Cogitator is a Rust harness for producing deterministic, tamper-evident records of AI-agent runs.

[![CI](https://img.shields.io/github/actions/workflow/status/noctem-o/cogitator/ci.yml?branch=main&label=CI&style=flat-square)](https://github.com/noctem-o/cogitator/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg?style=flat-square)](https://www.rust-lang.org/)
[![Release](https://img.shields.io/github/v/release/noctem-o/cogitator?style=flat-square)](https://github.com/noctem-o/cogitator/releases)
[![Protocol](https://img.shields.io/badge/protocol-draft-informational?style=flat-square)](spec/COGITATOR_WITNESS_PROTOCOL.md)
[![Threat model](https://img.shields.io/badge/threat%20model-design%20notes-informational?style=flat-square)](#design-notes--threat-model)

**Navigation:** [Quick start](#quick-start) · [Verification model](#verification-model) · [Policy interception](#policy-interception) · [Protocol](spec/COGITATOR_WITNESS_PROTOCOL.md) · [Threat model](#design-notes--threat-model) · [Development](#development)

Each run emits a witness root, a replay bundle, and a policy-aware tool transcript. The witness root is computed from canonical witnessed semantics (not from report files), and blocked/phantom operations are recorded explicitly.

## Why it exists

Most agent audit trails are mutable logs plus summaries written after execution. That is useful for debugging, but weak as evidence.

Cogitator commits the execution boundary before tool effects become the only source of truth. It records what the agent requested, what was executed, and what was intercepted.

The project is a verifier/audit substrate. It is not a full agent framework and it does not claim to solve runtime trust on its own.

## What you get

| Capability | What it means in practice |
|---|---|
| Witness root | Domain-separated BLAKE3 commitment over witnessed metadata, agent trace entries, and tool/phantom operations. |
| Strict canonical witnessed bytes | Deterministic canonical JSON subset for committed bytes (JCS-style / I-JSON constrained). |
| Tool transcript | Executed calls with outcomes and faults, plus policy digest when present. |
| Phantom entries | Blocked/phantom tool requests are recorded and committed instead of silently dropped. |
| Bundle verifier | Path confinement + artifact self-consistency checks on bundle contents. |
| Replay/drift reports | Drift demos and replay checks to surface mismatches deterministically. |
| Provenance split | Provenance is recorded separately and excluded from witness bytes. |
| Nix provenance (optional) | `nix_provenance.json` is diagnostic/provenance-only and does not change witness root. |

## Quick start

```bash
cargo build --release
```

```bash
cargo run --release -- demo drift --seed 42 --threads 1 --fault-profile stress --out-dir demo_out --clean
```

```bash
cargo run --release -- verify --witness demo_out/drift/baseline_faults
cargo run --release -- verify --witness demo_out/drift/baseline_faults --recompute-witness-root
```

Expected result:

- verify reports no bundle issues
- witness recompute reports `matched=true`

## Record and verify an agent run

```bash
cargo run --release -- run --agent ordeal --runs 1 --out-dir out --clean
```

```bash
cargo run --release -- verify --witness out/run_0000
```

```bash
cargo run --release -- verify --witness out/run_0000 --recompute-witness-root
```

## Policy interception

Cogitator evaluates policy before dispatch. First matching rule wins.

- `allow`: call is executed and recorded as a real tool entry.
- `block`: call is denied; a blocked phantom entry is recorded.
- `phantom`: call is intercepted; a phantom entry is recorded.

If no policy file is provided, current behavior is allow-all.

```toml
schema_version = 1

[[rules]]
id = "trade-budget"
tool_pattern = "trade.*"
history_tool_pattern = "trade.*"
history_max_calls = 2
verdict = "block"
reason = "trade call budget exceeded"

[[rules]]
id = "research-phantom"
tool_pattern = "research.**"
verdict = "phantom"
reason = "observe only"
```

```text
Agent step
  -> tool request
  -> policy gate
     -> allow: executed ToolCall
     -> block/phantom: PhantomEntry
  -> canonical witness event stream
  -> BLAKE3 witness root
```

## Verification model

Cogitator verification has three layers:

1. **Bundle self-consistency**: manifest paths + diagnostic artifact hashes.
2. **Semantic witness recompute**: recompute root from witnessed semantics.
3. **Anchored verification**: compare against an externally supplied expected root (`--expect`).

`witness_root.txt` stored in the same bundle is useful for internal consistency checks, but by itself it does not prove original occurrence.

| Check | Command | What it proves | What it does not prove |
|---|---|---|---|
| Bundle self-consistency | `verify --witness <bundle_dir>` | Manifest paths resolve under path confinement and diagnostic hashes match recorded values. | That the run originally occurred. |
| Semantic recompute | `verify --witness <bundle_dir> --recompute-witness-root` | Witnessed semantics recompute to the bundle's expected/co-located root. | External timestamping or publication. |
| Anchored verification | `verify --witness <bundle_dir> --expect <root>` | Bundle semantics match an externally supplied expected root. | Runtime/hardware integrity of the original execution. |

## Artifact layout

```text
out/run_0000/
├── meta.json
├── agent_trace.json
├── tool_transcript.json
├── chaos_profile.json
├── drift_report.json
├── hash_chain.txt
├── witness_manifest.json
├── witness_root.txt
├── verify_report.json
└── nix_provenance.json        # optional
```

Manifest artifact paths are bundle-relative and verified under strict path confinement.

Committed vs diagnostic (high-level):

- committed semantics: witnessed metadata, agent trace entries, tool/phantom operations
- diagnostic/self-consistency: manifest artifact hashes, bundle hash, verify/report artifacts

## Commitment boundary

Committed into witness root:

- witnessed metadata (`meta.json` witnessed section)
- canonicalized `agent_trace` entry semantics
- canonicalized executed tool-call witness view
- canonicalized phantom/intercepted witness view
- policy digest when present in transcript
- chaos/fault information when represented in committed tool outcomes

Not committed into witness root:

- provenance section of metadata
- external environment attestations
- report-only analytics and verifier output files

## Design notes / threat model

Cogitator is good at catching:

- post-hoc edits to committed run semantics (when checked against an expected root)
- malformed, incomplete, or inconsistent transcripts during recompute
- path escape attempts in manifest artifact references

Cogitator does not by itself provide:

- proof that a run happened unless root is externally anchored
- hardware/runtime attestation
- deterministic LLM outputs for nondeterministic backends

## Development

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --locked -- ordeal check --golden goldens/ordeal_witness_root.txt
```

## Release / supply chain

Current repository workflows include:

- `cargo-dist` release automation
- GitHub artifact attestations in release workflow
- pinned GitHub Actions enforcement
- no-git build gate checks
- RustSec `cargo audit` checks

See workflow files for exact enforcement points.

## Nix

```bash
nix develop
```

Nix provenance capture is provenance-only and does not alter witness root computation.

## License

Apache-2.0.
