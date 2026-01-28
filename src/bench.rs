use anyhow::{Context, Result};
use csv::Writer;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

use crate::eval;
use crate::model::{WitnessedMetadata, TRACE_SCHEMA_VERSION};
use crate::witness;

pub const BENCH_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct BenchConfig {
    pub seed: u64,
    pub runs: u32,
    pub threads: Vec<usize>,
    pub repeat: u32,
    pub determinism_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchBuildInfo {
    pub git_rev: Option<String>,
    pub rustc_version: Option<String>,
    pub cargo_version: Option<String>,
    pub nix_store_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchEntry {
    pub threads: usize,
    pub repeats: u32,
    pub runs: u32,
    pub total_wall_secs: f64,
    pub throughput_runs_per_sec: f64,
    pub witness_root: String,
    pub total_rng_calls: u64,
    pub memory_peak_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeterminismCheck {
    pub passed: bool,
    pub baseline_threads: usize,
    pub mismatches: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchReport {
    pub schema_version: u32,
    pub seed: u64,
    pub runs: u32,
    pub repeat: u32,
    pub entries: Vec<BenchEntry>,
    pub build: BenchBuildInfo,
    pub determinism_check: Option<DeterminismCheck>,
}

pub fn run_bench(config: BenchConfig, build: BenchBuildInfo) -> Result<BenchReport> {
    let run_ids: Vec<u32> = (0..config.runs).collect();
    let mut entries = Vec::new();
    let mut determinism_state = DeterminismState::default();

    for threads in &config.threads {
        let mut total_duration = 0.0_f64;
        let mut witness_root = String::new();
        let mut total_rng_calls = 0_u64;

        for _ in 0..config.repeat {
            let started = Instant::now();
            let output = eval::run_with_trace(config.seed, &run_ids, true, *threads)?;
            let elapsed = started.elapsed().as_secs_f64();
            total_duration += elapsed;
            total_rng_calls = output.total_rng_calls;

            let witnessed_metadata = WitnessedMetadata {
                schema_version: TRACE_SCHEMA_VERSION,
                seed: config.seed,
                requested_runs: config.runs,
                executed_runs: config.runs,
                parallel: true,
                parallel_strategy: "rayon/ordered-run-ids".to_string(),
                case_filter: None,
                entropy_sources: vec!["rng:StdRng(seed)".to_string()],
                total_rng_calls,
            };
            witness_root = witness::compute_witness_root(&witnessed_metadata, &output.trace)?;

            if config.determinism_check {
                determinism_state.observe(*threads, &witness_root, &output.trace)?;
            }
        }

        let total_runs = config.runs as f64 * config.repeat as f64;
        let throughput = if total_duration > 0.0 {
            total_runs / total_duration
        } else {
            0.0
        };

        entries.push(BenchEntry {
            threads: *threads,
            repeats: config.repeat,
            runs: config.runs,
            total_wall_secs: total_duration,
            throughput_runs_per_sec: throughput,
            witness_root,
            total_rng_calls,
            memory_peak_bytes: None,
        });
    }

    let determinism_check = if config.determinism_check {
        Some(determinism_state.finalize())
    } else {
        None
    };

    if let Some(check) = &determinism_check {
        if !check.passed {
            anyhow::bail!("determinism check failed: {:?}", check.mismatches);
        }
    }

    Ok(BenchReport {
        schema_version: BENCH_SCHEMA_VERSION,
        seed: config.seed,
        runs: config.runs,
        repeat: config.repeat,
        entries,
        build,
        determinism_check,
    })
}

pub fn write_csv(path: &Path, report: &BenchReport) -> Result<()> {
    let mut writer = Writer::from_path(path).with_context(|| "failed to create bench.csv")?;
    writer.write_record([
        "threads",
        "repeats",
        "runs",
        "total_wall_secs",
        "throughput_runs_per_sec",
        "witness_root",
        "total_rng_calls",
        "memory_peak_bytes",
    ])?;
    for entry in &report.entries {
        writer.write_record([
            entry.threads.to_string(),
            entry.repeats.to_string(),
            entry.runs.to_string(),
            format!("{:.6}", entry.total_wall_secs),
            format!("{:.6}", entry.throughput_runs_per_sec),
            entry.witness_root.clone(),
            entry.total_rng_calls.to_string(),
            entry
                .memory_peak_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

#[derive(Default)]
struct DeterminismState {
    baseline_threads: Option<usize>,
    baseline_root: Option<String>,
    baseline_events: Option<Vec<Vec<u8>>>,
    mismatches: Vec<String>,
}

impl DeterminismState {
    fn observe(
        &mut self,
        threads: usize,
        witness_root: &str,
        trace: &[crate::model::TraceEvent],
    ) -> Result<()> {
        let encoded_events: Vec<Vec<u8>> = trace
            .iter()
            .map(crate::trace::encode_event)
            .collect::<Result<Vec<_>>>()?;

        match (
            &self.baseline_root,
            &self.baseline_events,
            self.baseline_threads,
        ) {
            (None, None, None) => {
                self.baseline_threads = Some(threads);
                self.baseline_root = Some(witness_root.to_string());
                self.baseline_events = Some(encoded_events);
            }
            (Some(root), Some(events), Some(base_threads)) => {
                if root != witness_root {
                    self.mismatches.push(format!(
                        "witness_root mismatch: baseline threads {} ({}) vs threads {} ({})",
                        base_threads, root, threads, witness_root
                    ));
                }
                if events.len() != encoded_events.len() {
                    self.mismatches.push(format!(
                        "trace length mismatch: baseline {} vs threads {} ({})",
                        events.len(),
                        threads,
                        encoded_events.len()
                    ));
                } else {
                    for (idx, (base, candidate)) in
                        events.iter().zip(encoded_events.iter()).enumerate()
                    {
                        if base != candidate {
                            self.mismatches.push(format!(
                                "trace event mismatch at index {} for threads {}",
                                idx, threads
                            ));
                            break;
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn finalize(self) -> DeterminismCheck {
        DeterminismCheck {
            passed: self.mismatches.is_empty(),
            baseline_threads: self.baseline_threads.unwrap_or(1),
            mismatches: self.mismatches,
        }
    }
}
