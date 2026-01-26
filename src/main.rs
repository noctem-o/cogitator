fn main() {
    println!("Hello, world!");
use anyhow::{Context, Result};
use clap::Parser;
use csv::Writer;
use rand::{rngs::StdRng, Rng, SeedableRng};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "cogitator", version, about = "Deterministic evaluation harness for agents.")]
struct Args {
    /// Seed for deterministic evaluation.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Number of evaluation runs.
    #[arg(long, default_value_t = 5000)]
    runs: u32,
    /// Output CSV path.
    #[arg(long, default_value = "results.csv")]
    output: PathBuf,
}

#[derive(Debug)]
struct CaseResult {
    run_id: u32,
    case_id: String,
    difficulty: f32,
    score: f32,
    passed: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let results = run_evaluation(args.seed, args.runs);
    write_results(&args.output, &results).with_context(|| "failed to write results")?;
    let summary = summarize(&results);
    println!(
        "Seed: {} | Runs: {} | Pass rate: {:.2}% | Avg score: {:.3} | Output: {}",
        args.seed,
        args.runs,
        summary.pass_rate * 100.0,
        summary.avg_score,
        args.output.display()
    );
    Ok(())
}

fn run_evaluation(seed: u64, runs: u32) -> Vec<CaseResult> {
    (0..runs)
        .map(|run_id| evaluate_case(seed, run_id))
        .collect()
}

fn evaluate_case(seed: u64, run_id: u32) -> CaseResult {
    let digest = hash_seed(seed, run_id);
    let case_id = to_hex(&digest);
    let difficulty = digest[0] as f32 / 255.0;
    let rng_seed = u64::from_le_bytes(digest[..8].try_into().unwrap());
    let mut rng = StdRng::seed_from_u64(rng_seed);
    let base = 0.45 + rng.gen_range(0.0..0.55);
    let score = (base * (1.0 - difficulty)).clamp(0.0, 1.0);
    let passed = score >= 0.6;
    CaseResult {
        run_id,
        case_id,
        difficulty,
        score,
        passed,
    }
}

fn hash_seed(seed: u64, run_id: u32) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(run_id.to_le_bytes());
    hasher.finalize().into()
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(hex_char(byte >> 4));
        out.push(hex_char(byte & 0x0f));
    }
    out
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => '?',
    }
}

fn write_results(path: &PathBuf, results: &[CaseResult]) -> Result<()> {
    let file = File::create(path).with_context(|| format!("unable to create {}", path.display()))?;
    let mut writer = Writer::from_writer(file);
    writer.write_record(["run_id", "case_id", "difficulty", "score", "passed"])?;
    for result in results {
        writer.write_record([
            result.run_id.to_string(),
            result.case_id.clone(),
            format!("{:.3}", result.difficulty),
            format!("{:.3}", result.score),
            result.passed.to_string(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

struct Summary {
    pass_rate: f32,
    avg_score: f32,
}

fn summarize(results: &[CaseResult]) -> Summary {
    let total = results.len() as f32;
    let pass_count = results.iter().filter(|r| r.passed).count() as f32;
    let avg_score = results.iter().map(|r| r.score).sum::<f32>() / total.max(1.0);
    Summary {
        pass_rate: pass_count / total.max(1.0),
        avg_score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_results() {
        let first = run_evaluation(7, 5);
        let second = run_evaluation(7, 5);
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.case_id, b.case_id);
            assert!((a.score - b.score).abs() < f32::EPSILON);
            assert_eq!(a.passed, b.passed);
        }
    }
}
