# Contributing to Cogitator

Thank you for your interest in contributing to Cogitator.

## Dev Workflow

All contributions must pass the following three checks before merging. Run them
in order from inside the Nix dev shell (`nix develop`):

```bash
# 1. Formatting — zero diff expected
cargo fmt --check

# 2. Lints — zero warnings allowed
cargo clippy -- -D warnings

# 3. Test suite — all tests must pass
cargo test
```

The ordeal golden witness gate must also remain green:

```bash
cargo run -- ordeal check
```

If an intentional change alters the ordeal witness root (e.g. a schema or
scoring change), regenerate the golden before committing:

```bash
cargo run -- ordeal check --update-golden
```

## Entering the Dev Shell

This project uses a Nix flake dev shell that provides `cargo`, `rustc`,
`rustfmt`, and `clippy`. On a NixOS machine (or any system with Nix flakes
enabled) run:

```bash
nix develop
```

Do **not** install a separate `rustup` toolchain — use the shell-provided
binaries only.

## Commit Style

Use conventional commit prefixes (`feat:`, `fix:`, `chore:`, `docs:`, `test:`).
Keep the subject line under 72 characters.
