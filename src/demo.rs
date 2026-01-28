use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::drift::DriftReport;
use crate::{run_agent, DemoDriftArgs, RunArgs};

pub fn run_drift_demo(args: DemoDriftArgs) -> Result<()> {
    if args.clean && args.out_dir.exists() {
        fs::remove_dir_all(&args.out_dir).with_context(|| "failed to clean demo output dir")?;
    }
    fs::create_dir_all(&args.out_dir).with_context(|| "failed to create demo output dir")?;

    let baseline_dir = args.out_dir.join("baseline");
    let regressed_dir = args.out_dir.join("regressed");

    let baseline_args = RunArgs {
        seed: args.seed,
        runs: 1,
        case: Some(0),
        out_dir: baseline_dir.clone(),
        clean: true,
        no_tui: true,
        parallel: false,
        threads: None,
        created_at: None,
        agent: Some("clawdbot".to_string()),
        replay: None,
    };
    run_agent(baseline_args)?;

    let replay_source = baseline_dir.join("run_0000");
    let regressed_args = RunArgs {
        seed: args.seed,
        runs: 1,
        case: Some(0),
        out_dir: regressed_dir.clone(),
        clean: true,
        no_tui: true,
        parallel: false,
        threads: None,
        created_at: None,
        agent: Some("clawdbot-regressed".to_string()),
        replay: Some(replay_source.clone()),
    };
    run_agent(regressed_args)?;

    let drift_path: PathBuf = regressed_dir.join("run_0000").join("drift_report.json");
    let drift_file = fs::File::open(&drift_path).with_context(|| "failed to read drift report")?;
    let drift_report: DriftReport =
        serde_json::from_reader(drift_file).with_context(|| "failed to parse drift report")?;

    println!("Drift demo output:");
    println!("  baseline: {}", baseline_dir.display());
    println!("  regressed: {}", regressed_dir.display());
    println!("  drifted: {}", drift_report.drifted);
    if drift_report.issues.is_empty() {
        println!("  no drift issues detected");
    } else {
        println!("  issues:");
        for issue in &drift_report.issues {
            println!(
                "    - index {} step {:?} {:?} ({})",
                issue.index, issue.step, issue.kind, issue.message
            );
        }
    }

    Ok(())
}
