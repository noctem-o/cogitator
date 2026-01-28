use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use sha2::{Digest, Sha256};

use crate::agent::{Agent, ClawdbotVariant};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

mod agent;
mod bench;
mod demo;
mod drift;
mod eval;
mod model;
mod tooling;
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
    Demo(DemoArgs),
    Bench(BenchArgs),
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
    pub threads: Option<usize>,

    #[arg(long)]
    pub created_at: Option<String>,

    #[arg(long)]
    pub agent: Option<String>,

    #[arg(long)]
    pub replay: Option<PathBuf>,
}

/// Demo scenarios that illustrate drift detection.
#[derive(Args, Debug)]
pub struct DemoArgs {
    #[command(subcommand)]
    pub scenario: DemoScenario,
}

#[derive(Subcommand, Debug)]
pub enum DemoScenario {
    Drift(DemoDriftArgs),
}

#[derive(Args, Debug)]
pub struct DemoDriftArgs {
    #[arg(long, default_value_t = 42)]
    pub seed: u64,

    #[arg(long, default_value = "demo_drift")]
    pub out_dir: PathBuf,

    #[arg(long)]
    pub clean: bool,
}

/// Benchmark and determinism validation.
#[derive(Args, Debug)]
pub struct BenchArgs {
    #[arg(long, default_value_t = 42)]
    pub seed: u64,

    #[arg(long, default_value_t = 5000)]
    pub runs: u32,

    #[arg(long, value_delimiter = ',', default_value = "1,2,4")]
    pub threads: Vec<usize>,

    #[arg(long, default_value_t = 3)]
    pub repeat: u32,

    #[arg(long, default_value = "bench")]
    pub out_dir: PathBuf,

    #[arg(long)]
    pub clean: bool,

    #[arg(long)]
    pub determinism_check: bool,
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
        CommandLine::Demo(args) => run_demo(args),
        CommandLine::Bench(args) => run_bench(args),
    }
}

fn run(args: RunArgs) -> Result<()> {
    if args.agent.is_some() || args.replay.is_some() {
        return run_agent(args);
    }

    let run_ids: Vec<u32> = match args.case {
        Some(case_id) => vec![case_id],
        None => (0..args.runs).collect(),
    };

    let threads = resolve_threads(&args);
    let output = eval::run_with_trace(args.seed, &run_ids, args.parallel, threads)?;
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

    let witness_root = witness::compute_witness_root(&metadata.witnessed, &output.trace)?;
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
        agent_trace_json: None,
        tool_transcript_json: None,
        witness_manifest_json: None,
        hash_chain_txt: None,
        drift_report_json: None,
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

struct ReplayBundle {
    manifest: model::WitnessManifest,
    transcript: tooling::ToolTranscriptRecord,
}

fn load_replay_bundle(path: &Path) -> Result<ReplayBundle> {
    let manifest_path = path.join("witness_manifest.json");
    let manifest_file =
        File::open(&manifest_path).with_context(|| "failed to open witness_manifest.json")?;
    let manifest: model::WitnessManifest = serde_json::from_reader(manifest_file)
        .with_context(|| "failed to parse witness_manifest.json")?;

    let transcript_path = drift::artifact_path(&manifest, "tool_transcript.json")?;
    let transcript = tooling::read_transcript(Path::new(&transcript_path))?;
    Ok(ReplayBundle {
        manifest,
        transcript,
    })
}

fn run_agent(args: RunArgs) -> Result<()> {
    let run_ids: Vec<u32> = match args.case {
        Some(case_id) => vec![case_id],
        None => (0..args.runs).collect(),
    };

    if args.replay.is_some() && run_ids.len() != 1 {
        anyhow::bail!("--replay requires a single --case or --runs 1");
    }

    if args.clean && args.out_dir.exists() {
        fs::remove_dir_all(&args.out_dir).with_context(|| "failed to clean output dir")?;
    }
    fs::create_dir_all(&args.out_dir).with_context(|| "failed to create output dir")?;

    let replay_bundle = if let Some(replay_dir) = args.replay.as_ref() {
        Some(load_replay_bundle(replay_dir)?)
    } else {
        None
    };

    let agent_name = args.agent.clone().or_else(|| {
        replay_bundle
            .as_ref()
            .map(|bundle| bundle.manifest.agent.clone())
    });
    let agent_name = agent_name.unwrap_or_else(|| "clawdbot".to_string());

    let agent_variant = match agent_name.as_str() {
        "clawdbot" => ClawdbotVariant::Baseline,
        "clawdbot-regressed" => ClawdbotVariant::Regressed,
        _ => anyhow::bail!("unsupported agent: {}", agent_name),
    };

    let tui_enabled = !args.no_tui && cfg!(feature = "tui");

    let single_run = run_ids.len() == 1;

    for &run_id in run_ids.iter() {
        let case_id = derive_case_id(args.seed, run_id);
        let run_dir = args.out_dir.join(format!("run_{:04}", run_id));
        fs::create_dir_all(&run_dir).with_context(|| "failed to create run dir")?;

        let metadata = build_agent_metadata(&args, 0);
        let meta_path = run_dir.join("meta.json");
        write_json(&meta_path, &metadata, "meta.json")?;

        let mut tool_transcript = if let Some(bundle) = replay_bundle.as_ref() {
            tooling::ToolTranscript::new_replay(bundle.transcript.clone())
        } else {
            tooling::ToolTranscript::new_live()
        };

        let mut agent = agent::ClawdbotAgent::new(args.seed, agent_variant);
        let mut agent_trace = Vec::new();
        let mut prior_outputs = Vec::new();

        const MAX_STEPS: u32 = 8;
        for step in 0..MAX_STEPS {
            let case_context = agent::AgentCaseContext {
                run_id,
                case_id: case_id.clone(),
                notes: "deterministic demo case".to_string(),
            };
            let input = agent::AgentInput {
                case_context,
                step,
                seed: args.seed,
                run_metadata: metadata.clone(),
                transcript: tool_transcript.handle(),
                prior_tool_outputs: prior_outputs.clone(),
            };
            let output = agent.step(input);
            agent_trace.push(agent::trace_entry_from_output(step, &output));

            for request in output.tool_requests {
                let response = tool_transcript.execute(step, request);
                prior_outputs.push(response);
            }

            if output.is_final {
                break;
            }
        }

        let mismatches = tool_transcript.mismatches().to_vec();
        let transcript_record = tool_transcript.into_record();
        let agent_trace_path = run_dir.join("agent_trace.json");
        write_json(&agent_trace_path, &agent_trace, "agent_trace.json")?;

        let tool_transcript_path = run_dir.join("tool_transcript.json");
        tooling::write_transcript(&tool_transcript_path, &transcript_record)?;

        let hash_chain = drift::build_hash_chain(&agent_trace, &transcript_record.entries)?;
        let hash_chain_path = run_dir.join("hash_chain.txt");
        fs::write(&hash_chain_path, hash_chain.join("\n") + "\n")
            .with_context(|| "failed to write hash_chain.txt")?;

        let drift_report = if let Some(bundle) = replay_bundle.as_ref() {
            let mut report = drift::detect_transcript_drift(&bundle.transcript, &transcript_record);
            for mismatch in mismatches.iter() {
                report.issues.push(drift::issue_from_mismatch(mismatch));
            }
            report.drifted = report.drifted || !report.issues.is_empty();
            report
        } else {
            drift::DriftReport {
                schema_version: drift::DRIFT_SCHEMA_VERSION,
                drifted: !mismatches.is_empty(),
                issues: mismatches.iter().map(drift::issue_from_mismatch).collect(),
            }
        };

        let drift_report_path = run_dir.join("drift_report.json");
        write_json(&drift_report_path, &drift_report, "drift_report.json")?;

        let artifacts = vec![
            model::WitnessArtifact {
                name: "meta.json".to_string(),
                path: meta_path.display().to_string(),
                blake3: drift::hash_file(&meta_path)?,
            },
            model::WitnessArtifact {
                name: "agent_trace.json".to_string(),
                path: agent_trace_path.display().to_string(),
                blake3: drift::hash_file(&agent_trace_path)?,
            },
            model::WitnessArtifact {
                name: "tool_transcript.json".to_string(),
                path: tool_transcript_path.display().to_string(),
                blake3: drift::hash_file(&tool_transcript_path)?,
            },
            model::WitnessArtifact {
                name: "drift_report.json".to_string(),
                path: drift_report_path.display().to_string(),
                blake3: drift::hash_file(&drift_report_path)?,
            },
            model::WitnessArtifact {
                name: "hash_chain.txt".to_string(),
                path: hash_chain_path.display().to_string(),
                blake3: drift::hash_file(&hash_chain_path)?,
            },
        ];
        let bundle_hash = drift::bundle_hash(&artifacts)?;

        let witness_manifest = model::WitnessManifest {
            schema_version: model::WITNESS_MANIFEST_SCHEMA_VERSION,
            run_id,
            agent: agent_name.clone(),
            mode: match transcript_record.mode {
                tooling::ToolMode::Live => "live".to_string(),
                tooling::ToolMode::Replay => "replay".to_string(),
            },
            artifacts,
            schema_versions: model::BundleSchemaVersions {
                witness_manifest: model::WITNESS_MANIFEST_SCHEMA_VERSION,
                agent_trace: agent::AGENT_TRACE_SCHEMA_VERSION,
                tool_transcript: tooling::TOOL_TRANSCRIPT_SCHEMA_VERSION,
                drift_report: drift::DRIFT_SCHEMA_VERSION,
                hash_chain: 1,
            },
            bundle_hash,
            replay_source: args.replay.as_ref().map(|path| path.display().to_string()),
        };

        let witness_manifest_path = run_dir.join("witness_manifest.json");
        write_json(
            &witness_manifest_path,
            &witness_manifest,
            "witness_manifest.json",
        )?;

        let _manifest = model::ArtifactManifest {
            meta_json: meta_path.display().to_string(),
            trace_jsonl: String::new(),
            results_csv: String::new(),
            results_json: String::new(),
            summary_json: String::new(),
            witness_root_txt: String::new(),
            analysis_json: String::new(),
            agent_trace_json: Some(agent_trace_path.display().to_string()),
            tool_transcript_json: Some(tool_transcript_path.display().to_string()),
            witness_manifest_json: Some(witness_manifest_path.display().to_string()),
            hash_chain_txt: Some(hash_chain_path.display().to_string()),
            drift_report_json: Some(drift_report_path.display().to_string()),
        };

        if tui_enabled && single_run {
            #[cfg(feature = "tui")]
            tui::launch_agent(
                &agent_name,
                run_id,
                args.seed,
                &agent_trace,
                &transcript_record,
                &drift_report,
                args.replay.is_some(),
                &_manifest,
            )?;
        } else if !tui_enabled && single_run && !args.no_tui {
            println!("TUI disabled (missing feature).");
        }

        println!(
            "Agent={} Run={} OutputDir={} Drifted={}",
            agent_name,
            run_id,
            run_dir.display(),
            drift_report.drifted
        );
        println!("Artifacts:");
        println!("  agent_trace.json: {}", agent_trace_path.display());
        println!("  tool_transcript.json: {}", tool_transcript_path.display());
        println!(
            "  witness_manifest.json: {}",
            witness_manifest_path.display()
        );
        println!("  hash_chain.txt: {}", hash_chain_path.display());
        println!("  drift_report.json: {}", drift_report_path.display());
    }

    Ok(())
}

fn run_demo(args: DemoArgs) -> Result<()> {
    match args.scenario {
        DemoScenario::Drift(config) => demo::run_drift_demo(config),
    }
}

fn run_bench(args: BenchArgs) -> Result<()> {
    if args.clean && args.out_dir.exists() {
        fs::remove_dir_all(&args.out_dir).with_context(|| "failed to clean bench output dir")?;
    }
    fs::create_dir_all(&args.out_dir).with_context(|| "failed to create bench output dir")?;

    let build_info = bench::BenchBuildInfo {
        git_rev: git_rev(),
        rustc_version: command_version("rustc"),
        cargo_version: command_version("cargo"),
        nix_store_path: std::env::var("NIX_STORE").ok(),
    };

    let report = bench::run_bench(
        bench::BenchConfig {
            seed: args.seed,
            runs: args.runs,
            threads: args.threads.clone(),
            repeat: args.repeat,
            determinism_check: args.determinism_check,
        },
        build_info,
    )?;

    let json_path = args.out_dir.join("bench.json");
    write_json(&json_path, &report, "bench.json")?;

    let csv_path = args.out_dir.join("bench.csv");
    bench::write_csv(&csv_path, &report)?;

    println!("Bench artifacts:");
    println!("  bench.json: {}", json_path.display());
    println!("  bench.csv: {}", csv_path.display());
    println!(
        "Bench complete: threads={} repeat={} runs={}",
        report.entries.len(),
        report.repeat,
        report.runs
    );

    Ok(())
}

fn verify_cmd(args: VerifyArgs) -> Result<()> {
    if let Some(ref witness) = args.witness {
        if witness.is_dir() {
            drift::verify_witness_bundle(witness)?;
            println!("Verified witness bundle at {}", witness.display());
            return Ok(());
        }
    }

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

fn build_agent_metadata(args: &RunArgs, total_rng_calls: u64) -> model::RunMetadata {
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
            requested_runs: 1,
            executed_runs: 1,
            parallel: false,
            parallel_strategy: "sequential".to_string(),
            case_filter: args.case,
            entropy_sources: vec![
                "rng:StdRng(seed)".to_string(),
                "tooling:stubbed-or-replay".to_string(),
            ],
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

fn resolve_threads(args: &RunArgs) -> usize {
    if !args.parallel {
        return 1;
    }
    if let Some(threads) = args.threads {
        return threads.max(1);
    }
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1)
}

fn derive_case_id(seed: u64, run_id: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(run_id.to_le_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{:02x}", byte));
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn determinism_across_threads() -> Result<()> {
        let seed = 42;
        let run_ids: Vec<u32> = (0..128).collect();
        let mut roots = Vec::new();

        for threads in [1_usize, 2, 4] {
            let output = eval::run_with_trace(seed, &run_ids, true, threads)?;
            let metadata = model::WitnessedMetadata {
                schema_version: model::TRACE_SCHEMA_VERSION,
                seed,
                requested_runs: run_ids.len() as u32,
                executed_runs: run_ids.len() as u32,
                parallel: true,
                parallel_strategy: "rayon/ordered-run-ids".to_string(),
                case_filter: None,
                entropy_sources: vec!["rng:StdRng(seed)".to_string()],
                total_rng_calls: output.total_rng_calls,
            };
            let root = witness::compute_witness_root(&metadata, &output.trace)?;
            roots.push(root);
        }

        assert!(roots.iter().all(|root| root == &roots[0]));
        Ok(())
    }

    #[test]
    fn drift_demo_flags_regression() -> Result<()> {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let out_dir = PathBuf::from("target/test_drift_demo").join(format!("case_{}", id));

        let args = DemoDriftArgs {
            seed: 4242,
            out_dir: out_dir.clone(),
            clean: true,
        };
        demo::run_drift_demo(args)?;

        let drift_path = out_dir
            .join("regressed")
            .join("run_0000")
            .join("drift_report.json");
        let drift_file = File::open(&drift_path).with_context(|| "failed to open drift report")?;
        let drift_report: drift::DriftReport =
            serde_json::from_reader(drift_file).with_context(|| "failed to parse drift report")?;

        assert!(drift_report.drifted);
        assert!(!drift_report.issues.is_empty());
        Ok(())
    }
}
