use anyhow::{Context, Result};
use csv::Writer;
use rand::{rngs::StdRng, Rng, SeedableRng};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use rayon::prelude::*;

use crate::model::{CaseResult, Summary, ThoughtEvent};

/// Sequential evaluation
pub fn run_sequential(seed: u64, runs: u32) -> Vec<CaseResult> {
    (0..runs).map(|id| evaluate_case(seed, id)).collect()
}

/// Parallel evaluation
pub fn run_parallel(seed: u64, runs: u32) -> Vec<CaseResult> {
    (0..runs)
        .into_par_iter()
        .map(|id| evaluate_case(seed, id))
        .collect()
}

/// Evaluate one deterministic case
pub fn evaluate_case(seed: u64, run_id: u32) -> CaseResult {
    let digest = hash_seed(seed, run_id);
    let case_id = to_hex(&digest);

    let difficulty = digest[0] as f32 / 255.0;
    let rng_seed = u64::from_le_bytes(digest[..8].try_into().unwrap());

    let mut rng = StdRng::seed_from_u64(rng_seed);
    let base = 0.45 + rng.gen_range(0.0..0.55);

    let score = (base * (1.0 - difficulty)).clamp(0.0, 1.0);
    let passed = score >= 0.5;

    let thoughts = vec![
        ThoughtEvent {
            step: 0,
            role: "system".into(),
            content: format!("Initializing difficulty {:.2}", difficulty),
        },
        ThoughtEvent {
            step: 1,
            role: "assistant".into(),
            content: format!("Generated score {:.3}", score),
        },
        ThoughtEvent {
            step: 2,
            role: "assistant".into(),
            content: if passed {
                "Decision: PASS".into()
            } else {
                "Decision: FAIL".into()
            },
        },
    ];

    CaseResult {
        run_id,
        case_id,
        difficulty,
        score,
        passed,
        thoughts,
    }
}

/// Write CSV results
pub fn write_results(path: &PathBuf, results: &[CaseResult]) -> Result<()> {
    let mut writer = Writer::from_path(path).with_context(|| "failed to open CSV output")?;

    writer.write_record(["run_id", "case_id", "difficulty", "score", "passed"])?;

    for r in results {
        writer.write_record([
            r.run_id.to_string(),
            r.case_id.clone(),
            format!("{:.3}", r.difficulty),
            format!("{:.3}", r.score),
            r.passed.to_string(),
        ])?;
    }

    writer.flush()?;
    Ok(())
}

/// Summary statistics
pub fn summarize(results: &[CaseResult]) -> Summary {
    let total = results.len() as f32;
    let pass = results.iter().filter(|r| r.passed).count() as f32;

    let avg = results.iter().map(|r| r.score).sum::<f32>() / total.max(1.0);

    Summary {
        pass_rate: pass / total.max(1.0),
        avg_score: avg,
    }
}

/// Hash seed+run_id → deterministic digest
fn hash_seed(seed: u64, run_id: u32) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(run_id.to_le_bytes());
    hasher.finalize().into()
}

/// Digest → hex case_id
fn to_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
