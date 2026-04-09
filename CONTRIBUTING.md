# Contributing to COGITATOR

## CI check sequence

Every pull request must pass all four checks in order:

```bash
cargo fmt --check          # 1. Formatting
cargo clippy -- -D warnings # 2. Lints (zero warnings)
cargo test                  # 3. Unit + integration tests (includes policy_bail)
cargo run -- ordeal check   # 4. Ordeal golden gate
```

The ordeal golden gate (`goldens/ordeal_witness_root.txt`) locks the deterministic witness root of the canonical ordeal run. If your change intentionally alters the witness output (e.g. you add a new witnessed metadata field), regenerate the golden with:

```bash
cargo run -- ordeal check --update-golden
git add goldens/ordeal_witness_root.txt
```

Then include the golden update in your PR and explain why the witness root changed.

## Policy bail test

`tests/policy_bail.rs` is the integration test for the 2.0 pre-call interception layer. It is part of the standard `cargo test` suite. If you modify `policy.rs` or `tooling.rs`, ensure all tests in `policy_bail.rs` continue to pass and add new tests for any new policy behaviour.

## Adding a policy rule to a fixture

Policy fixtures live in `fixtures/`. The canonical examples are:

- `fixtures/policy_clawdbot_block.toml` — block-after-budget demo
- `fixtures/policy_allow_all.toml` — explicit allow-all reference

New fixtures should include a comment header explaining the intended scenario and the correct `cogitator run` invocation.

## Witness root stability

The witness root is a cryptographic commitment over the full execution. The following changes will shift the witness root and require a golden update:

- Adding or removing fields in `WitnessedMetadata`
- Changing the serialisation of any witnessed type
- Changing the `ToolTranscriptRecord` schema version
- Changing the BLAKE3 hash chain construction
- Adding new entropy sources to the metadata

Changes that do *not* shift the witness root (and do not require a golden update):

- Changes to provenance fields (git_rev, rustc_version, etc.)
- Changes to the TUI, reporting, or CLI flag defaults
- Adding new test fixtures or integration tests
- Documentation changes

## Code style

- All public types and functions must have doc comments.
- `deny_unknown_fields` on all serialised structs.
- No `unwrap()` in non-test code — use `?` with `anyhow::Context`.
- New serialised fields that may be absent must use `#[serde(default, skip_serializing_if = ...)]`.
