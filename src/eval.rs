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

/// Evaluate one deterministic case
fn evaluate_case(seed: u64, run_id: u32) -> CaseRun {
    let digest = hash_seed(seed, run_id);
    let case_id = to_hex(&digest);

    let difficulty = digest[0] as f32 / 255.0;
    let rng_seed = u64::from_le_bytes(digest[..8].try_into().unwrap());

    let mut rng = StdRng::seed_from_u64(rng_seed);
    let base = 0.45 + rng.gen_range(0.0..0.55);
    let rng_calls = 1;

    let score = (base * (1.0 - difficulty)).clamp(0.0, 1.0);
    let passed = score >= 0.5;

    let thoughts = vec![
        ThoughtEvent {
            step: 0,
            role: "system".into(),
            content: format!("Initializing difficulty {:.2}", difficulty),
            entropy_bits: 0,
            rng_calls: 0,
        },
        ThoughtEvent {
            step: 1,
            role: "assistant".into(),
            content: format!("Generated score {:.3}", score),
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
                // More precision makes diffs and downstream math less cursed.
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

/// Digest → hex case_id
fn to_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
