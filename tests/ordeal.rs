use cogitator::model::WitnessedMetadata;
use cogitator::ordeal::{OrdealConfig, TaskSuite, ORDEAL_TASKS_PATH};
use cogitator::report::DriftIssue;
use cogitator::tooling::ToolTranscript;
use cogitator::{drift, trace};
use rayon::ThreadPoolBuilder;

fn run_ordeal_root(seed: u64, pass_threshold: &str) -> String {
    let suite = TaskSuite::load(std::path::Path::new(ORDEAL_TASKS_PATH)).expect("suite");
    let mut transcript = ToolTranscript::new_live(None);
    let config = OrdealConfig {
        seed,
        run_id: 0,
        case_id: "case".to_string(),
        pass_threshold_f32: pass_threshold.parse().expect("threshold"),
        pass_threshold_witnessed: format!(
            "f32:0x{:08X}",
            pass_threshold.parse::<f32>().expect("threshold").to_bits()
        ),
        regress: false,
    };
    let output =
        cogitator::ordeal::run_ordeal(&suite, &config, &mut transcript).expect("ordeal run");
    let record = transcript.into_record();
    let metadata = WitnessedMetadata {
        schema_version: cogitator::model::TRACE_SCHEMA_VERSION,
        seed,
        requested_runs: 1,
        executed_runs: 1,
        parallel: false,
        parallel_strategy: "sequential".to_string(),
        case_filter: Some(0),
        entropy_sources: vec![
            "rng:StdRng(seed)".to_string(),
            "tooling:stubbed-or-replay".to_string(),
        ],
        total_rng_calls: output.total_rng_calls,
        chaos_profile: None,
        pass_threshold: Some(format!(
            "f32:0x{:08X}",
            pass_threshold.parse::<f32>().expect("threshold").to_bits()
        )),
    };
    trace::compute_agent_witness_root(&metadata, &output.agent_trace, &record.entries)
        .expect("witness root")
}

#[test]
fn ordeal_task_loader_validates_count() {
    let suite = TaskSuite::load(std::path::Path::new(ORDEAL_TASKS_PATH)).expect("suite");
    assert_eq!(suite.tasks.len(), 50);
    assert_eq!(suite.tasks.first().map(|task| task.task_id), Some(0));
    assert_eq!(suite.tasks.last().map(|task| task.task_id), Some(49));
}

#[test]
fn ordeal_witness_root_thread_invariant() {
    let roots: Vec<String> = [1usize, 16]
        .iter()
        .map(|threads| {
            ThreadPoolBuilder::new()
                .num_threads(*threads)
                .build()
                .unwrap()
                .install(|| run_ordeal_root(42, "0.5"))
        })
        .collect();
    assert!(roots.iter().all(|root| root == &roots[0]));
}

#[test]
fn ordeal_witness_root_changes_with_threshold() {
    let root_a = run_ordeal_root(42, "0.346");
    let root_b = run_ordeal_root(42, "0.5");
    assert_ne!(root_a, root_b);
}

#[test]
fn ordeal_replay_regression_reports_drift() {
    let suite = TaskSuite::load(std::path::Path::new(ORDEAL_TASKS_PATH)).expect("suite");
    let config = OrdealConfig {
        seed: 42,
        run_id: 0,
        case_id: "case".to_string(),
        pass_threshold_f32: 0.5,
        pass_threshold_witnessed: "f32:0x3F000000".to_string(),
        regress: false,
    };
    let mut live_transcript = ToolTranscript::new_live(None);
    let _live_output =
        cogitator::ordeal::run_ordeal(&suite, &config, &mut live_transcript).expect("live");
    let live_record = live_transcript.into_record();

    let mut replay_transcript = ToolTranscript::new_replay(live_record.clone());
    let config_regressed = OrdealConfig {
        regress: true,
        ..config
    };
    let replay_output =
        cogitator::ordeal::run_ordeal(&suite, &config_regressed, &mut replay_transcript)
            .expect("replay");
    let replay_record = replay_transcript.into_record();

    let mut report = drift::detect_transcript_drift(&live_record, &replay_record);
    report.issues.extend(replay_output.issues);
    report.drifted = report.drifted || !report.issues.is_empty();

    assert!(report.drifted);
    let has_ordeal_issue = report.issues.iter().any(|issue| match issue {
        DriftIssue::OrdealOutputMismatch {
            step,
            tool_name,
            json_pointer,
            issue_kind,
            expected,
            actual,
            ..
        } => {
            *step == 0
                && tool_name == "ordeal.lookup"
                && json_pointer == "/payload/tags/0"
                && issue_kind == "missing"
                && !expected.is_empty()
                && actual == "missing"
        }
        _ => false,
    });
    assert!(has_ordeal_issue);
}

#[test]
fn ordeal_check_command_detects_drift() {
    let temp = tempfile::tempdir().expect("tempdir");
    let golden = temp.path().join("golden.txt");
    std::fs::write(&golden, "deadbeef\n").expect("write golden");

    let bin = env!("CARGO_BIN_EXE_cogitator");
    let output = std::process::Command::new(bin)
        .current_dir(temp.path())
        .arg("ordeal")
        .arg("check")
        .arg("--golden")
        .arg(golden.as_os_str())
        .output()
        .expect("run ordeal check");

    assert!(!output.status.success());
    let agent_trace = ordeal_run_dir(temp.path()).join("agent_trace.json");
    assert!(agent_trace.exists(), "missing {}", agent_trace.display());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("drift detected"), "stderr: {stderr}");
}

#[test]
fn ordeal_check_command_accepts_matching_golden() {
    let temp = tempfile::tempdir().expect("tempdir");
    let golden = temp.path().join("golden.txt");

    let bin = env!("CARGO_BIN_EXE_cogitator");
    let update = std::process::Command::new(bin)
        .current_dir(temp.path())
        .arg("ordeal")
        .arg("check")
        .arg("--golden")
        .arg(golden.as_os_str())
        .arg("--update-golden")
        .output()
        .expect("run ordeal check update");
    assert!(update.status.success());

    let output = std::process::Command::new(bin)
        .current_dir(temp.path())
        .arg("ordeal")
        .arg("check")
        .arg("--golden")
        .arg(golden.as_os_str())
        .output()
        .expect("run ordeal check");

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let agent_trace = ordeal_run_dir(temp.path()).join("agent_trace.json");
    assert!(agent_trace.exists(), "missing {}", agent_trace.display());
}

fn ordeal_run_dir(root: &std::path::Path) -> std::path::PathBuf {
    let out_ci = root.join("out_ci");
    let mut run_dirs = std::fs::read_dir(&out_ci)
        .expect("read out_ci")
        .map(|entry| entry.expect("read out_ci entry").path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    run_dirs.sort();
    assert_eq!(
        run_dirs.len(),
        1,
        "expected exactly one run dir under {}",
        out_ci.display()
    );
    run_dirs.remove(0)
}
