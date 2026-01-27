use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

mod eval;
mod model;
mod trace;
mod verify;
mod witness;

#[cfg(feature = "tui")]
mod tui;

/// CLI entrypoint
#[derive(Parser, Debug)]
#[command(
    name = "cogitator",
    version,
    about = "Deterministic evaluation harness"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: CommandLine,
}

#[derive(Subcommand, Debug)]
pub enum CommandLine {
    Run(RunArgs),
    Verify(VerifyArgs),
}

/// Run a deterministic evaluation and emit artifacts.
#[derive(Args, Debug)]
pub struct RunArgs {
    #[arg(long, default_value_t = 42)]
    pub seed: u64,

    #[arg(long, default_value_t = 5000)]
    pub runs: u32,

    #[arg(long)]
    pub case: Option<u32>,

    #[arg(long, default_value = "out")]
    pub out_dir: PathBuf,

    #[arg(long)]
    pub clean: bool,

    #[arg(long)]
    pub no_tui: bool,

    #[arg(long, default_value_t = true)]
    pub parallel: bool,

    #[arg(long)]
    pub created_at: Option<String>,
}

/// Verify a trace against an expected witness root.
#[derive(Args, Debug)]
pub struct VerifyArgs {
    #[arg(long, default_value = "meta.json")]
    pub meta: PathBuf,

    #[arg(long, default_value = "trace.jsonl")]
    pub trace: PathBuf,

    #[arg(long)]
    pub expect: Option<String>,

    #[arg(long)]
    pub witness: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        CommandLine::Run(args) => run(args),
        CommandLine::Verify(args) => verify_cmd(args),
    }
}

fn run(args: RunArgs) -> Result<()> {
    let run_ids: Vec<u32> = match args.case {
        Some(case_id) => vec![case_id],
        None => (0..args.runs).collect(),
    };

    let output = eval::run_with_trace(args.seed, &run_ids, args.parallel);
    let (summary, pass_count, fail_count) = eval::summarize_with_counts(&output.results);

    if args.clean && args.out_dir.exists() {
        fs::remove_dir_all(&args.out_dir).with_context(|| "failed to clean output dir")?;
    }

    fs::create_dir_all(&args.out_dir).with_context(|| "failed to create output dir")?;

    let metadata = build_metadata(&args, output.total_rng_calls, run_ids.len() as u32);
    let metadata_bytes = trace::encode_metadata(&metadata)?;
    let meta_path = args.out_dir.join("meta.json");
    fs::write(&meta_path, &metadata_bytes).with_context(|| "failed to write meta.json")?;

    let trace_path = args.out_dir.join("trace.jsonl");
    write_trace(&trace_path, &output.trace)?;

    let witness_root = compute_witness_root(&metadata.witnessed, &output.trace)?;
    let witness_path = args.out_dir.join("witness_root.txt");
    fs::write(&witness_path, format!("{}\n", witness_root))
        .with_context(|| "failed to write witness_root.txt")?;

    let csv_path = args.out_dir.join("results.csv");
    eval::write_results(&csv_path, &output.results)?;

    let results_json_path = args.out_dir.join("results.json");
    write_json(&results_json_path, &output.results, "results.json")?;

    let summary_json_path = args.out_dir.join("summary.json");
    write_json(&summary_json_path, &summary, "summary.json")?;

    let manifest = model::ArtifactManifest {
        meta_json: meta_path.display().to_string(),
        trace_jsonl: trace_path.display().to_string(),
        results_csv: csv_path.display().to_string(),
        results_json: results_json_path.display().to_string(),
        summary_json: summary_json_path.display().to_string(),
        witness_root_txt: witness_path.display().to_string(),
        analysis_json: args.out_dir.join("analysis.json").display().to_string(),
    };

    let analysis_bundle = model::AnalysisBundle {
        metadata: metadata.clone(),
        summary: summary.clone(),
        results: output.results.clone(),
        witness_root: witness_root.clone(),
        artifacts: manifest.clone(),
    };

    let analysis_path = args.out_dir.join("analysis.json");
    write_json(&analysis_path, &analysis_bundle, "analysis.json")?;

    let tui_enabled = !args.no_tui && cfg!(feature = "tui");
    if tui_enabled {
        #[cfg(feature = "tui")]
        tui::launch(
            args.seed,
            run_ids.len() as u32,
            &output.results,
            &summary,
            &metadata,
            &manifest,
        )?;
    } else {
        if !args.no_tui {
            println!("TUI disabled (missing feature).");
        }
        println!("Artifacts:");
        println!("  meta.json: {}", meta_path.display());
        println!("  trace.jsonl: {}", trace_path.display());
        println!("  results.csv: {}", csv_path.display());
        println!("  results.json: {}", results_json_path.display());
        println!("  summary.json: {}", summary_json_path.display());
        println!("  analysis.json: {}", analysis_path.display());
        println!("  witness_root.txt: {}", witness_path.display());
    }

    println!(
        "Seed={} Runs={} PassRate={:.2}% AvgScore={:.3} OutputDir={} WitnessRoot={}",
        args.seed,
        run_ids.len(),
        summary.pass_rate * 100.0,
        summary.avg_score,
        args.out_dir.display(),
        witness_root
    );
    println!(
        "Passed={} Failed={} total_rng_calls={}",
        pass_count, fail_count, output.total_rng_calls
    );

    Ok(())
}

fn verify_cmd(args: VerifyArgs) -> Result<()> {
    let expect = match (args.expect, args.witness) {
        (Some(expect), _) => expect,
        (None, Some(path)) => read_trimmed(&path)?,
        (None, None) => anyhow::bail!("--expect or --witness is required"),
    };

    let computed = verify::verify(&args.meta, &args.trace, &expect)?;
    println!("Verified witness_root={}", computed);
    Ok(())
}

fn write_trace(path: &Path, events: &[model::TraceEvent]) -> Result<()> {
    let file = File::create(path).with_context(|| "failed to create trace.jsonl")?;
    let mut writer = BufWriter::new(file);

    for event in events {
        let bytes = trace::encode_event(event)?;
        writer.write_all(&bytes)?;
        writer.write_all(b"\n")?;
    }

    writer.flush()?;
    Ok(())
}

fn compute_witness_root(
    metadata: &model::WitnessedMetadata,
    events: &[model::TraceEvent],
) -> Result<String> {
    let metadata_bytes = trace::encode_witnessed_metadata(metadata)?;
    let mut witness = witness::Witness::new(&metadata_bytes)?;

    for event in events {
        let event_bytes = trace::encode_event(event)?;
        witness.update(&event_bytes)?;
    }

    Ok(witness.finalize_hex())
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    let file = File::create(path).with_context(|| format!("failed to create {}", label))?;
    serde_json::to_writer_pretty(file, value)
        .with_context(|| format!("failed to write {}", label))?;
    Ok(())
}

fn build_metadata(args: &RunArgs, total_rng_calls: u64, executed_runs: u32) -> model::RunMetadata {
    let created_at = resolve_created_at(args);
    let git_rev = git_rev();
    let rustc_version = command_version("rustc");
    let cargo_version = command_version("cargo");
    let nix_store_path = std::env::var("NIX_STORE").ok();
    let variability_factors = build_variability_factors(
        &created_at,
        &git_rev,
        &rustc_version,
        &cargo_version,
        &nix_store_path,
    );

    model::RunMetadata {
        witnessed: model::WitnessedMetadata {
            schema_version: model::TRACE_SCHEMA_VERSION,
            seed: args.seed,
            requested_runs: args.runs,
            executed_runs,
            parallel: args.parallel,
            parallel_strategy: parallel_strategy(args.parallel),
            case_filter: args.case,
            entropy_sources: vec!["rng:StdRng(seed)".to_string()],
            total_rng_calls,
        },
        provenance: model::ProvenanceMetadata {
            created_at,
            git_rev,
            rustc_version,
            cargo_version,
            nix_store_path,
            variability_factors,
        },
    }
}

fn resolve_created_at(args: &RunArgs) -> String {
    if let Some(created_at) = args.created_at.clone() {
        return created_at;
    }

    let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") else {
        return String::new();
    };
    let Ok(secs) = epoch.parse::<i64>() else {
        return String::new();
    };
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

fn parallel_strategy(parallel: bool) -> String {
    if parallel {
        "rayon/ordered-run-ids".to_string()
    } else {
        "sequential".to_string()
    }
}

fn build_variability_factors(
    created_at: &str,
    git_rev: &Option<String>,
    rustc_version: &Option<String>,
    cargo_version: &Option<String>,
    nix_store_path: &Option<String>,
) -> Vec<String> {
    let mut factors = Vec::new();
    if !created_at.is_empty() {
        factors.push("created_at".to_string());
    }
    if git_rev.is_some() {
        factors.push("git_rev".to_string());
    }
    if rustc_version.is_some() {
        factors.push("rustc_version".to_string());
    }
    if cargo_version.is_some() {
        factors.push("cargo_version".to_string());
    }
    if nix_store_path.is_some() {
        factors.push("nix_store_path".to_string());
    }
    factors
}

fn git_rev() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn command_version(command: &str) -> Option<String> {
    let output = Command::new(command).arg("-V").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn read_trimmed(path: &Path) -> Result<String> {
    let content = fs::read_to_string(path).with_context(|| "failed to read witness file")?;
    Ok(content.trim().to_string())
}
