use crate::agent::Agent;
use anyhow::{Context, Result};
use clap::builder::ArgPredicate;
use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use sha2::{Digest, Sha256};

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn rel_artifact_path(out_dir: &Path, path: &Path) -> String {
    path.strip_prefix(out_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

mod agent;
mod canonical_json;
mod chaos;
mod drift;
mod eval;
mod hex;
mod io_utils;
mod llm;
mod model;
mod nix_provenance;
mod ordeal;
mod report;
mod strict_json;
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
    Ordeal(OrdealArgs),
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum FaultToggle {
    On,
    Off,
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum FaultProfile {
    None,
    Ci,
    Stress,
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum LlmToggle {
    On,
    Off,
}

#[derive(Subcommand, Debug)]
pub enum DemoCommand {
    Drift(DemoDriftArgs),
}

#[derive(Subcommand, Debug)]
pub enum OrdealCommand {
    Check(OrdealCheckArgs),
}

#[derive(Args, Debug)]
pub struct OrdealArgs {
    #[command(subcommand)]
    pub command: OrdealCommand,
}

#[derive(Args, Debug)]
pub struct OrdealCheckArgs {
    #[arg(long, default_value = "goldens/ordeal_witness_root.txt")]
    pub golden: PathBuf,

    #[arg(long)]
    pub update_golden: bool,

    #[arg(long, default_value_t = 42)]
    pub seed: u64,
}

#[derive(Args, Debug)]
pub struct DemoArgs {
    #[command(subcommand)]
    pub command: DemoCommand,
}

#[derive(Args, Debug)]
pub struct DemoDriftArgs {
    #[arg(long, default_value_t = 42)]
    pub seed: u64,

    #[arg(long, default_value = "demo_out")]
    pub out_dir: PathBuf,

    #[arg(long, default_value = "stress", value_enum)]
    pub fault_profile: FaultProfile,

    #[arg(long, default_value_t = 1)]
    pub threads: usize,

    #[arg(long)]
    pub clean: bool,
}

/// Run a deterministic evaluation and emit artifacts.
#[derive(Args, Debug)]
#[command(
    about = "Run a deterministic evaluation and emit artifacts.",
    long_about = "Run a deterministic evaluation and emit artifacts.\n\nWitness roots exclude runtime environment details and simulated latency. Thread counts are recorded in provenance only."
)]
#[command(
    group(ArgGroup::new("agent_mode").args(["agent", "replay"]).multiple(false))
)]
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

    #[arg(
        long,
        default_value_t = true,
        action = clap::ArgAction::Set,
        value_parser = clap::value_parser!(bool)
    )]
    pub parallel: bool,

    #[arg(long)]
    pub created_at: Option<String>,

    #[arg(
        long,
        group = "agent_mode",
        value_parser = ["clawdbot", "ordeal", "gauntlet"],
        help = "Agent name (clawdbot or ordeal; gauntlet is a deprecated alias)"
    )]
    pub agent: Option<String>,

    #[arg(long, group = "agent_mode")]
    pub replay: Option<PathBuf>,

    #[arg(
        long,
        requires = "agent_mode",
        default_value_if("agent_mode", ArgPredicate::IsPresent, "1"),
        help = "Agent/replay only; stored in provenance only"
    )]
    pub threads: Option<usize>,

    #[arg(
        long,
        requires = "agent_mode",
        default_value_if("agent_mode", ArgPredicate::IsPresent, "off"),
        value_enum,
        help = "Agent/replay only"
    )]
    pub faults: Option<FaultToggle>,

    #[arg(
        long,
        requires = "agent_mode",
        default_value_if("agent_mode", ArgPredicate::IsPresent, "none"),
        value_enum,
        help = "Agent/replay only"
    )]
    pub fault_profile: Option<FaultProfile>,

    #[arg(long, requires = "agent_mode", help = "Agent/replay only")]
    pub fault_timeout_rate: Option<f64>,

    #[arg(long, requires = "agent_mode", help = "Agent/replay only")]
    pub fault_corrupt_rate: Option<f64>,

    #[arg(long, requires = "agent_mode", help = "Agent/replay only")]
    pub fault_drop_rate: Option<f64>,

    #[arg(long, requires = "agent_mode", help = "Agent/replay only")]
    pub fault_latency_rate: Option<f64>,

    #[arg(
        long,
        requires = "agent_mode",
        default_value_if("agent_mode", ArgPredicate::IsPresent, "off"),
        value_enum,
        help = "Agent/replay only"
    )]
    pub llm: Option<LlmToggle>,

    #[arg(
        long,
        requires = "agent_mode",
        default_value_if("agent_mode", ArgPredicate::IsPresent, "stub"),
        help = "Agent/replay only"
    )]
    pub llm_model: Option<String>,

    #[arg(long, requires = "agent_mode", help = "Agent/replay only")]
    pub llm_seed: Option<u64>,

    #[arg(
        long,
        requires = "agent_mode",
        default_value_if("agent_mode", ArgPredicate::IsPresent, "0.5"),
        help = "Agent/replay only; stored as canonical string in witness metadata for ordeal runs"
    )]
    pub pass_threshold: Option<String>,

    #[arg(
        long,
        default_value = "auto",
        value_enum,
        help = "Capture Nix provenance (auto|on|off)"
    )]
    pub nix_provenance: nix_provenance::NixProvenanceMode,
}

impl RunArgs {
    fn agent_threads(&self) -> usize {
        self.threads.unwrap_or(1)
    }

    fn faults_toggle(&self) -> FaultToggle {
        self.faults.clone().unwrap_or(FaultToggle::Off)
    }

    fn fault_profile_value(&self) -> FaultProfile {
        self.fault_profile.clone().unwrap_or(FaultProfile::None)
    }

    fn llm_toggle(&self) -> LlmToggle {
        self.llm.clone().unwrap_or(LlmToggle::Off)
    }

    fn llm_model_value(&self) -> &str {
        self.llm_model.as_deref().unwrap_or("stub")
    }
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

    #[arg(long)]
    pub recompute_witness_root: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        CommandLine::Run(args) => run(args),
        CommandLine::Verify(args) => verify_cmd(args),
        CommandLine::Demo(args) => demo_cmd(args),
        CommandLine::Ordeal(args) => ordeal_cmd(args),
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

    let output = eval::run_with_trace(args.seed, &run_ids, args.parallel);
    let (summary, pass_count, fail_count) = eval::summarize_with_counts(&output.results);

    if args.clean && args.out_dir.exists() {
        fs::remove_dir_all(&args.out_dir).with_context(|| "failed to clean output dir")?;
    }

    fs::create_dir_all(&args.out_dir).with_context(|| "failed to create output dir")?;

    let nix_provenance =
        nix_provenance::collect_nix_provenance(args.nix_provenance.clone(), Path::new("."))?;
    let metadata = build_metadata(
        &args,
        output.total_rng_calls,
        run_ids.len() as u32,
        nix_provenance.clone(),
    );
    let meta_path = args.out_dir.join("meta.json");
    canonical_json::write_json(&meta_path, &metadata, "meta.json")?;

    let nix_provenance_path = if let Some(ref provenance) = nix_provenance {
        let path = args.out_dir.join("nix_provenance.json");
        nix_provenance::write_nix_provenance(&path, provenance)?;
        Some(path)
    } else {
        None
    };

    let trace_path = args.out_dir.join("trace.jsonl");
    write_trace(&trace_path, &output.trace)?;

    let witness_root = compute_witness_root(&metadata.witnessed, &output.trace)?;
    let witness_path = args.out_dir.join("witness_root.txt");
    io_utils::write_atomic_string(
        &witness_path,
        "witness_root.txt",
        &format!("{}\n", witness_root),
    )?;

    let csv_path = args.out_dir.join("results.csv");
    eval::write_results(&csv_path, &output.results)?;

    let results_json_path = args.out_dir.join("results.json");
    canonical_json::write_json(&results_json_path, &output.results, "results.json")?;

    let summary_json_path = args.out_dir.join("summary.json");
    canonical_json::write_json(&summary_json_path, &summary, "summary.json")?;

    let manifest = model::ArtifactManifest {
        meta_json: rel_artifact_path(&args.out_dir, &meta_path),
        trace_jsonl: Some(rel_artifact_path(&args.out_dir, &trace_path)),
        results_csv: Some(rel_artifact_path(&args.out_dir, &csv_path)),
        results_json: Some(rel_artifact_path(&args.out_dir, &results_json_path)),
        summary_json: Some(rel_artifact_path(&args.out_dir, &summary_json_path)),
        witness_root_txt: Some(rel_artifact_path(&args.out_dir, &witness_path)),
        analysis_json: Some(rel_artifact_path(
            &args.out_dir,
            &args.out_dir.join("analysis.json"),
        )),
        nix_provenance_json: nix_provenance_path
            .as_ref()
            .map(|path| rel_artifact_path(&args.out_dir, path)),
        agent_trace_json: None,
        tool_transcript_json: None,
        witness_manifest_json: None,
        hash_chain_txt: None,
        drift_report_json: None,
        chaos_profile_json: None,
    };

    let analysis_bundle = model::AnalysisBundle {
        metadata: metadata.clone(),
        summary: summary.clone(),
        results: output.results.clone(),
        witness_root: witness_root.clone(),
        artifacts: manifest.clone(),
    };

    let analysis_path = args.out_dir.join("analysis.json");
    canonical_json::write_json(&analysis_path, &analysis_bundle, "analysis.json")?;

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
        if let Some(path) = nix_provenance_path.as_ref() {
            println!("  nix_provenance.json: {}", path.display());
        }
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

fn demo_cmd(args: DemoArgs) -> Result<()> {
    match args.command {
        DemoCommand::Drift(args) => demo_drift(args),
    }
}

fn demo_drift(args: DemoDriftArgs) -> Result<()> {
    if args.clean && args.out_dir.exists() {
        fs::remove_dir_all(&args.out_dir).with_context(|| "failed to clean demo output dir")?;
    }
    fs::create_dir_all(&args.out_dir).with_context(|| "failed to create demo output dir")?;

    let base_dir = args.out_dir.join("drift");
    fs::create_dir_all(&base_dir).with_context(|| "failed to create drift demo dir")?;

    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build()?;

    thread_pool.install(|| {
        let mut baseline_no_faults = None;
        let mut baseline_faults = None;

        for (label, faults_enabled, regress) in [
            ("baseline", false, false),
            ("regressed", false, true),
            ("baseline_faults", true, false),
            ("regressed_faults", true, true),
        ] {
            let scenario_dir = base_dir.join(label);
            fs::create_dir_all(&scenario_dir)
                .with_context(|| "failed to create drift scenario dir")?;

            let chaos_profile =
                demo_chaos_profile(args.seed, args.fault_profile.clone(), faults_enabled);
            let demo_run = run_demo_agent(args.seed, 0, chaos_profile.clone(), regress)?;

            let expected = match (faults_enabled, regress) {
                (false, true) => baseline_no_faults.as_ref(),
                (true, true) => baseline_faults.as_ref(),
                _ => None,
            };

            let drift_report = if let Some(expected_record) = expected {
                drift::detect_transcript_drift(expected_record, &demo_run.transcript)
            } else {
                drift::DriftReport {
                    schema_version: drift::DRIFT_SCHEMA_VERSION,
                    drifted: false,
                    issues: Vec::new(),
                }
            };

            let meta_path = scenario_dir.join("meta.json");
            canonical_json::write_json(&meta_path, &demo_run.metadata, "meta.json")?;

            let chaos_profile_path = scenario_dir.join("chaos_profile.json");
            canonical_json::write_json(
                &chaos_profile_path,
                &demo_run.chaos_profile,
                "chaos_profile.json",
            )?;

            let agent_trace_path = scenario_dir.join("agent_trace.json");
            canonical_json::write_json(
                &agent_trace_path,
                &demo_run.agent_trace,
                "agent_trace.json",
            )?;

            let tool_transcript_path = scenario_dir.join("tool_transcript.json");
            tooling::write_transcript(&tool_transcript_path, &demo_run.transcript)?;

            let hash_chain =
                drift::build_hash_chain(&demo_run.agent_trace, &demo_run.transcript.entries)?;
            let hash_chain_path = scenario_dir.join("hash_chain.txt");
            io_utils::write_atomic_string(
                &hash_chain_path,
                "hash_chain.txt",
                &(hash_chain.join("\n") + "\n"),
            )?;

            let witness_root = compute_agent_witness_root(
                &demo_run.metadata.witnessed,
                &demo_run.agent_trace,
                &demo_run.transcript.entries,
            )?;
            let witness_root_path = scenario_dir.join("witness_root.txt");
            io_utils::write_atomic_string(
                &witness_root_path,
                "witness_root.txt",
                &format!("{}\n", witness_root),
            )?;

            let drift_report_path = scenario_dir.join("drift_report.json");
            canonical_json::write_json(&drift_report_path, &drift_report, "drift_report.json")?;

            let artifact_hashes = drift::artifact_hashes(&[
                &meta_path,
                &agent_trace_path,
                &tool_transcript_path,
                &drift_report_path,
                &hash_chain_path,
                &chaos_profile_path,
                &witness_root_path,
            ])?;
            let bundle_hash = drift::bundle_hash(&artifact_hashes)?;

            let witness_manifest = model::WitnessManifest {
                schema_version: model::WITNESS_MANIFEST_SCHEMA_VERSION,
                run_id: 0,
                agent: "clawdbot".to_string(),
                mode: if regress {
                    "replay".to_string()
                } else {
                    "live".to_string()
                },
                meta_json: meta_path.display().to_string(),
                agent_trace_json: agent_trace_path.display().to_string(),
                tool_transcript_json: tool_transcript_path.display().to_string(),
                drift_report_json: drift_report_path.display().to_string(),
                hash_chain_txt: hash_chain_path.display().to_string(),
                chaos_profile_json: Some(chaos_profile_path.display().to_string()),
                witness_root_txt: Some(witness_root_path.display().to_string()),
                nix_provenance_json: None,
                artifact_hashes,
                bundle_hash,
                replay_source: expected.map(|_| {
                    base_dir
                        .join(if faults_enabled {
                            "baseline_faults"
                        } else {
                            "baseline"
                        })
                        .display()
                        .to_string()
                }),
            };

            let witness_manifest_path = scenario_dir.join("witness_manifest.json");
            canonical_json::write_json(
                &witness_manifest_path,
                &witness_manifest,
                "witness_manifest.json",
            )?;

            if !regress {
                if faults_enabled {
                    baseline_faults = Some(demo_run.transcript.clone());
                } else {
                    baseline_no_faults = Some(demo_run.transcript.clone());
                }
            }

            println!(
                "Demo scenario {} (faults_enabled={} regress={}): drifted={}",
                label, faults_enabled, regress, drift_report.drifted
            );
        }

        Ok::<(), anyhow::Error>(())
    })?;

    println!("Drift demo complete. Outputs at {}", base_dir.display());

    Ok(())
}

#[derive(Clone, Copy)]
enum AgentSource {
    Cli,
    ReplayManifest,
    Default,
}

struct AgentSelection {
    name: String,
    legacy_gauntlet: bool,
    source: AgentSource,
}

fn normalize_agent_name(raw: &str) -> Result<(String, bool)> {
    match raw {
        "clawdbot" => Ok((raw.to_string(), false)),
        "ordeal" => Ok((raw.to_string(), false)),
        "gauntlet" => Ok(("ordeal".to_string(), true)),
        _ => anyhow::bail!("unsupported agent: {}", raw),
    }
}

fn select_agent_name(
    args: &RunArgs,
    replay_bundle: &Option<(
        model::WitnessManifest,
        tooling::ToolTranscriptRecord,
        Vec<agent::AgentTraceEntry>,
    )>,
) -> Result<AgentSelection> {
    let (raw, source) = if let Some(name) = args.agent.as_ref() {
        (name.clone(), AgentSource::Cli)
    } else if let Some((manifest, _, _)) = replay_bundle.as_ref() {
        (manifest.agent.clone(), AgentSource::ReplayManifest)
    } else {
        ("clawdbot".to_string(), AgentSource::Default)
    };

    let (name, legacy_gauntlet) = normalize_agent_name(&raw)?;
    Ok(AgentSelection {
        name,
        legacy_gauntlet,
        source,
    })
}

fn warn_deprecated_gauntlet(legacy_gauntlet: bool) {
    if legacy_gauntlet {
        eprintln!("Warning: agent 'gauntlet' is deprecated; use 'ordeal'.");
    }
}

fn parse_bool_env(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

fn resolve_ordeal_regress() -> bool {
    if let Ok(value) = std::env::var("COGITATOR_ORDEAL_REGRESS") {
        return parse_bool_env(&value);
    }
    std::env::var("COGITATOR_GAUNTLET_REGRESS")
        .map(|value| parse_bool_env(&value))
        .unwrap_or(false)
}

fn run_agent(args: RunArgs) -> Result<()> {
    let agent_threads = args.agent_threads();
    if agent_threads == 0 {
        anyhow::bail!("--threads must be at least 1");
    }

    let run_ids: Vec<u32> = match args.case {
        Some(case_id) => vec![case_id],
        None => (0..args.runs).collect(),
    };

    if args.replay.is_some() && run_ids.len() != 1 {
        anyhow::bail!("--replay requires a single --case or --runs 1");
    }

    let nix_provenance =
        nix_provenance::collect_nix_provenance(args.nix_provenance.clone(), Path::new("."))?;

    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(agent_threads)
        .build()?;

    thread_pool.install(|| -> Result<()> {
        if args.clean && args.out_dir.exists() {
            fs::remove_dir_all(&args.out_dir).with_context(|| "failed to clean output dir")?;
        }
        fs::create_dir_all(&args.out_dir).with_context(|| "failed to create output dir")?;

        let replay_bundle = if let Some(replay_dir) = args.replay.as_ref() {
            let manifest_path = replay_dir.join("witness_manifest.json");
            let manifest_file = File::open(&manifest_path)
                .with_context(|| "failed to open witness_manifest.json")?;
            let manifest: model::WitnessManifest = serde_json::from_reader(manifest_file)
                .with_context(|| "failed to parse witness_manifest.json")?;
            let transcript = tooling::read_transcript(Path::new(&manifest.tool_transcript_json))?;
            let agent_trace_file = File::open(&manifest.agent_trace_json)
                .with_context(|| "failed to open agent_trace.json")?;
            let agent_trace: Vec<agent::AgentTraceEntry> =
                serde_json::from_reader(agent_trace_file)
                    .with_context(|| "failed to parse agent_trace.json")?;
            Some((manifest, transcript, agent_trace))
        } else {
            None
        };

        let replay_chaos_profile = replay_bundle
            .as_ref()
            .and_then(|(manifest, _, _)| manifest.chaos_profile_json.as_ref())
            .and_then(|path| File::open(path).ok())
            .and_then(|file| serde_json::from_reader(file).ok());

        let agent_selection = select_agent_name(&args, &replay_bundle)?;
        warn_deprecated_gauntlet(agent_selection.legacy_gauntlet);
        let agent_name = agent_selection.name.clone();
        let use_legacy_tasks = agent_selection.legacy_gauntlet
            && matches!(agent_selection.source, AgentSource::ReplayManifest);

        let tui_enabled = !args.no_tui && cfg!(feature = "tui");

        let single_run = run_ids.len() == 1;

        for &run_id in run_ids.iter() {
            let case_id = derive_case_id(args.seed, run_id);
            let run_dir = args.out_dir.join(format!("run_{:04}", run_id));
            fs::create_dir_all(&run_dir).with_context(|| "failed to create run dir")?;

            let chaos_profile = replay_chaos_profile
                .clone()
                .unwrap_or_else(|| resolve_chaos_profile(&args, args.seed));
            let pass_threshold_value = args
                .pass_threshold
                .clone()
                .unwrap_or_else(|| "0.5".to_string());
            let pass_threshold_f32 =
                parse_pass_threshold(&pass_threshold_value).context("invalid pass_threshold")?;
            let pass_threshold_witnessed = if agent_name == "ordeal" {
                Some(canonical_threshold_string(pass_threshold_f32))
            } else {
                None
            };
            let metadata = build_agent_metadata(
                &args,
                0,
                chaos_profile.clone(),
                nix_provenance.clone(),
                pass_threshold_witnessed.clone(),
            );
            let meta_path = run_dir.join("meta.json");
            canonical_json::write_json(&meta_path, &metadata, "meta.json")?;

            let nix_provenance_path = if let Some(ref provenance) = nix_provenance {
                let path = run_dir.join("nix_provenance.json");
                nix_provenance::write_nix_provenance(&path, provenance)?;
                Some(path)
            } else {
                None
            };

            let chaos_profile_path = run_dir.join("chaos_profile.json");
            canonical_json::write_json(&chaos_profile_path, &chaos_profile, "chaos_profile.json")?;

            let chaos_engine = if replay_bundle.is_some() {
                None
            } else if chaos_profile.enabled {
                Some(chaos::ChaosEngine::new(chaos_profile.clone(), run_id))
            } else {
                None
            };

            let mut tool_transcript = if let Some((_, transcript, _)) = replay_bundle.as_ref() {
                tooling::ToolTranscript::new_replay(transcript.clone())
            } else {
                tooling::ToolTranscript::new_live(chaos_engine)
            };

            let mut agent_trace = Vec::new();
            let mut ordeal_issues: Vec<report::DriftIssue> = Vec::new();

            if agent_name == "ordeal" {
                let tasks_path = if use_legacy_tasks {
                    ordeal::LEGACY_GAUNTLET_TASKS_PATH
                } else {
                    ordeal::ORDEAL_TASKS_PATH
                };
                let suite = ordeal::TaskSuite::load(Path::new(tasks_path))?;
                let regress = resolve_ordeal_regress();
                let config = ordeal::OrdealConfig {
                    seed: args.seed,
                    run_id,
                    case_id: case_id.clone(),
                    pass_threshold_f32,
                    pass_threshold_witnessed: pass_threshold_witnessed
                        .clone()
                        .unwrap_or_else(|| canonical_threshold_string(0.5)),
                    regress,
                };
                let ordeal_output = ordeal::run_ordeal(&suite, &config, &mut tool_transcript)?;
                let _ = ordeal_output.total_rng_calls;
                agent_trace = ordeal_output.agent_trace;
                ordeal_issues = ordeal_output.issues;
            } else {
                let llm_config = agent::LlmConfig {
                    enabled: matches!(args.llm_toggle(), LlmToggle::On),
                    model: args.llm_model_value().to_string(),
                    seed: args.llm_seed,
                };
                let mut agent = agent::ClawdbotAgent::new(llm_config);
                let mut prior_outputs = Vec::new();

                const MAX_STEPS: u32 = 8;
                for step in 0..MAX_STEPS {
                    let input = agent::AgentInput {
                        run_id,
                        case_id: case_id.clone(),
                        step,
                        seed: args.seed,
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
            }

            let mut mismatches = tool_transcript.mismatches().to_vec();
            mismatches.extend(ordeal_issues);
            let transcript_record = tool_transcript.into_record();
            let agent_trace_path = run_dir.join("agent_trace.json");
            canonical_json::write_json(&agent_trace_path, &agent_trace, "agent_trace.json")?;

            let tool_transcript_path = run_dir.join("tool_transcript.json");
            tooling::write_transcript(&tool_transcript_path, &transcript_record)?;

            let hash_chain = drift::build_hash_chain(&agent_trace, &transcript_record.entries)?;
            let hash_chain_path = run_dir.join("hash_chain.txt");
            io_utils::write_atomic_string(
                &hash_chain_path,
                "hash_chain.txt",
                &(hash_chain.join("\n") + "\n"),
            )?;

            let agent_witness_root = compute_agent_witness_root(
                &metadata.witnessed,
                &agent_trace,
                &transcript_record.entries,
            )?;
            let agent_witness_path = run_dir.join("witness_root.txt");
            io_utils::write_atomic_string(
                &agent_witness_path,
                "witness_root.txt",
                &format!("{}\n", agent_witness_root),
            )?;

            let drift_report = if let Some((_, expected_transcript, _)) = replay_bundle.as_ref() {
                let mut report =
                    drift::detect_transcript_drift(expected_transcript, &transcript_record);
                report.issues.extend(mismatches);
                report.drifted = report.drifted || !report.issues.is_empty();
                report
            } else {
                drift::DriftReport {
                    schema_version: drift::DRIFT_SCHEMA_VERSION,
                    drifted: !mismatches.is_empty(),
                    issues: mismatches,
                }
            };

            let drift_report_path = run_dir.join("drift_report.json");
            canonical_json::write_json(&drift_report_path, &drift_report, "drift_report.json")?;

            let mut artifact_paths: Vec<&Path> = vec![
                &meta_path,
                &agent_trace_path,
                &tool_transcript_path,
                &drift_report_path,
                &hash_chain_path,
                &chaos_profile_path,
                &agent_witness_path,
            ];
            if let Some(ref path) = nix_provenance_path {
                artifact_paths.push(path.as_path());
            }
            let artifact_hashes = drift::artifact_hashes(&artifact_paths)?;
            let bundle_hash = drift::bundle_hash(&artifact_hashes)?;

            let witness_manifest = model::WitnessManifest {
                schema_version: model::WITNESS_MANIFEST_SCHEMA_VERSION,
                run_id,
                agent: agent_name.clone(),
                mode: match transcript_record.mode {
                    tooling::ToolMode::Live => "live".to_string(),
                    tooling::ToolMode::Replay => "replay".to_string(),
                },
                meta_json: meta_path.display().to_string(),
                agent_trace_json: agent_trace_path.display().to_string(),
                tool_transcript_json: tool_transcript_path.display().to_string(),
                drift_report_json: drift_report_path.display().to_string(),
                hash_chain_txt: hash_chain_path.display().to_string(),
                chaos_profile_json: Some(chaos_profile_path.display().to_string()),
                witness_root_txt: Some(agent_witness_path.display().to_string()),
                nix_provenance_json: nix_provenance_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                artifact_hashes,
                bundle_hash,
                replay_source: args.replay.as_ref().map(|path| path.display().to_string()),
            };

            let witness_manifest_path = run_dir.join("witness_manifest.json");
            canonical_json::write_json(
                &witness_manifest_path,
                &witness_manifest,
                "witness_manifest.json",
            )?;

            let _manifest = model::ArtifactManifest {
                meta_json: meta_path.display().to_string(),
                trace_jsonl: None,
                results_csv: None,
                results_json: None,
                summary_json: None,
                witness_root_txt: None,
                analysis_json: None,
                nix_provenance_json: nix_provenance_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                agent_trace_json: Some(agent_trace_path.display().to_string()),
                tool_transcript_json: Some(tool_transcript_path.display().to_string()),
                witness_manifest_json: Some(witness_manifest_path.display().to_string()),
                hash_chain_txt: Some(hash_chain_path.display().to_string()),
                drift_report_json: Some(drift_report_path.display().to_string()),
                chaos_profile_json: Some(chaos_profile_path.display().to_string()),
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
            println!("  chaos_profile.json: {}", chaos_profile_path.display());
            println!(
                "  witness_manifest.json: {}",
                witness_manifest_path.display()
            );
            println!("  hash_chain.txt: {}", hash_chain_path.display());
            println!("  drift_report.json: {}", drift_report_path.display());
            println!("  witness_root.txt: {}", agent_witness_path.display());
            if let Some(path) = nix_provenance_path.as_ref() {
                println!("  nix_provenance.json: {}", path.display());
            }
        }

        Ok(())
    })
}

fn verify_cmd(args: VerifyArgs) -> Result<()> {
    if args.recompute_witness_root {
        let witness_dir = args
            .witness
            .as_ref()
            .filter(|path| path.is_dir())
            .ok_or_else(|| anyhow::anyhow!("--recompute-witness-root requires --witness <dir>"))?;
        let receipt =
            verify::recompute_agent_witness_root_from_bundle(witness_dir, args.expect.as_deref())?;
        println!(
            "Recomputed witness_root: matched={} expected={} computed={}",
            receipt.matched, receipt.expected, receipt.computed
        );
        if !receipt.matched {
            if let Some(component) = receipt.differing_component.as_deref() {
                eprintln!("Committed component hint: {}", component);
            }
            anyhow::bail!("recomputed witness_root mismatch");
        }
        return Ok(());
    }

    if let Some(ref witness) = args.witness {
        if witness.is_dir() {
            let report = drift::verify_witness_bundle(witness)?;
            println!(
                "Verification: verified={} issues={} bundle_hash_expected={} bundle_hash_actual={}",
                report.verified,
                report.issues.len(),
                report.bundle_hash_expected,
                report.bundle_hash_actual
            );
            println!(
                "verify_report.json: {}",
                witness.join("verify_report.json").display()
            );
            if !report.verified {
                anyhow::bail!("witness bundle verification failed");
            }
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

fn ordeal_cmd(args: OrdealArgs) -> Result<()> {
    match args.command {
        OrdealCommand::Check(check) => ordeal_check_cmd(check),
    }
}

fn ordeal_check_cmd(args: OrdealCheckArgs) -> Result<()> {
    let out_dir = PathBuf::from("out_ci");
    let run_args = RunArgs {
        seed: args.seed,
        runs: 1,
        case: Some(0),
        out_dir: out_dir.clone(),
        clean: true,
        no_tui: true,
        parallel: false,
        created_at: None,
        agent: Some("ordeal".to_string()),
        replay: None,
        threads: Some(1),
        faults: Some(FaultToggle::Off),
        fault_profile: Some(FaultProfile::None),
        fault_timeout_rate: None,
        fault_corrupt_rate: None,
        fault_drop_rate: None,
        fault_latency_rate: None,
        llm: Some(LlmToggle::Off),
        llm_model: Some("stub".to_string()),
        llm_seed: None,
        pass_threshold: Some("0.5".to_string()),
        nix_provenance: nix_provenance::NixProvenanceMode::Off,
    };

    run(run_args)?;

    let actual = read_trimmed(&out_dir.join("run_0000").join("witness_root.txt"))?;
    if args.update_golden {
        io_utils::write_atomic_string(
            &args.golden,
            "ordeal golden witness root",
            &(actual.clone()
                + "
"),
        )?;
        println!("Updated golden {} with {}", args.golden.display(), actual);
        return Ok(());
    }

    let expected = read_trimmed(&args.golden)?;
    if expected != actual {
        anyhow::bail!(
            "ordeal witness root drift detected: expected {} actual {} (use --update-golden for intentional changes)",
            expected,
            actual
        );
    }

    println!("Ordeal witness root matches golden {}", actual);
    Ok(())
}

fn write_trace(path: &Path, events: &[model::TraceEvent]) -> Result<()> {
    let ordered = ordered_trace_events(events);
    io_utils::write_atomic(path, "trace.jsonl", |file| {
        let mut writer = BufWriter::new(file);
        for event in ordered {
            let bytes = trace::encode_event(event)?;
            writer.write_all(&bytes)?;
            writer.write_all(b"\n")?;
        }
        writer.flush()?;
        Ok(())
    })
}

fn compute_witness_root(
    metadata: &model::WitnessedMetadata,
    events: &[model::TraceEvent],
) -> Result<String> {
    let metadata_bytes = trace::encode_witnessed_metadata(metadata)?;
    let mut witness = witness::Witness::new(&metadata_bytes)?;

    let ordered = ordered_trace_events(events);
    for event in ordered {
        let event_bytes = trace::encode_event(event)?;
        witness.update(&event_bytes)?;
    }

    Ok(witness.finalize_hex())
}

fn ordered_trace_events(events: &[model::TraceEvent]) -> Vec<&model::TraceEvent> {
    let mut ordered: Vec<&model::TraceEvent> = events.iter().collect();
    // Stable ordering by run_id + step ensures deterministic hashing across parallel runs.
    ordered.sort_by_key(|event| (event.run_id, event.step));
    ordered
}

fn compute_agent_witness_root(
    metadata: &model::WitnessedMetadata,
    agent_trace: &[agent::AgentTraceEntry],
    tool_calls: &[tooling::ToolCall],
) -> Result<String> {
    trace::compute_agent_witness_root(metadata, agent_trace, tool_calls)
}

fn build_metadata(
    args: &RunArgs,
    total_rng_calls: u64,
    executed_runs: u32,
    nix_provenance: Option<model::NixProvenance>,
) -> model::RunMetadata {
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
        args.agent_threads(),
        nix_provenance.as_ref(),
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
            chaos_profile: None,
            pass_threshold: None,
        },
        provenance: model::ProvenanceMetadata {
            created_at,
            git_rev,
            rustc_version,
            cargo_version,
            nix_store_path,
            agent_threads: None,
            nix_provenance,
            variability_factors,
        },
    }
}

fn build_agent_metadata(
    args: &RunArgs,
    total_rng_calls: u64,
    chaos_profile: chaos::ChaosProfile,
    nix_provenance: Option<model::NixProvenance>,
    pass_threshold: Option<String>,
) -> model::RunMetadata {
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
        args.agent_threads(),
        nix_provenance.as_ref(),
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
            entropy_sources: {
                let mut sources = vec![
                    "rng:StdRng(seed)".to_string(),
                    "tooling:stubbed-or-replay".to_string(),
                ];
                if chaos_profile.enabled {
                    sources.push("chaos:fault-schedule".to_string());
                }
                sources
            },
            total_rng_calls,
            chaos_profile: Some(model::ChaosProfileSummary {
                enabled: chaos_profile.enabled,
                profile: chaos_profile.profile.clone(),
                schedule_version: chaos_profile.schedule_version,
                rates: chaos_profile.rates.clone(),
            }),
            pass_threshold,
        },
        provenance: model::ProvenanceMetadata {
            created_at,
            git_rev,
            rustc_version,
            cargo_version,
            nix_store_path,
            agent_threads: Some(args.agent_threads()),
            nix_provenance,
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

fn parse_pass_threshold(value: &str) -> Result<f32> {
    let parsed: f32 = value.parse().with_context(|| "parse pass threshold")?;
    Ok(parsed)
}

fn canonical_threshold_string(value: f32) -> String {
    format!("f32:0x{:08X}", value.to_bits())
}

fn resolve_chaos_profile(args: &RunArgs, seed: u64) -> chaos::ChaosProfile {
    let enabled = matches!(args.faults_toggle(), FaultToggle::On);
    let profile_name = match args.fault_profile_value() {
        FaultProfile::None => "none",
        FaultProfile::Ci => "ci",
        FaultProfile::Stress => "stress",
    };

    let profile = chaos::profile_from_name(profile_name, seed, enabled);
    let timeout_rate = args.fault_timeout_rate.map(chaos::rate_to_per_million);
    let corrupt_rate = args.fault_corrupt_rate.map(chaos::rate_to_per_million);
    let drop_rate = args.fault_drop_rate.map(chaos::rate_to_per_million);
    let latency_rate = args.fault_latency_rate.map(chaos::rate_to_per_million);

    chaos::with_overrides(profile, timeout_rate, corrupt_rate, drop_rate, latency_rate)
}

fn demo_chaos_profile(seed: u64, profile: FaultProfile, enabled: bool) -> chaos::ChaosProfile {
    let profile_name = match profile {
        FaultProfile::None => "none",
        FaultProfile::Ci => "ci",
        FaultProfile::Stress => "stress",
    };
    chaos::profile_from_name(profile_name, seed, enabled)
}

struct DemoRun {
    metadata: model::RunMetadata,
    chaos_profile: chaos::ChaosProfile,
    agent_trace: Vec<agent::AgentTraceEntry>,
    transcript: tooling::ToolTranscriptRecord,
}

fn run_demo_agent(
    seed: u64,
    run_id: u32,
    chaos_profile: chaos::ChaosProfile,
    regress: bool,
) -> Result<DemoRun> {
    let case_id = derive_case_id(seed, run_id);
    let metadata = build_agent_metadata(
        &RunArgs {
            seed,
            runs: 1,
            case: Some(run_id),
            out_dir: PathBuf::new(),
            clean: false,
            no_tui: true,
            parallel: false,
            created_at: None,
            agent: Some("clawdbot".to_string()),
            replay: None,
            threads: Some(1),
            faults: Some(FaultToggle::Off),
            fault_profile: Some(FaultProfile::None),
            fault_timeout_rate: None,
            fault_corrupt_rate: None,
            fault_drop_rate: None,
            fault_latency_rate: None,
            llm: Some(LlmToggle::Off),
            llm_model: Some("stub".to_string()),
            llm_seed: None,
            pass_threshold: Some("0.5".to_string()),
            nix_provenance: nix_provenance::NixProvenanceMode::Off,
        },
        0,
        chaos_profile.clone(),
        None,
        None,
    );

    let chaos_engine = if chaos_profile.enabled {
        Some(chaos::ChaosEngine::new(chaos_profile.clone(), run_id))
    } else {
        None
    };
    let mut tool_transcript = tooling::ToolTranscript::new_live(chaos_engine);
    let mut agent = agent::ClawdbotAgent::new(agent::LlmConfig::default());
    let mut agent_trace = Vec::new();
    let mut prior_outputs = Vec::new();

    const MAX_STEPS: u32 = 4;
    for step in 0..MAX_STEPS {
        let input = agent::AgentInput {
            run_id,
            case_id: case_id.clone(),
            step,
            seed,
            prior_tool_outputs: prior_outputs.clone(),
        };
        let output = agent.step(input);
        let output = if regress {
            apply_demo_regression_to_output(step, output)
        } else {
            output
        };
        agent_trace.push(agent::trace_entry_from_output(step, &output));

        for request in output.tool_requests {
            let response = tool_transcript.execute(step, request);
            prior_outputs.push(response);
        }

        if output.is_final {
            break;
        }
    }

    Ok(DemoRun {
        metadata,
        chaos_profile,
        agent_trace,
        transcript: tool_transcript.into_record(),
    })
}

fn apply_demo_regression_to_output(
    step: u32,
    mut output: agent::AgentOutput,
) -> agent::AgentOutput {
    if step == 0 {
        if let Some(request) = output.tool_requests.get_mut(0) {
            request.tool_name = "clawdbot.lookup.v2".to_string();
            if let serde_json::Value::Object(map) = &mut request.arguments {
                map.insert(
                    "contract".to_string(),
                    serde_json::Value::String("v2".to_string()),
                );
            }
        }
    }
    output
}

fn parallel_strategy(parallel: bool) -> String {
    if parallel {
        "rayon/ordered-run-ids".to_string()
    } else {
        "sequential".to_string()
    }
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
    agent_threads: usize,
    nix_provenance: Option<&model::NixProvenance>,
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
    if agent_threads > 1 {
        factors.push("agent_threads".to_string());
    }
    if nix_provenance.is_some() {
        factors.push("nix_provenance".to_string());
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
    use super::{resolve_ordeal_regress, Cli};
    use clap::Parser;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock")
    }

    struct EnvRestore {
        new_value: Option<String>,
        legacy_value: Option<String>,
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match self.new_value.as_ref() {
                Some(value) => std::env::set_var("COGITATOR_ORDEAL_REGRESS", value),
                None => std::env::remove_var("COGITATOR_ORDEAL_REGRESS"),
            }
            match self.legacy_value.as_ref() {
                Some(value) => std::env::set_var("COGITATOR_GAUNTLET_REGRESS", value),
                None => std::env::remove_var("COGITATOR_GAUNTLET_REGRESS"),
            }
        }
    }

    #[test]
    fn clap_rejects_agent_only_flags_without_agent_mode() {
        let result = Cli::try_parse_from(["cogitator", "run", "--threads", "2"]);
        assert!(result.is_err());
        let result = Cli::try_parse_from(["cogitator", "run", "--llm", "on"]);
        assert!(result.is_err());
    }

    #[test]
    fn clap_rejects_agent_mode_conflicts() {
        let result =
            Cli::try_parse_from(["cogitator", "run", "--agent", "clawdbot", "--replay", "out"]);
        assert!(result.is_err());
    }

    #[test]
    fn clap_accepts_agent_mode_defaults() {
        let result = Cli::try_parse_from(["cogitator", "run", "--agent", "clawdbot"]);
        assert!(result.is_ok());
    }

    #[test]
    fn clap_accepts_ordeal_agent() {
        let result = Cli::try_parse_from(["cogitator", "run", "--agent", "ordeal"]);
        assert!(result.is_ok());
    }

    #[test]
    fn clap_accepts_gauntlet_agent_alias() {
        let result = Cli::try_parse_from(["cogitator", "run", "--agent", "gauntlet"]);
        assert!(result.is_ok());
    }

    #[test]
    fn clap_rejects_unknown_agent() {
        let result = Cli::try_parse_from(["cogitator", "run", "--agent", "foo"]);
        assert!(result.is_err());
    }

    #[test]
    fn legacy_gauntlet_regress_env_alias() {
        let _guard = env_lock();
        let _restore = EnvRestore {
            new_value: std::env::var("COGITATOR_ORDEAL_REGRESS").ok(),
            legacy_value: std::env::var("COGITATOR_GAUNTLET_REGRESS").ok(),
        };

        std::env::remove_var("COGITATOR_ORDEAL_REGRESS");
        std::env::remove_var("COGITATOR_GAUNTLET_REGRESS");
        assert!(!resolve_ordeal_regress());

        std::env::set_var("COGITATOR_GAUNTLET_REGRESS", "1");
        assert!(resolve_ordeal_regress());

        std::env::set_var("COGITATOR_ORDEAL_REGRESS", "0");
        assert!(!resolve_ordeal_regress());

        std::env::set_var("COGITATOR_ORDEAL_REGRESS", "true");
        std::env::set_var("COGITATOR_GAUNTLET_REGRESS", "0");
        assert!(resolve_ordeal_regress());
    }
}
