# Changelog

All notable changes to Cogitator are documented in this file.

## Unreleased

## 2.2.2 - 2026-05-16

### Changed
- Replay mode now preserves `policy_digest` from the baseline transcript.
- Ordeal live dispatch now applies policy when recording precomputed fixture responses.
- README clarifies that `--nix-provenance=auto|on` may run bounded local Nix commands for diagnostic provenance.
- Bumped crate version to `2.2.2` for release preparation.

## 2.2.1 - 2026-05-15

### Changed
- Bumped crate version to `2.2.1` for release preparation.
- README wording nits only: replayable run bundle phrasing, policy-digest wording, verifier artifact annotation, and canonicalization terminology alignment.

## 2.0.0 - 2026-04-09

### Added
- **Pre-call policy intercept** (`src/policy.rs`): `PolicyEngine` loads a TOML
  policy file and evaluates every tool request *before* dispatch.  Returns one
  of three verdicts: `Allow`, `Block`, or `Phantom`.
- **Call history** (`CallHistory`): rolling record of what the agent has already
  attempted this run, passed into every policy evaluation so rules can reason
  about call sequences and budgets (e.g. block after N calls to `trade.*`).
- **Phantom entries** (`PhantomEntry` in `src/tooling.rs`): blocked and
  phantomed calls are recorded as `PhantomEntry` structs and committed into the
  witness chain alongside real `ToolCall`s.  Third-party verifiers can prove
  *what the agent tried to do* as well as *what it did*.
- **`policy_digest`** in `ToolTranscriptRecord` and `WitnessedMetadata`: SHA-256
  of the policy file is committed into the witness root so the exact policy
  version in effect is cryptographically provable.
- **`policy.toml`**: default example policy file shipped with the repository,
  with commented-out rules for finance, research, and trade-budget scenarios.
- `TOOL_TRANSCRIPT_SCHEMA_VERSION` bumped from 3 to 4.
- `ToolTranscript::with_policy()` builder method to attach a `PolicyEngine`.
- `ToolTranscript::phantom_entries()` accessor.

### Changed
- `ToolTranscript::execute` now runs the policy intercept before dispatching.
  Behaviour is identical to 1.0 when no policy file is present (allow-all).
- `ToolTranscriptRecord` gains two optional fields (`phantom_entries`,
  `policy_digest`) that are omitted from serialisation when empty/absent,
  preserving forward-compatibility with 1.0 readers.

## 1.0.0 - 2026-02-15

### Changed
- First stable release: project versioning is now anchored at `1.0.0` in
  `Cargo.toml`, and CLI version output is sourced from Cargo package metadata.
- Release documentation was aligned to the 1.0.0 release line while preserving
  existing determinism and witness semantics.
