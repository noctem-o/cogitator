use std::fs;

use cogitator::canonical_json;
use cogitator::drift;
use cogitator::model::{
    RunMetadata, WitnessManifest, WitnessedMetadata, TRACE_SCHEMA_VERSION,
    WITNESS_MANIFEST_SCHEMA_VERSION,
};
use cogitator::tooling::{ToolCall, ToolMode, ToolOutcome, ToolRequest, ToolTranscriptRecord};
use cogitator::trace;
use tempfile::tempdir;

#[test]
fn canonical_json_is_byte_stable() {
    let value = serde_json::json!({
        "b": {"z": 2, "a": 1},
        "a": [3, 2, 1],
    });

    let first = canonical_json::to_vec(&value).expect("first encoding");
    let second = canonical_json::to_vec(&value).expect("second encoding");

    assert_eq!(first, second);
}

#[test]
fn canonical_json_matches_expected_bytes() {
    let value = serde_json::json!({
        "z": [3, 2, 1],
        "a": {"emoji": "\u{1F600}", "x": 1},
    });

    let bytes = canonical_json::to_vec(&value).expect("canonical bytes");
    // RFC 8785 §3.2.2.2: non-ASCII characters MUST be emitted as raw UTF-8,
    // not as \uXXXX escape sequences. The emoji U+1F600 serialises as the
    // four-byte UTF-8 sequence for 😀, not as a backslash-u escape.
    assert_eq!(
        String::from_utf8(bytes).expect("utf8"),
        "{\"a\":{\"emoji\":\"\u{1F600}\",\"x\":1},\"z\":[3,2,1]}"
    );
}

#[test]
fn canonical_json_rejects_floats_in_release_and_debug() {
    let value = serde_json::json!({"value": 0.25});
    let err = canonical_json::to_vec(&value).expect_err("floats must be rejected");
    assert!(
        err.to_string().contains("rejected non-integer number"),
        "unexpected error: {err}"
    );
}

#[test]
fn verify_witness_bundle_recomputes_hashes() {
    let temp = tempdir().expect("tempdir");
    let dir = temp.path();

    let meta_path = dir.join("meta.json");
    let agent_trace_path = dir.join("agent_trace.json");
    let tool_transcript_path = dir.join("tool_transcript.json");
    let drift_report_path = dir.join("drift_report.json");
    let hash_chain_path = dir.join("hash_chain.txt");
    let witness_root_path = dir.join("witness_root.txt");
    let witness_manifest_path = dir.join("witness_manifest.json");

    let metadata = RunMetadata {
        witnessed: WitnessedMetadata {
            schema_version: TRACE_SCHEMA_VERSION,
            seed: 7,
            requested_runs: 1,
            executed_runs: 1,
            parallel: false,
            parallel_strategy: "sequential".to_string(),
            case_filter: Some(0),
            entropy_sources: vec!["rng:StdRng(seed)".to_string()],
            total_rng_calls: 0,
            chaos_profile: None,
            pass_threshold: None,
            ..Default::default()
        },
        provenance: cogitator::model::ProvenanceMetadata {
            created_at: "2024-01-01T00:00:00Z".to_string(),
            git_rev: None,
            rustc_version: None,
            cargo_version: None,
            nix_store_path: None,
            agent_threads: Some(1),
            rayon_threads_requested: None,
            rayon_threads_resolved: None,
            nix_provenance: None,
            variability_factors: Vec::new(),
        },
    };

    let agent_trace = vec![cogitator::agent::AgentTraceEntry {
        step: 0,
        role: "assistant".to_string(),
        thought: "probe".to_string(),
        action: "lookup".to_string(),
        tool_requests: vec![ToolRequest {
            tool_name: "clawdbot.lookup".to_string(),
            arguments: serde_json::json!({"case": "alpha"}),
        }],
        is_final: true,
    }];

    let tool_call = ToolCall {
        step: 0,
        tool_call_idx: 0,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"case": "alpha"}),
        outcome: ToolOutcome::Ok {
            output: serde_json::json!({"ok": true}),
            simulated_latency_ms: None,
        },
        fault: None,
    };

    let transcript = ToolTranscriptRecord {
        schema_version: cogitator::tooling::TOOL_TRANSCRIPT_SCHEMA_VERSION,
        mode: ToolMode::Live,
        entries: vec![tool_call],
        ..Default::default()
    };

    let drift_report = drift::DriftReport {
        schema_version: drift::DRIFT_SCHEMA_VERSION,
        drifted: false,
        issues: Vec::new(),
    };

    canonical_json::write_json(&meta_path, &metadata, "meta.json").expect("meta.json");
    canonical_json::write_json(&agent_trace_path, &agent_trace, "agent_trace.json")
        .expect("agent_trace.json");
    canonical_json::write_json(&tool_transcript_path, &transcript, "tool_transcript.json")
        .expect("tool_transcript.json");
    canonical_json::write_json(&drift_report_path, &drift_report, "drift_report.json")
        .expect("drift_report.json");

    let chain =
        drift::build_hash_chain(&agent_trace, &transcript.entries).expect("build hash chain");
    fs::write(&hash_chain_path, chain.join("\n") + "\n").expect("hash_chain.txt");

    let witness_root = trace::compute_agent_witness_root(
        &metadata.witnessed,
        &agent_trace,
        &transcript.entries,
        &transcript.phantom_entries,
    )
    .expect("witness root");
    fs::write(&witness_root_path, format!("{}\n", witness_root)).expect("witness_root.txt");

    let artifact_hashes = drift::artifact_hashes(&[
        &meta_path,
        &agent_trace_path,
        &tool_transcript_path,
        &drift_report_path,
        &hash_chain_path,
        &witness_root_path,
    ])
    .expect("artifact hashes");

    let bundle_hash = drift::bundle_hash(&artifact_hashes).expect("bundle hash");

    let witness_manifest = WitnessManifest {
        schema_version: WITNESS_MANIFEST_SCHEMA_VERSION,
        run_id: 0,
        agent: "clawdbot".to_string(),
        mode: "live".to_string(),
        meta_json: meta_path.display().to_string(),
        agent_trace_json: agent_trace_path.display().to_string(),
        tool_transcript_json: tool_transcript_path.display().to_string(),
        drift_report_json: drift_report_path.display().to_string(),
        hash_chain_txt: hash_chain_path.display().to_string(),
        chaos_profile_json: None,
        witness_root_txt: Some(witness_root_path.display().to_string()),
        nix_provenance_json: None,
        artifact_hashes: artifact_hashes.clone(),
        bundle_hash,
        replay_source: None,
    };

    canonical_json::write_json(
        &witness_manifest_path,
        &witness_manifest,
        "witness_manifest.json",
    )
    .expect("witness_manifest.json");

    let report = drift::verify_witness_bundle(dir).expect("verify witness bundle");
    assert!(report.verified, "issues: {:?}", report.issues);
    assert!(report.issues.is_empty());
}
