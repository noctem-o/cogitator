use anyhow::Result;
use csv::Writer;
use rand::{rngs::StdRng, Rng, SeedableRng};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::model::{CaseResult, Summary, ThoughtEvent, TraceEvent, TRACE_SCHEMA_VERSION};

pub struct RunOutput {
    pub results: Vec<CaseResult>,
    pub trace: Vec<TraceEvent>,
    pub total_rng_calls: u64,
}

struct CaseRun {
    result: CaseResult,
    events: Vec<TraceEvent>,
    rng_calls: u32,
}

/// Sequential evaluation (deterministic)
#[allow(dead_code)]
pub fn run_sequential(seed: u64, run_ids: &[u32]) -> Vec<CaseResult> {
    run_ids
        .iter()
        .map(|&id| evaluate_case(seed, id).result)
        .collect()
}

/// Parallel evaluation (deterministic ordering by run_id)
#[allow(dead_code)]
pub fn run_parallel(seed: u64, run_ids: &[u32]) -> Vec<CaseResult> {
    run_with_trace(seed, run_ids, true).results
}

/// Run evaluation with a canonical trace and entropy accounting.
pub fn run_with_trace(seed: u64, run_ids: &[u32], parallel: bool) -> RunOutput {
    let n = run_ids.len();
    let mut runs: Vec<Option<CaseRun>> = (0..n).map(|_| None).collect();

    if parallel {
        runs.par_iter_mut().enumerate().for_each(|(i, slot)| {
            let run_id = run_ids[i];
            *slot = Some(evaluate_case(seed, run_id));
        });
    } else {
        for (i, run_id) in run_ids.iter().copied().enumerate() {
            runs[i] = Some(evaluate_case(seed, run_id));
        }
    }

    let mut results = Vec::with_capacity(n);
    let mut trace = Vec::new();
    let mut total_rng_calls: u64 = 0;

    for item in runs.into_iter() {
        let case_run = item.expect("slot must be filled");
        total_rng_calls += case_run.rng_calls as u64;
        results.push(case_run.result);
        trace.extend(case_run.events);
    }

    trace.sort_by_key(|event| (event.run_id, event.step));

    RunOutput {
        results,
        trace,
        total_rng_calls,
    }
}

/// Evaluate one deterministic case using fixed-point arithmetic for cross-platform determinism
///
/// Uses parts-per-million (ppm) representation to avoid floating-point non-determinism
/// across CPU architectures (x86 vs ARM vs RISC-V).
fn evaluate_case(seed: u64, run_id: u32) -> CaseRun {
    let digest = hash_seed(seed, run_id);
    let case_id = crate::hex::encode(&digest);
    
    // Use fixed-point arithmetic (parts-per-million) for deterministic scores
    // difficulty_ppm: 0 to 1_000_000 representing 0.0 to 1.0
    let difficulty_ppm = (digest[0] as u32 * 1_000_000) / 255;
    
    let rng_seed = u64::from_le_bytes(digest[..8].try_into().unwrap());
    let mut rng = StdRng::seed_from_u64(rng_seed);
    
    // Generate base in range [450_000, 1_000_000] parts-per-million (0.45 to 1.0)
    let base_ppm = 450_000 + rng.gen_range(0..550_001);
    let rng_calls = 1;
    
    // Compute score = base * (1.0 - difficulty) using fixed-point
    // score_ppm = base_ppm * (1_000_000 - difficulty_ppm) / 1_000_000
    let score_ppm = (base_ppm as u64 * (1_000_000 - difficulty_ppm) as u64) / 1_000_000;
    let score_ppm = score_ppm.min(1_000_000) as u32;
    
    let passed = score_ppm >= 500_000; // threshold: 0.5
    
    // Convert to f32 only for display (CSV output) - not part of witness commitment
    let difficulty = difficulty_ppm as f32 / 1_000_000.0;
    let score = score_ppm as f32 / 1_000_000.0;
    
    let thoughts = vec![
        ThoughtEvent {
            step: 0,
            role: "system".into(),
            content: format!("Initializing difficulty {:.6}", difficulty),
            entropy_bits: 0,
            rng_calls: 0,
        },
        ThoughtEvent {
            step: 1,
            role: "assistant".into(),
            content: format!("Generated score {:.6}", score),
            entropy_bits: 32,
            rng_calls,
        },
        ThoughtEvent {
            step: 2,
            role: "assistant".into(),
            content: if passed {
                "Decision: PASS".into()
            } else {
                "Decision: FAIL".into()
            },
            entropy_bits: 0,
            rng_calls: 0,
        },
    ];
    
    let events = thoughts
        .iter()
        .map(|thought| TraceEvent {
            schema_version: TRACE_SCHEMA_VERSION,
            run_id,
            case_id: case_id.clone(),
            step: thought.step,
            role: thought.role.clone(),
            content: thought.content.clone(),
            entropy_bits: thought.entropy_bits,
            rng_calls: thought.rng_calls,
        })
        .collect();
    
    CaseRun {
        result: CaseResult {
            run_id,
            case_id,
            difficulty,
            score,
            passed,
            rng_calls,
            thoughts,
        },
        events,
        rng_calls,
    }
}

/// Write CSV results (stable ordering + ergonomic Path API)
pub fn write_results(path: &Path, results: &[CaseResult]) -> Result<()> {
    crate::io_utils::write_atomic(path, "results.csv", |file| {
        let mut writer = Writer::from_writer(file);
        writer.write_record([
            "run_id",
            "case_id",
            "difficulty",
            "score",
            "passed",
            "rng_calls",
        ])?;

        // Belt-and-suspenders: ensure deterministic CSV row order.
        let mut ordered: Vec<&CaseResult> = results.iter().collect();
        ordered.sort_by_key(|r| r.run_id);

        for r in ordered {
            writer.write_record([
                r.run_id.to_string(),
                r.case_id.clone(),
                // Increased precision from 3 to 6 decimals for better reproducibility
                format!("{:.6}", r.difficulty),
                format!("{:.6}", r.score),
                r.passed.to_string(),
                r.rng_calls.to_string(),
            ])?;
        }

        writer.flush()?;
        Ok(())
    })
}

/// Summary statistics (stable + less float wobble)
pub fn summarize(results: &[CaseResult]) -> Summary {
    let total = results.len() as f64;
    if total == 0.0 {
        return Summary {
            pass_rate: 0.0,
            avg_score: 0.0,
        };
    }

    let pass = results.iter().filter(|r| r.passed).count() as f64;
    let avg = results.iter().map(|r| r.score as f64).sum::<f64>() / total;

    Summary {
        pass_rate: (pass / total) as f32,
        avg_score: avg as f32,
    }
}

/// Structured summary with counts (for analysis bundle)
pub fn summarize_with_counts(results: &[CaseResult]) -> (Summary, usize, usize) {
    let summary = summarize(results);
    let pass_count = results.iter().filter(|r| r.passed).count();
    let fail_count = results.len().saturating_sub(pass_count);
    (summary, pass_count, fail_count)
}

/// Hash seed+run_id → deterministic digest
fn hash_seed(seed: u64, run_id: u32) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(run_id.to_le_bytes());
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_point_arithmetic() {
        // Verify fixed-point conversion matches expected values
        let test_cases = [
            (0u32, 0.0f32),
            (500_000, 0.5),
            (1_000_000, 1.0),
            (250_000, 0.25),
            (750_000, 0.75),
        ];
        
        for (ppm, expected_float) in test_cases {
            let computed = ppm as f32 / 1_000_000.0;
            assert!((computed - expected_float).abs() < 1e-6);
        }
    }

    #[test]
    fn test_deterministic_case_evaluation() {
        let seed = 42u64;
        let run_id = 0u32;
        
        let case1 = evaluate_case(seed, run_id);
        let case2 = evaluate_case(seed, run_id);
        
        assert_eq!(case1.result.case_id, case2.result.case_id);
        assert_eq!(case1.result.passed, case2.result.passed);
        assert_eq!(case1.result.score.to_bits(), case2.result.score.to_bits());
    }
}
