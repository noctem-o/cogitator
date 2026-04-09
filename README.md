# COGITATOR

> **Tamper-evident AI agent audit harness with cryptographic witness chain, pre-call policy interception, and byte-stable replay.**

[![CI](https://img.shields.io/github/actions/workflow/status/noctem-o/COGITATOR/ci.yml?branch=main&label=CI&style=flat-square)](https://github.com/noctem-o/COGITATOR/actions)
[![License: BUSL-1.1](https://img.shields.io/badge/license-BUSL--1.1-green.svg?style=flat-square)](LICENSE)
[![Spec: Apache-2.0](https://img.shields.io/badge/spec-Apache--2.0-blue.svg?style=flat-square)](spec/COGITATOR_WITNESS_PROTOCOL.md)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg?style=flat-square)](https://www.rust-lang.org)

COGITATOR lets you prove what your AI agent did, what it tried to do, and what it was blocked from doing. Every run produces a cryptographic witness root -- a BLAKE3 hash chain over RFC 8785 canonical JSON -- that any third party can recompute independently to verify the record was not altered after the fact. As of v2.0, COGITATOR also intercepts every tool call before dispatch, evaluates it against a declarative policy, and records the decision as an auditable artefact committed into the witness chain.

The wire format is specified in [spec/COGITATOR_WITNESS_PROTOCOL.md](spec/COGITATOR_WITNESS_PROTOCOL.md) under Apache 2.0 -- free to implement, cite, and build on independently of this implementation. The protocol has been submitted to the IETF as `draft-noctem-cogitator-witness-protocol-00` and is designed to be carried as a payload inside [SCITT](https://datatracker.ietf.org/wg/scitt/about/) Signed Statements for transparent, timestamped registration on a Transparency Service.

---

## The Problem

AI agents are being deployed into regulated environments -- financial trading, medical triage, legal research, critical infrastructure -- where they issue tool calls with real-world consequences. The current state of the art offers almost no verifiable answer to the three questions that matter most:

1. **Did the agent do exactly what the log says it did?** Logs are mutable. Post-hoc summaries are reconstructions.
2. **Was the agent operating within its sanctioned boundaries at the time of the call?** Runtime policy is typically advisory, unenforced, or checked after the fact.
3. **Can an independent auditor reproduce the same result from the same inputs?** Without deterministic replay, audit is interpretation, not verification.

COGITATOR is a direct answer to all three.

---

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

### Key properties

| Property | How it is achieved |
|---|---|
| **Tamper-evidence** | BLAKE3 hash chain over RFC 8785 canonical JSON; any mutation changes the witness root |
| **Deterministic replay** | Fixed seed + chaos profile + policy file produces identical witness root |
| **Pre-call interception** | `PolicyEngine::evaluate` runs before any tool dispatch |
| **Policy auditability** | SHA-256 digest of the policy file is committed into the witness root |
| **Blocked-call provability** | `PhantomEntry` records every blocked or phantomed call, committed into the chain |
| **Chaos fault injection** | Timeout, drop, corrupt, and latency faults -- seeded and reproducible |
| **Nix reproducibility** | `nix_provenance.json` captures the closed Nix store path used to build the binary |

---

## v2.0 -- Pre-Call Policy Interception

v2.0 adds a policy layer between the agent's intent and tool execution. It is declarative, TOML-defined, and zero-config (absent = allow-all).

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

Rules are evaluated top-to-bottom; the first match wins. An empty rules list (or a missing file) is equivalent to allow-all -- no policy overhead for basic usage.

### Verdicts

| Verdict | Tool dispatched? | Agent sees | Witness chain |
|---|---|---|---|
| `allow` | Yes | Real outcome | `ToolCall` entry |
| `block` | No | `{ blocked: true, reason: "..." }` | `PhantomEntry(Blocked)` |
| `phantom` | No | `{ blocked: true, reason: "..." }` | `PhantomEntry(Phantom)` |

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

# Run CI checks (fmt -> clippy -> test)
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

## Artifact Layout

Every agent run emits a self-contained bundle:

```
out/run_0000/
+-- agent_trace.json          # Every agent step: inputs, tool requests, outputs
+-- tool_transcript.json      # Every tool call (real + phantom) with outcomes
+-- chaos_profile.json        # Fault injection schedule (seeded, reproducible)
+-- drift_report.json         # Replay mismatch report (empty if no drift)
+-- hash_chain.txt            # Per-call BLAKE3 hashes
+-- meta.json                 # Witnessed metadata (seed, policy_digest, ...)
+-- witness_manifest.json     # Cross-file artifact hashes + bundle hash
+-- witness_root.txt          # Single hex string -- the tamper-evident root
```

`witness_root.txt` is the only value that needs to be published for a third party to verify the entire bundle.

---

## Why This Matters Now

AI agents are being deployed in regulated industries -- financial services, healthcare, legal -- without any standardised mechanism for proving the agent behaved as claimed. Existing approaches (log ingestion, RAG-based audit tools, model cards) are all reconstructive: they describe what probably happened, not what provably happened.

COGITATOR takes the position that agent execution should be as auditable as a compiled binary in a reproducible build system. The witness root is the runtime equivalent of a Nix store path: a cryptographic commitment that ties a specific output to a specific, verifiable execution.

### Target applications

- **EU AI Act compliance** -- Articles 12 and 9 require tamper-evident record-keeping and risk management for high-risk AI systems. COGITATOR provides a ready technical substrate.
- **Regulated AI deployment** -- Financial regulators (FCA, SEC) and healthcare frameworks (FDA SaMD guidance) are moving toward mandatory audit trails for high-autonomy systems.
- **AI red-teaming and safety evaluation** -- Security researchers can define capability restrictions via policy, run an agent, and produce a tamper-evident record proving what the agent attempted or was blocked from attempting.
- **Multi-party AI contracting** -- When an AI agent acts on behalf of a client against a third-party service, both parties can independently verify the same witness root.
- **Benchmark integrity** -- Public AI benchmarks are vulnerable to cherry-picking and post-hoc result manipulation. COGITATOR's witness root makes benchmark runs independently replayable.
- **Model-level compliance testing** -- Organisations deploying frontier models can run COGITATOR as a CI gate: if the witness root for a given policy file drifts, the deployment is blocked.

### Research directions

- **Multi-agent witness composition** -- Extending the hash chain model to orchestrated multi-agent runs where sub-agent witness roots are nested into a parent root.
- **Formal policy verification** -- Using the TOML policy grammar as input to a formal model checker to prove policy rules are consistent and non-bypassable.
- **Hardware attestation integration** -- Binding the witness root to a TPM attestation quote for tamper-evidence that extends from software into silicon.
- **Differential privacy for traces** -- Adding noise-injection mechanisms to agent traces that preserve witness root integrity while reducing privacy leakage from sensitive tool arguments.

---

## Protocol Specification

The COGITATOR Witness Protocol is specified as a standalone document under Apache 2.0, separate from this implementation. Anyone is free to implement, cite, or build on the protocol without restriction.

- **Spec:** [spec/COGITATOR_WITNESS_PROTOCOL.md](spec/COGITATOR_WITNESS_PROTOCOL.md)
- **IETF Internet-Draft:** `draft-noctem-cogitator-witness-protocol-00` (submitted to [IETF SCITT working group](https://datatracker.ietf.org/wg/scitt/about/))

The protocol is designed to be carried as the payload of a SCITT Signed Statement, with the witness root registered on a Transparency Service for an externally verifiable timestamp and proof of inclusion.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Contact

george.g@tuta.io -- questions, regulated deployment enquiries, or if you're working on audit trails for AI agents and want to compare notes.

## Licence

Business Source License 1.1. Free for non-production use. Commercial production use requires a licence. Converts to Apache 2.0 on 2029-04-09. See [LICENSE](LICENSE).

The protocol specification at [spec/COGITATOR_WITNESS_PROTOCOL.md](spec/COGITATOR_WITNESS_PROTOCOL.md) is separately licenced under Apache 2.0.
