fn main() {
    println!("Hello, world!");
use anyhow::{Context, Result};
use clap::Parser;
use csv::Writer;
use rand::{rngs::StdRng, Rng, SeedableRng};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::fs::{self, File};
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
    /// Disable terminal UI output.
    #[arg(long)]
    no_tui: bool,
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
    if !args.no_tui {
        render_tui(&args, &results, &summary);
    }
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
@@ -105,41 +109,387 @@ fn write_results(path: &PathBuf, results: &[CaseResult]) -> Result<()> {
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

struct HardwareSnapshot {
    cpu_brand: String,
    logical_cores: usize,
    total_memory_gb: Option<f32>,
    os: String,
    os_pretty: Option<String>,
    accelerator: String,
}

struct TelemetryMetrics {
    min_score: f32,
    max_score: f32,
    median_score: f32,
    p90_score: f32,
    std_dev_score: f32,
    score_difficulty_corr: f32,
    pass_entropy_bits: f32,
}

struct ConfigSignal {
    label: &'static str,
    path: PathBuf,
    exists: bool,
    bytes: Option<u64>,
}

// TODO: Replace with the official Cogitator logo asset once provided.
const COGITATOR_LOGO: &str = r#"
   ██████╗  ██████╗  ██████╗ ██╗████████╗ █████╗ ████████╗ ██████╗ ██████╗
  ██╔════╝ ██╔═══██╗██╔════╝ ██║╚══██╔══╝██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗
  ██║      ██║   ██║██║  ███╗██║   ██║   ███████║   ██║   ██║   ██║██████╔╝
  ██║      ██║   ██║██║   ██║██║   ██║   ██╔══██║   ██║   ██║   ██║██╔══██╗
  ╚██████╗ ╚██████╔╝╚██████╔╝██║   ██║   ██║  ██║   ██║   ╚██████╔╝██║  ██║
   ╚═════╝  ╚═════╝  ╚═════╝ ╚═╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝  ╚═╝
"#;

fn render_tui(args: &Args, results: &[CaseResult], summary: &Summary) {
    let hardware = capture_hardware();
    let metrics = compute_metrics(results);
    let configs = collect_config_signals();
    let pass_count = results.iter().filter(|r| r.passed).count();
    let fail_count = results.len().saturating_sub(pass_count);
    println!(
        "{}\n{}",
        COGITATOR_LOGO.trim_end(),
        "=".repeat(70)
    );
    println!(" Mission Control :: Deterministic Evaluation Harness");
    println!("{}", "-".repeat(70));
    println!(" Seed            : {}", args.seed);
    println!(" Runs            : {}", args.runs);
    println!(" Output CSV      : {}", args.output.display());
    println!("{}", "-".repeat(70));
    println!(" Results");
    println!("  [PASS] Passed   : {}", pass_count);
    println!("  [FAIL] Failed   : {}", fail_count);
    println!("  [RATE] PassRate : {:.2}%", summary.pass_rate * 100.0);
    println!("  [SCORE] Avg     : {:.3}", summary.avg_score);
    println!("{}", "-".repeat(70));
    println!(" Reasoning Trace (High-Level, Non-Sensitive)");
    println!("  1) Parse CLI + seed");
    println!("  2) Hash seed + run_id to derive case difficulty");
    println!("  3) Generate deterministic score");
    println!("  4) Aggregate CSV + summary");
    println!("{}", "-".repeat(70));
    println!(" Thought Telemetry (High-Level, Non-Sensitive)");
    println!("  TL-0 : deterministic seed and run_id");
    println!("  TL-1 : hash -> case_id + difficulty");
    println!("  TL-2 : score synthesis (seeded RNG)");
    println!("  TL-3 : pass/fail gate and aggregate");
    println!("  TL-4 : CSV persistence + summary");
    println!("{}", "-".repeat(70));
    println!(" LLM Component Map");
    println!("  PF Prompt Fidelity     : deterministic seed + hash");
    println!("  EM Evaluation Matrix   : difficulty, score, pass");
    println!("  TM Telemetry           : CSV output + summary");
    println!("  TR Traceability        : stable case_id per run");
    println!("{}", "-".repeat(70));
    println!(" Telemetry Metrics");
    println!(
        "  Score Range       : {:.3} → {:.3}",
        metrics.min_score, metrics.max_score
    );
    println!(
        "  Median / P90      : {:.3} / {:.3}",
        metrics.median_score, metrics.p90_score
    );
    println!("  Volatility        : σ={:.3}", metrics.std_dev_score);
    println!(
        "  Score-Difficulty  : r={:.3}",
        metrics.score_difficulty_corr
    );
    println!(
        "  Pass Entropy      : {:.3} bits",
        metrics.pass_entropy_bits
    );
    println!("{}", "-".repeat(70));
    println!(" Hardware Snapshot");
    println!("  CPU Model         : {}", hardware.cpu_brand);
    println!("  Logical Cores     : {}", hardware.logical_cores);
    match hardware.total_memory_gb {
        Some(memory_gb) => println!("  Total Memory      : {:.2} GB", memory_gb),
        None => println!("  Total Memory      : Unknown"),
    }
    println!("  OS                : {}", hardware.os);
    if let Some(pretty) = &hardware.os_pretty {
        println!("  OS Pretty         : {}", pretty);
    }
    println!("  Accelerator       : {}", hardware.accelerator);
    println!("{}", "-".repeat(70));
    println!(" Local Config Signals");
    for signal in &configs {
        let status = if signal.exists { "FOUND" } else { "MISSING" };
        let size = signal
            .bytes
            .map(|bytes| format!("{} B", bytes))
            .unwrap_or_else(|| "Unknown".to_string());
        println!(
            "  {:<14} : {:<7} | {} | {}",
            signal.label,
            status,
            signal.path.display(),
            size
        );
    }
    println!("{}", "-".repeat(70));
    println!(" Scaling & Compatibility");
    println!("  Single Node       : {} threads", hardware.logical_cores.max(1));
    println!("  Multi-Socket      : partition by run_id ranges");
    println!("  Multi-Node        : shard runs across nodes, merge CSVs");
    println!("  Supercomputer     : deterministic seeds per shard");
    println!("  GPU Offload       : device detected → plan for batch shards");
    println!("  NixOS             : prefer reproducible runs + pinned deps");
    println!("{}", "=".repeat(70));
}

fn capture_hardware() -> HardwareSnapshot {
    let logical_cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let cpu_brand = detect_cpu_brand();
    let total_memory_gb = detect_total_memory_gb();
    let os = std::env::consts::OS.to_string();
    let os_pretty = detect_os_pretty();
    let accelerator = detect_accelerator();
    HardwareSnapshot {
        cpu_brand,
        logical_cores,
        total_memory_gb,
        os,
        os_pretty,
        accelerator,
    }
}

fn compute_metrics(results: &[CaseResult]) -> TelemetryMetrics {
    let mut scores: Vec<f32> = results.iter().map(|r| r.score).collect();
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min_score = *scores.first().unwrap_or(&0.0);
    let max_score = *scores.last().unwrap_or(&0.0);
    let median_score = percentile(&scores, 0.5);
    let p90_score = percentile(&scores, 0.9);
    let score_mean = mean(&scores);
    let std_dev_score = std_dev(&scores, score_mean);
    let difficulty_mean = mean(
        &results
            .iter()
            .map(|r| r.difficulty)
            .collect::<Vec<f32>>(),
    );
    let score_difficulty_corr = correlation(results, score_mean, difficulty_mean);
    let pass_rate = results
        .iter()
        .filter(|r| r.passed)
        .count() as f32
        / results.len().max(1) as f32;
    let pass_entropy_bits = entropy_bits(pass_rate);
    TelemetryMetrics {
        min_score,
        max_score,
        median_score,
        p90_score,
        std_dev_score,
        score_difficulty_corr,
        pass_entropy_bits,
    }
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

fn std_dev(values: &[f32], mean: f32) -> f32 {
    if values.len() < 2 {
        return 0.0;
    }
    let variance = values
        .iter()
        .map(|v| {
            let diff = v - mean;
            diff * diff
        })
        .sum::<f32>()
        / values.len() as f32;
    variance.sqrt()
}

fn percentile(values: &[f32], quantile: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let clamped = quantile.clamp(0.0, 1.0);
    let idx = ((values.len() - 1) as f32 * clamped).round() as usize;
    values[idx.min(values.len() - 1)]
}

fn correlation(results: &[CaseResult], score_mean: f32, difficulty_mean: f32) -> f32 {
    if results.len() < 2 {
        return 0.0;
    }
    let mut cov = 0.0;
    let mut score_var = 0.0;
    let mut difficulty_var = 0.0;
    for result in results {
        let score_diff = result.score - score_mean;
        let difficulty_diff = result.difficulty - difficulty_mean;
        cov += score_diff * difficulty_diff;
        score_var += score_diff * score_diff;
        difficulty_var += difficulty_diff * difficulty_diff;
    }
    let denom = (score_var * difficulty_var).sqrt();
    if denom == 0.0 {
        0.0
    } else {
        cov / denom
    }
}

fn entropy_bits(pass_rate: f32) -> f32 {
    let p = pass_rate.clamp(0.0, 1.0);
    let q = 1.0 - p;
    let mut entropy = 0.0;
    if p > 0.0 {
        entropy -= p * p.log2();
    }
    if q > 0.0 {
        entropy -= q * q.log2();
    }
    entropy
}

fn detect_cpu_brand() -> String {
    if cfg!(target_os = "linux") {
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
            for line in cpuinfo.lines() {
                if let Some(rest) = line.strip_prefix("model name") {
                    if let Some((_key, value)) = rest.split_once(':') {
                        return value.trim().to_string();
                    }
                }
            }
        }
    }
    std::env::var("PROCESSOR_IDENTIFIER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Unknown CPU".to_string())
}

fn detect_total_memory_gb() -> Option<f32> {
    if cfg!(target_os = "linux") {
        let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
        for line in meminfo.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if let Some(kb_str) = parts.first() {
                    if let Ok(kb) = kb_str.parse::<f32>() {
                        return Some(kb / 1024.0 / 1024.0);
                    }
                }
            }
        }
    }
    None
}

fn detect_os_pretty() -> Option<String> {
    if let Ok(contents) = fs::read_to_string("/etc/os-release") {
        for line in contents.lines() {
            if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                return Some(rest.trim_matches('"').to_string());
            }
        }
    }
    None
}

fn detect_accelerator() -> String {
    if std::env::var("CUDA_VISIBLE_DEVICES").is_ok() {
        return "CUDA (env)".to_string();
    }
    if std::env::var("ROCR_VISIBLE_DEVICES").is_ok() {
        return "ROCm (env)".to_string();
    }
    if std::env::var("METAL_DEVICE_WRAPPER_TYPE").is_ok() {
        return "Metal (env)".to_string();
    }
    if let Ok(value) = std::env::var("NVIDIA_VISIBLE_DEVICES") {
        if !value.trim().is_empty() {
            return format!("NVIDIA ({})", value);
        }
    }
    "Unknown / CPU-only".to_string()
}

fn collect_config_signals() -> Vec<ConfigSignal> {
    let mut signals = Vec::new();
    let home = std::env::var("HOME").map(PathBuf::from).ok();
    let candidates: Vec<(&'static str, Option<PathBuf>)> = vec![
        ("nixos", Some(PathBuf::from("/etc/nixos/configuration.nix"))),
        (
            "home.nix",
            home.as_ref().map(|h| h.join(".config/nixpkgs/home.nix")),
        ),
        (
            "hyprland",
            home.as_ref().map(|h| h.join(".config/hypr/hyprland.conf")),
        ),
    ];
    for (label, path_opt) in candidates {
        if let Some(path) = path_opt {
            let metadata = fs::metadata(&path).ok();
            signals.push(ConfigSignal {
                label,
                path,
                exists: metadata.is_some(),
                bytes: metadata.as_ref().map(|m| m.len()),
            });
        }
    }
    signals
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

