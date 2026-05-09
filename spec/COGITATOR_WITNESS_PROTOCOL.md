# Cogitator Witness Protocol

**Status:** Draft (reference implementation in this repository)  
**Last updated:** 2026-05-09  
**License:** Apache-2.0

## 1) Scope

This document defines the current witness protocol implemented by Cogitator.

It is intentionally implementation-coupled: schema constants, commitment boundaries, and verification rules here are expected to match code in `src/` exactly. Future protocol-breaking changes require explicit migration notes.

## 2) Protocol versions and schemas

Current schema constants:

- Witnessed trace schema version: **3** (`TRACE_SCHEMA_VERSION`)
- Witness manifest schema version: **3** (`WITNESS_MANIFEST_SCHEMA_VERSION`)
- Tool transcript schema version: **4** (`TOOL_TRANSCRIPT_SCHEMA_VERSION`)

## 3) Bundle layout

A run bundle contains:

- `meta.json`
- `agent_trace.json`
- `tool_transcript.json`
- `chaos_profile.json`
- `drift_report.json`
- `hash_chain.txt`
- `witness_manifest.json`
- `witness_root.txt`
- optional `nix_provenance.json`

`witness_manifest.json` paths are bundle-relative by design.

## 4) Witnessed vs provenance data

Cogitator separates metadata into:

- `metadata.witnessed`: committed into witness bytes
- `metadata.provenance`: recorded for diagnostics/provenance, **excluded** from witness bytes

This split is normative for current protocol behavior.

## 5) Canonicalization

Witnessed bytes use Cogitator’s strict deterministic canonical JSON subset (JCS-style / I-JSON constrained):

- deterministic object-key ordering
- deterministic UTF-8 serialization
- integer-only witnessed numbers

This is not a blanket claim of full RFC 8785 conformance across all JSON artifacts.

Report and diagnostic files may use ordinary JSON where implementation does so.

## 6) Witness root algorithm

The authoritative witness root algorithm is a domain-separated BLAKE3 linear chain over semantic witness events.

1. Start witness state with `Witness::new(canonical(witnessed_metadata))` using INIT domain separation.
2. Iterate `agent_trace` entries in order.
3. For each agent step, commit canonical `AgentTraceEntryWitness` bytes.
4. For that same step, commit all tool operations (executed calls and phantom/intercepted entries) ordered by `tool_call_idx`.
5. Each `Witness::update` uses STEP domain separation, previous hash, length prefix, and event bytes.
6. Final root is lowercase BLAKE3 hex.

`witness_manifest.json`, `artifact_hashes`, and `bundle_hash` are **not** the witness-root authority.

## 7) Transcript totality and ordering rules

Verifier recompute fails closed if transcript semantics are incomplete or inconsistent.

Required properties:

- `agent_trace.step` is strictly increasing and unique.
- No orphan tool/phantom operations (every op step must exist in agent trace).
- No duplicate/colliding `tool_call_idx` across executed+phantom operations.
- Global `tool_call_idx` coverage is contiguous from zero.
- For each step, `agent_trace.tool_requests` must match transcript operations by tool name and request arguments.

## 8) Path confinement rules

Manifest artifact paths must be bundle-relative.

Verifier rejects:

- absolute paths
- canonical path escapes (`..` traversal after normalization)
- symlink escapes outside bundle root

## 9) Artifact hashes and bundle hash

`artifact_hashes` and `bundle_hash` are bundle self-consistency diagnostics.

- Current implementation uses SHA-256 for these diagnostics.
- They are useful for tamper detection and integrity reporting.
- They do not replace semantic witness recompute.

## 10) Verification modes

Cogitator exposes three relevant verification modes:

1. **Bundle self-consistency**: validate manifest paths and diagnostic hashes.
2. **Semantic witness recompute**: recompute root from witnessed semantics (metadata, trace, tool/phantom operations).
3. **Anchored verification**: compare against externally supplied expected root (for example via `--expect`).

A co-located `witness_root.txt` alone is not proof that the run originally occurred.

## 11) Policy behavior (current)

- If policy file exists, policy digest is committed.
- If policy file is absent, behavior currently resolves to allow-all.
- Verdicts:
  - `allow`: real tool call recorded and committed
  - `block`: phantom/blocked entry recorded and committed
  - `phantom`: phantom/intercepted entry recorded and committed

This section documents current behavior, not a recommendation.

## 12) Threat model summary

Cogitator is designed to:

- detect post-hoc mutation of committed bundle contents when compared against an expected root
- reject malformed or incomplete transcript structures during recompute

Cogitator does **not** by itself:

- prove a run occurred (without external root anchoring)
- attest hardware/runtime environment integrity
- make nondeterministic model backends deterministic

## 13) Versioning and migrations

Protocol-breaking changes (schema constants, witness event encoding, root algorithm, commitment boundary) require:

- versioned migration notes in this spec
- explicit verifier compatibility notes
- test updates showing expected behavior changes

No such protocol change is introduced in this documentation pass.
