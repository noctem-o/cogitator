# COGITATOR — Deterministic Evaluation Harness for Agents


██████╗ ██████╗ ██████╗ ██╗████████╗ █████╗ ████████╗ ██████╗ ██████╗
██╔════╝ ██╔═══██╗██╔════╝ ██║╚══██╔══╝██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗
██║ ██║ ██║██║ ███╗██║ ██║ ███████║ ██║ ██║ ██║██████╔╝
██║ ██║ ██║██║ ██║██║ ██║ ██╔══██║ ██║ ██║ ██║██╔══██╗
╚██████╗ ╚██████╔╝╚██████╔╝██║ ██║ ██║ ██║ ██║ ╚██████╔╝██║ ██║
╚═════╝ ╚═════╝ ╚═════╝ ╚═╝ ╚═╝ ╚═╝ ╚═╝ ╚═╝ ╚═════╝ ╚═╝ ╚═╝


Cogitator is a small Rust program for producing repeatable evaluation runs. Give it a seed and a number of runs and it generates a stable set of cases, writes a CSV, and prints an optional terminal “Mission Control” summary.

The main promise is simple: same inputs, same output. If the results change, something changed.

## What it produces

Each run generates:

- `case_id`: stable ID derived from `SHA-256(seed, run_id)` (hex)
- `difficulty`: derived scalar from the digest
- `score`: deterministic score in `[0, 1]`
- `passed`: deterministic pass/fail flag
- `results.csv`: easy to diff, plot, or feed into a pipeline

The terminal UI can be disabled for CI with `--no-tui`.

## Build

```bash
cargo build --release

Run

Defaults: --seed 42 --runs 5000 --output results.csv

cargo run --release

Explicit parameters:

cargo run --release -- --seed 1337 --runs 10000 --output results.csv

Disable the terminal UI:

cargo run --release -- --no-tui

CLI

cogitator [OPTIONS]

Options:
      --seed <SEED>        Seed for deterministic evaluation [default: 42]
      --runs <RUNS>        Number of evaluation runs [default: 5000]
      --output <OUTPUT>    Output CSV path [default: results.csv]
      --no-tui             Disable terminal UI output
  -h, --help               Print help
  -V, --version            Print version

CSV format

Columns:

    run_id (u32): run index 0..runs-1

    case_id (string): stable per (seed, run_id)

    difficulty (float): digest[0] / 255.0

    score (float): deterministic scalar in [0, 1]

    passed (bool): deterministic pass/fail

Example:

run_id,case_id,difficulty,score,passed
0,9f2a...e41c,0.125,0.731,true

How determinism works

High level:

    Parse CLI args (seed, runs, output, no_tui)

    For each run_id:

        compute digest = SHA256(seed, run_id)

        case_id = hex(digest)

        difficulty = digest[0] / 255

        derive RNG seed from digest[0..8]

        compute score and passed deterministically

    Write CSV telemetry and print a summary (and the TUI, unless disabled)

The important part is that all randomness comes from a seed derived from the hash, not from ambient entropy.

Tests

cargo test

There’s a determinism test that runs the harness twice with the same seed and checks that case IDs, scores, and pass/fail match.
Extending it

This is meant to be a harness, not a full evaluation suite. Typical next steps:

    Replace evaluate_case() with your real task runner and scoring logic.

    Keep case_id stable so results remain comparable across versions.

    Add more telemetry if you need it (JSONL, traces, histograms).

    Improve capture_hardware() if you want richer environment reporting (CPU brand, RAM, GPU, kernel, etc.). Just don’t let hardware data influence scoring unless you explicitly want that.

License

MIT / Apache-2.0
