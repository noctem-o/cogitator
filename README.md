# COGITATOR

> **Cryptographic auditing and pre-call policy interception for AI agent runs.**

[![CI](https://img.shields.io/github/actions/workflow/status/noctem-o/COGITATOR/ci.yml?branch=main&label=CI)](https://github.com/noctem-o/COGITATOR/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

COGITATOR is a deterministic evaluation harness and **tamper-evident notary** for AI agent execution. Every run produces a cryptographic witness root — a BLAKE3 hash chain over RFC 8785 canonical JSON — that any third party can recompute independently to verify the record was not altered after the fact. As of v2.0, COGITATOR also intercepts every tool call *before* dispatch, evaluating it against a declarative policy and recording the decision as an auditable, signed artifact.

---

## The Problem

Modern AI agents are deployed into high-stakes environments — financial trading, medical triage, legal research, critical infrastructure — where they issue tool calls with real-world consequences. The current state of the art offers almost no verifiable answer to the three questions that matter most:

1. **Did the agent do exactly what the log says it did?** Logs are mutable. Post-hoc summaries are reconstructions.
2. **Was the agent operating within its sanctioned boundaries at the time of the call?** Runtime policy is typically advisory, unenforced, or checked after the fact.
3. **Can an independent auditor reproduce the same result from the same inputs?** Without deterministic replay, audit is interpretation, not verification.

COGITATOR is a direct answer to all three.

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                          Agent Loop                              │
│   AgentInput → Agent::step() → AgentOutput (tool_requests)      │
└────────────────────────────┬─────────────────────────────────────┘
                             │  tool_requests
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│               ToolTranscript::execute()    ◄── 2.0 INTERCEPT     │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  PolicyEngine::evaluate(request, &CallHistory)          │    │
│  │                                                         │    │
│  │  Allow  → dispatch to tool, record ToolCall             │    │
│  │  Block  → record PhantomEntry(Blocked), return synthetic│    │
│  │  Phantom→ record PhantomEntry(Phantom), return synthetic│    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                  │
│  CallHistory updated after every verdict (Block counts too)      │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│                    Witness Chain                                  │
│                                                                  │
│  BLAKE3( RFC-8785-canonical( AgentTrace                          │
│                            + ToolCalls                           │
│                            + PhantomEntries   ◄── 2.0            │
│                            + policy_digest    ◄── 2.0            │
│                            + WitnessedMetadata ) )               │
│                                                                  │
│  → witness_root.txt  (single hex string, independently verifiable)│
└──────────────────────────────────────────────────────────────────┘
```

### Key properties

| Property | How it is achieved |
|---|---|
| **Tamper-evidence** | BLAKE3 hash chain over RFC 8785 canonical JSON; any mutation changes the witness root |
| **Deterministic replay** | Fixed seed + chaos profile + policy file → identical witness root |
| **Pre-call interception** | `PolicyEngine::evaluate` runs *before* any tool dispatch |
| **Policy auditability** | SHA-256 digest of the policy file is committed into the witness root |
| **Blocked-call provability** | `PhantomEntry` records every blocked/phantomed call; committed into the chain |
| **Chaos fault injection** | Timeout / drop / corrupt / latency faults, seeded and reproducible |
| **Nix reproducibility** | `nix_provenance.json` captures the closed Nix store path used to build the binary |

---

## v2.0 — Pre-Call Policy Interception

v2.0 adds a **policy layer** between the agent's intent and tool execution. It is declarative, TOML-defined, and zero-config (absent = allow-all).

### Policy file format

```toml
# policy.toml
schema_version = 1

# Block any financial tool call after more than 2 have been dispatched this run.
[[rules]]
id           = "trade-budget"
tool_pattern = "trade.*"
history_tool_pattern = "trade.*"
history_max_calls    = 2
verdict      = "block"
reason       = "trade call budget exceeded"

# Observe-only: record the agent's intent to call research tools, but do not
# dispatch them. The agent receives a synthetic response.
[[rules]]
id           = "research-phantom"
tool_pattern = "research.**"
verdict      = "phantom"
reason       = "research tools are observe-only"
```

Rules are evaluated top-to-bottom; the first match wins. An empty rules list (or a missing file) is equivalent to allow-all — no policy overhead for basic usage.

### Verdicts

| Verdict | Tool dispatched? | Agent sees | Witness chain |
|---|---|---|---|
| `allow` | ✅ Yes | Real outcome | `ToolCall` entry |
| `block` | ❌ No | `{ blocked: true, reason: "..." }` | `PhantomEntry(Blocked)` |
| `phantom` | ❌ No | `{ blocked: true, reason: "..." }` | `PhantomEntry(Phantom)` |

### History guards

Rules can condition on cumulative call history within a run:

```toml
[[rules]]
id                   = "llm-budget"
tool_pattern         = "llm_generate"
history_tool_pattern = "llm_generate"
history_max_calls    = 5
verdict              = "block"
reason               = "LLM call budget exceeded"
```

The `CallHistory` is updated after every verdict (blocked calls count against the budget), preventing circumvention by interleaving allowed calls.

---

## Quick Start

```bash
# Enter the reproducible dev shell (requires Nix with flakes)
nix develop

# Run CI checks (fmt → clippy → test)
cargo fmt --check
cargo clippy -- -D warnings
cargo test

# Run the ordeal golden gate
cargo run -- ordeal check

# Run a single agent evaluation with policy enforcement
cargo run -- run --agent clawdbot --policy fixtures/policy_clawdbot_block.toml

# Verify a prior run's witness root
cargo run -- verify --recompute-witness-root --witness out/run_0000 \
  --expect <witness_root_hex>

# Replay a prior run and check for drift
cargo run -- run --replay out/run_0000
```

---

## Artifact layout

Every agent run emits a self-contained bundle:

```
out/run_0000/
├── agent_trace.json          # Every agent step: inputs, tool requests, outputs
├── tool_transcript.json      # Every tool call (real + phantom) with outcomes
├── chaos_profile.json        # Fault injection schedule (seeded, reproducible)
├── drift_report.json         # Replay mismatch report (empty if no drift)
├── hash_chain.txt            # Per-call BLAKE3 hashes
├── meta.json                 # Witnessed metadata (seed, policy_digest, ...)
├── witness_manifest.json     # Cross-file artifact hashes + bundle hash
└── witness_root.txt          # Single hex string — the tamper-evident root
```

The `witness_root.txt` is the only value that needs to be published for a third party to verify the entire bundle.

---

## Research & Commercialisation Context

### Why this matters now

AI agents are being deployed in regulated industries (financial services, healthcare, legal) without any standardised mechanism for proving that the agent behaved as claimed. Existing approaches — log ingestion, RAG-based audit tools, model cards — are all reconstructive: they describe what *probably* happened, not what *provably* happened.

COGITATOR takes the position that **agent execution should be as auditable as a compiled binary in a reproducible build system.** The witness root is the runtime equivalent of a Nix store path: a cryptographic commitment that ties a specific output to a specific, verifiable execution.

### Target applications

- **Regulated AI deployment** — Financial regulators (FCA, SEC) and healthcare frameworks (EU AI Act, FDA SaMD guidance) are moving toward mandatory audit trails for high-autonomy systems. COGITATOR provides a ready technical substrate.
- **AI red-teaming and safety evaluation** — Security researchers can use the policy layer to define capability restrictions, run an agent, and produce a tamper-evident record proving the agent attempted (or did not attempt) to circumvent them.
- **Multi-party AI contracting** — When an AI agent acts on behalf of a client against a third-party service, both parties can independently verify the same witness root.
- **Benchmark integrity** — Public AI benchmarks are vulnerable to cherry-picking and post-hoc result manipulation. COGITATOR's witness root makes benchmark runs independently replayable.
- **Model-level compliance testing** — Organisations deploying frontier models can run COGITATOR as a CI gate: if the witness root for a given policy file drifts, the deployment is blocked.

### Research directions

- **Multi-agent witness composition** — Extending the hash chain model to orchestrated multi-agent runs where sub-agent witness roots are nested into a parent root.
- **Formal policy verification** — Using the TOML policy grammar as input to a formal model checker to prove policy rules are consistent and non-bypassable.
- **Hardware attestation integration** — Binding the witness root to a TPM attestation quote for tamper-evidence that extends from software into silicon.
- **Differential privacy for traces** — Adding noise-injection mechanisms to agent traces that preserve witness root integrity while reducing privacy leakage from sensitive tool arguments.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

MIT. See [LICENSE](LICENSE).
