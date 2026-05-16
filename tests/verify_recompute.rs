use std::fs;

use cogitator::agent::AgentTraceEntry;
use cogitator::canonical_json;
use cogitator::model::{
    ProvenanceMetadata, RunMetadata, WitnessManifest, WitnessedMetadata, TRACE_SCHEMA_VERSION,
    WITNESS_MANIFEST_SCHEMA_VERSION,
};
use cogitator::tooling::{
    PhantomEntry, ToolCall, ToolError, ToolErrorKind, ToolMode, ToolOutcome, ToolRequest,
    ToolTranscriptRecord, TranscriptFault,
};
use cogitator::{drift, trace, verify};
use tempfile::tempdir;

fn write_bundle(dir: &std::path::Path, call: ToolCall) -> anyhow::Result<String> {
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
        provenance: ProvenanceMetadata {
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

    let agent_trace = vec![AgentTraceEntry {
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

    let transcript = ToolTranscriptRecord {
        schema_version: cogitator::tooling::TOOL_TRANSCRIPT_SCHEMA_VERSION,
        mode: ToolMode::Live,
        entries: vec![call],
        ..Default::default()
    };

    let drift_report = drift::DriftReport {
        schema_version: drift::DRIFT_SCHEMA_VERSION,
        drifted: false,
        issues: Vec::new(),
    };

    canonical_json::write_json(&meta_path, &metadata, "meta.json")?;
    canonical_json::write_json(&agent_trace_path, &agent_trace, "agent_trace.json")?;
    canonical_json::write_json(&tool_transcript_path, &transcript, "tool_transcript.json")?;
    canonical_json::write_json(&drift_report_path, &drift_report, "drift_report.json")?;

    let chain = drift::build_hash_chain(
        &agent_trace,
        &transcript.entries,
        &transcript.phantom_entries,
    )?;
    fs::write(&hash_chain_path, chain.join("\n") + "\n")?;

    let witness_root = trace::compute_agent_witness_root(
        &metadata.witnessed,
        &agent_trace,
        &transcript.entries,
        &transcript.phantom_entries,
    )?;
    fs::write(&witness_root_path, format!("{}\n", witness_root))?;

    let artifact_hashes = drift::artifact_hashes(&[
        &meta_path,
        &agent_trace_path,
        &tool_transcript_path,
        &drift_report_path,
        &hash_chain_path,
        &witness_root_path,
    ])?;
    let bundle_hash = drift::bundle_hash(&artifact_hashes)?;

    let witness_manifest = WitnessManifest {
        schema_version: WITNESS_MANIFEST_SCHEMA_VERSION,
        run_id: 0,
        agent: "clawdbot".to_string(),
        mode: "live".to_string(),
        meta_json: "meta.json".to_string(),
        agent_trace_json: "agent_trace.json".to_string(),
        tool_transcript_json: "tool_transcript.json".to_string(),
        drift_report_json: "drift_report.json".to_string(),
        hash_chain_txt: "hash_chain.txt".to_string(),
        chaos_profile_json: None,
        witness_root_txt: Some("witness_root.txt".to_string()),
        nix_provenance_json: None,
        artifact_hashes,
        bundle_hash,
        replay_source: None,
    };
    canonical_json::write_json(
        &witness_manifest_path,
        &witness_manifest,
        "witness_manifest.json",
    )?;

    Ok(witness_root)
}

fn base_call() -> ToolCall {
    ToolCall {
        step: 0,
        tool_call_idx: 0,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"case": "alpha"}),
        outcome: ToolOutcome::Err {
            error: ToolError {
                error_kind: ToolErrorKind::Timeout,
                message: Some("timeout".to_string()),
            },
            simulated_latency_ms: Some(120),
        },
        fault: Some(TranscriptFault::Timeout {
            domain: "tooling".to_string(),
            timeout_ms: Some(500),
        }),
    }
}

#[test]
fn recompute_witness_root_happy_path() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");

    let receipt = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect("recompute succeeds");
    assert!(receipt.matched);
}

#[test]
fn recompute_witness_root_detects_semantic_tamper() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");

    let path = temp.path().join("tool_transcript.json");
    let mut transcript: ToolTranscriptRecord =
        serde_json::from_slice(&fs::read(&path).expect("read transcript")).expect("parse");
    if let ToolOutcome::Err { error, .. } = &mut transcript.entries[0].outcome {
        error.message = Some("changed-message".to_string());
    }
    canonical_json::write_json(&path, &transcript, "tool_transcript.json").expect("rewrite");

    let receipt = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect("recompute succeeds");
    assert!(!receipt.matched);
    let diff = receipt
        .differing_component
        .expect("semantic mismatch should include component diagnostics");
    assert!(diff.contains("metadata_only="), "diagnostic: {diff}");
    assert!(diff.contains("trace_only="), "diagnostic: {diff}");
    assert!(diff.contains("toolcalls_only="), "diagnostic: {diff}");
    assert!(diff.contains("full_semantic="), "diagnostic: {diff}");
}

#[test]
fn recompute_witness_root_ignores_provenance_only_mutation() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");

    let path = temp.path().join("tool_transcript.json");
    let mut transcript: ToolTranscriptRecord =
        serde_json::from_slice(&fs::read(&path).expect("read transcript")).expect("parse");
    if let ToolOutcome::Err {
        simulated_latency_ms,
        ..
    } = &mut transcript.entries[0].outcome
    {
        *simulated_latency_ms = None;
    }
    if let Some(TranscriptFault::Timeout { timeout_ms, .. }) = &mut transcript.entries[0].fault {
        *timeout_ms = None;
    }
    canonical_json::write_json(&path, &transcript, "tool_transcript.json").expect("rewrite");

    let receipt = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect("recompute succeeds");
    assert!(receipt.matched);
}

#[test]
fn recompute_rejects_duplicate_manifest_keys() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");

    let manifest_path = temp.path().join("witness_manifest.json");
    std::fs::write(&manifest_path, r#"{"schema_version":1,"schema_version":1}"#)
        .expect("write duplicate manifest");

    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("duplicate keys must be rejected");
    assert!(err
        .to_string()
        .contains("duplicate JSON object member name"));
}

#[test]
fn recompute_detects_phantom_entry_tamper() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");

    let path = temp.path().join("tool_transcript.json");
    let mut transcript: ToolTranscriptRecord =
        serde_json::from_slice(&fs::read(&path).expect("read transcript")).expect("parse");
    transcript.entries.clear();
    transcript.phantom_entries = vec![PhantomEntry {
        step: 0,
        tool_call_idx: 0,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"case":"alpha"}),
        disposition: cogitator::policy::PhantomDisposition::Blocked,
        rule_id: Some("no-lookup".to_string()),
        reason: "blocked by policy".to_string(),
    }];
    canonical_json::write_json(&path, &transcript, "tool_transcript.json").expect("rewrite");

    let receipt = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect("recompute succeeds");
    assert!(!receipt.matched);
    let diff = receipt
        .differing_component
        .expect("semantic mismatch should include diagnostics");
    assert!(diff.contains("interceptions_only="), "diagnostic: {diff}");
}

#[test]
fn recompute_rejects_orphan_tool_call_step() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let path = temp.path().join("tool_transcript.json");
    let mut transcript: ToolTranscriptRecord =
        serde_json::from_slice(&fs::read(&path).expect("read transcript")).expect("parse");
    transcript.entries[0].step = 99;
    canonical_json::write_json(&path, &transcript, "tool_transcript.json").expect("rewrite");

    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("orphan step must fail");
    assert!(err.to_string().contains("orphan tool call at absent step"));
}

#[test]
fn recompute_rejects_orphan_phantom_step() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let path = temp.path().join("tool_transcript.json");
    let mut transcript: ToolTranscriptRecord =
        serde_json::from_slice(&fs::read(&path).expect("read transcript")).expect("parse");
    transcript.entries.clear();
    transcript.phantom_entries.push(PhantomEntry {
        step: 99,
        tool_call_idx: 0,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"case":"alpha"}),
        disposition: cogitator::policy::PhantomDisposition::Blocked,
        rule_id: None,
        reason: "blocked".to_string(),
    });
    canonical_json::write_json(&path, &transcript, "tool_transcript.json").expect("rewrite");

    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("orphan step must fail");
    assert!(err
        .to_string()
        .contains("orphan phantom entry at absent step"));
}

#[test]
fn recompute_rejects_duplicate_real_tool_call_idx() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let path = temp.path().join("tool_transcript.json");
    let mut transcript: ToolTranscriptRecord =
        serde_json::from_slice(&fs::read(&path).expect("read transcript")).expect("parse");
    let mut dup = transcript.entries[0].clone();
    dup.step = 0;
    dup.tool_call_idx = 0;
    transcript.entries.push(dup);
    canonical_json::write_json(&path, &transcript, "tool_transcript.json").expect("rewrite");

    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("duplicate idx must fail");
    assert!(err.to_string().contains("duplicate tool_call_idx"));
}

#[test]
fn recompute_rejects_real_phantom_tool_call_idx_collision() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let path = temp.path().join("tool_transcript.json");
    let mut transcript: ToolTranscriptRecord =
        serde_json::from_slice(&fs::read(&path).expect("read transcript")).expect("parse");
    transcript.phantom_entries.push(PhantomEntry {
        step: 0,
        tool_call_idx: 0,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"case":"alpha"}),
        disposition: cogitator::policy::PhantomDisposition::Phantom,
        rule_id: None,
        reason: "phantom".to_string(),
    });
    canonical_json::write_json(&path, &transcript, "tool_transcript.json").expect("rewrite");

    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("collision must fail");
    assert!(err
        .to_string()
        .contains("executed/phantom tool_call_idx collision"));
}

#[test]
fn recompute_rejects_non_contiguous_tool_call_idx() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let path = temp.path().join("tool_transcript.json");
    let mut transcript: ToolTranscriptRecord =
        serde_json::from_slice(&fs::read(&path).expect("read transcript")).expect("parse");
    transcript.entries[0].tool_call_idx = 1;
    canonical_json::write_json(&path, &transcript, "tool_transcript.json").expect("rewrite");

    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("gap must fail");
    assert!(err.to_string().contains("non-contiguous tool_call_idx"));
}

#[test]
fn recompute_rejects_non_increasing_agent_trace_steps() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let path = temp.path().join("agent_trace.json");
    let mut trace_entries: Vec<AgentTraceEntry> =
        serde_json::from_slice(&fs::read(&path).expect("read trace")).expect("parse");
    trace_entries.push(trace_entries[0].clone());
    canonical_json::write_json(&path, &trace_entries, "agent_trace.json").expect("rewrite");

    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("duplicate steps must fail");
    assert!(err
        .to_string()
        .contains("agent_trace steps must be strictly increasing"));
}

#[test]
fn recompute_rejects_absolute_manifest_path() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let manifest_path = temp.path().join("witness_manifest.json");
    let mut manifest: WitnessManifest =
        serde_json::from_slice(&fs::read(&manifest_path).expect("read manifest")).expect("parse");
    manifest.meta_json = temp.path().join("meta.json").to_string_lossy().to_string();
    canonical_json::write_json(&manifest_path, &manifest, "witness_manifest.json")
        .expect("rewrite");
    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("absolute path should fail");
    assert!(err
        .to_string()
        .contains("absolute manifest path is forbidden"));
}

#[test]
fn recompute_rejects_manifest_parent_escape() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let manifest_path = temp.path().join("witness_manifest.json");
    let mut manifest: WitnessManifest =
        serde_json::from_slice(&fs::read(&manifest_path).expect("read manifest")).expect("parse");
    manifest.meta_json = "../meta.json".to_string();
    canonical_json::write_json(&manifest_path, &manifest, "witness_manifest.json")
        .expect("rewrite");
    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("escape should fail");
    assert!(err
        .to_string()
        .contains("failed to canonicalize manifest artifact path"));
}

#[test]
fn recompute_rejects_missing_nested_manifest_path_without_fallback() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let manifest_path = temp.path().join("witness_manifest.json");
    let mut manifest: WitnessManifest =
        serde_json::from_slice(&fs::read(&manifest_path).expect("read manifest")).expect("parse");
    manifest.meta_json = "nested/meta.json".to_string();
    canonical_json::write_json(&manifest_path, &manifest, "witness_manifest.json")
        .expect("rewrite");
    let err = verify::recompute_agent_witness_root_from_bundle(temp.path(), None)
        .expect_err("missing nested path should fail");
    assert!(err
        .to_string()
        .contains("failed to canonicalize manifest artifact path"));
}

#[test]
fn recompute_accepts_moved_bundle_with_relative_manifest_paths() {
    let temp = tempdir().expect("tempdir");
    write_bundle(temp.path(), base_call()).expect("bundle");
    let copied = tempdir().expect("copied");
    for name in [
        "meta.json",
        "agent_trace.json",
        "tool_transcript.json",
        "drift_report.json",
        "hash_chain.txt",
        "witness_root.txt",
        "witness_manifest.json",
    ] {
        fs::copy(temp.path().join(name), copied.path().join(name)).expect("copy artifact");
    }
    let receipt = verify::recompute_agent_witness_root_from_bundle(copied.path(), None)
        .expect("recompute on copied bundle");
    assert!(receipt.matched);
}
