use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::agent::AgentTraceEntry;
use crate::canonical_json;
use crate::model::WitnessManifest;
use crate::report::DriftIssue;
use crate::tooling::{ToolCall, ToolTranscriptRecord};

pub const DRIFT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftReport {
    pub schema_version: u32,
    pub drifted: bool,
    pub issues: Vec<DriftIssue>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyReport {
    pub verified: bool,
    pub issues: Vec<String>,
    pub bundle_hash_expected: String,
    pub bundle_hash_actual: String,
}

pub fn detect_transcript_drift(
    expected: &ToolTranscriptRecord,
    actual: &ToolTranscriptRecord,
) -> DriftReport {
    let mut issues = Vec::new();

    if expected.schema_version != actual.schema_version {
        issues.push(DriftIssue::TranscriptSchemaMismatch {
            expected: expected.schema_version,
            actual: actual.schema_version,
        });
    }

    let mode_is_compatible = expected.mode == actual.mode
        || (expected.mode == crate::tooling::ToolMode::Live
            && actual.mode == crate::tooling::ToolMode::Replay)
        || (expected.mode == crate::tooling::ToolMode::Replay
            && actual.mode == crate::tooling::ToolMode::Live);

    if !mode_is_compatible {
        issues.push(DriftIssue::TranscriptModeMismatch {
            expected: format!("{:?}", expected.mode),
            actual: format!("{:?}", actual.mode),
        });
    }

    if expected.entries.len() != actual.entries.len() {
        issues.push(DriftIssue::TranscriptLengthMismatch {
            expected: expected.entries.len() as u32,
            actual: actual.entries.len() as u32,
        });
    }

    let n = expected.entries.len().min(actual.entries.len());
    for i in 0..n {
        let e = &expected.entries[i];
        let a = &actual.entries[i];

        if e.step != a.step {
            issues.push(DriftIssue::ToolStepMismatch {
                index: i as u32,
                expected: e.step,
                actual: a.step,
            });
        }

        if e.tool_call_idx != a.tool_call_idx {
            issues.push(DriftIssue::ToolCallIndexMismatch {
                index: i as u32,
                expected: e.tool_call_idx,
                actual: a.tool_call_idx,
            });
        }

        if e.tool_name != a.tool_name || e.request != a.request {
            issues.push(DriftIssue::ToolRequestMismatch { index: i as u32 });
        }

        if e.outcome != a.outcome {
            issues.push(DriftIssue::ToolOutcomeMismatch { index: i as u32 });
        }

        if e.fault != a.fault {
            issues.push(DriftIssue::ToolFaultMismatch { index: i as u32 });
        }
    }

    DriftReport {
        schema_version: DRIFT_SCHEMA_VERSION,
        drifted: !issues.is_empty(),
        issues,
    }
}

/// A human-friendly hash chain over the agent trace + tool calls.
///
/// This is intentionally *not* the witness root. It’s a debugging aid you can open in a terminal.
pub fn build_hash_chain(
    agent_trace: &[AgentTraceEntry],
    tool_calls: &[ToolCall],
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut prev = String::from("genesis");

    for (i, entry) in agent_trace.iter().enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(prev.as_bytes());
        hasher.update(i.to_le_bytes());
        hasher.update(entry.role.as_bytes());
        hasher.update(entry.thought.as_bytes());
        hasher.update(entry.action.as_bytes());
        hasher.update(canonical_json::to_vec(&entry.tool_requests)?);
        hasher.update([entry.is_final as u8]);

        let digest = hasher.finalize();
        prev = crate::hex::encode(&digest);
        out.push(format!("agent[{i}] {prev}"));
    }

    for (i, call) in tool_calls.iter().enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(prev.as_bytes());
        hasher.update(i.to_le_bytes());
        hasher.update(call.step.to_le_bytes());
        hasher.update(call.tool_call_idx.to_le_bytes());
        hasher.update(call.tool_name.as_bytes());
        hasher.update(canonical_json::to_vec(&call.request)?);
        hasher.update(canonical_json::to_vec(&call.outcome)?);
        hasher.update(canonical_json::to_vec(&call.fault)?);

        let digest = hasher.finalize();
        prev = crate::hex::encode(&digest);
        out.push(format!("tool[{i}]  {prev}"));
    }

    Ok(out)
}

/// Compute SHA-256 over each artifact file and return a stable map from filename -> hash.
pub fn artifact_hashes(paths: &[&Path]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();

    for path in paths {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read artifact {}", path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let digest = hasher.finalize();

        let key = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        out.insert(key, crate::hex::encode(&digest));
    }

    Ok(out)
}

/// Compute a stable bundle hash over an artifact hash map.
///
/// The BTreeMap iteration order is deterministic.
pub fn bundle_hash(artifact_hashes: &BTreeMap<String, String>) -> Result<String> {
    let mut hasher = Sha256::new();
    // Bundle-domain separator bytes are protocol-critical and intentionally stable.
    hasher.update(b"COGITATOR:BUNDLE:V1\n");

    for (name, hash) in artifact_hashes {
        hasher.update(name.as_bytes());
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }

    Ok(crate::hex::encode(&hasher.finalize()))
}

/// Verify that the witness bundle is self-consistent.
///
/// Checks:
/// - each artifact hash matches the stored artifact_hashes map
/// - the bundle_hash matches
///
/// Writes `verify_report.json` into the witness directory.
pub fn verify_witness_bundle(witness_dir: &Path) -> Result<VerifyReport> {
    let manifest_path = witness_dir.join("witness_manifest.json");
    let manifest: WitnessManifest =
        crate::strict_json::from_path(&manifest_path, "witness_manifest.json")?;
    if manifest.schema_version != crate::model::WITNESS_MANIFEST_SCHEMA_VERSION {
        anyhow::bail!("witness manifest schema mismatch");
    }

    let mut artifact_paths: Vec<PathBuf> = vec![
        crate::io_utils::resolve_bundle_relative_path(witness_dir, &manifest.meta_json)?,
        crate::io_utils::resolve_bundle_relative_path(witness_dir, &manifest.agent_trace_json)?,
        crate::io_utils::resolve_bundle_relative_path(witness_dir, &manifest.tool_transcript_json)?,
        crate::io_utils::resolve_bundle_relative_path(witness_dir, &manifest.drift_report_json)?,
        crate::io_utils::resolve_bundle_relative_path(witness_dir, &manifest.hash_chain_txt)?,
    ];

    // Optional
    if let Some(ref path) = manifest.chaos_profile_json {
        artifact_paths.push(crate::io_utils::resolve_bundle_relative_path(
            witness_dir,
            path,
        )?);
    }
    if let Some(ref path) = manifest.witness_root_txt {
        artifact_paths.push(crate::io_utils::resolve_bundle_relative_path(
            witness_dir,
            path,
        )?);
    }
    if let Some(ref path) = manifest.nix_provenance_json {
        artifact_paths.push(crate::io_utils::resolve_bundle_relative_path(
            witness_dir,
            path,
        )?);
    }

    // Compute
    let artifact_refs: Vec<&Path> = artifact_paths.iter().map(|p| p.as_path()).collect();
    let actual_hashes = artifact_hashes(&artifact_refs)?;
    let actual_bundle_hash = bundle_hash(&actual_hashes)?;

    let expected_bundle_hash = manifest.bundle_hash.clone();

    let mut issues: Vec<String> = Vec::new();

    // Compare per-artifact hashes
    for (k, expected) in manifest.artifact_hashes.iter() {
        match actual_hashes.get(k) {
            Some(actual) if actual == expected => {}
            Some(actual) => issues.push(format!(
                "artifact hash mismatch for {}: expected {} got {}",
                k, expected, actual
            )),
            None => issues.push(format!("artifact missing from computed set: {}", k)),
        }
    }

    // Check for extra artifacts (not fatal, but worth reporting)
    for k in actual_hashes.keys() {
        if !manifest.artifact_hashes.contains_key(k) {
            issues.push(format!("extra artifact not in manifest: {}", k));
        }
    }

    // Compare bundle hash
    if actual_bundle_hash != expected_bundle_hash {
        issues.push(format!(
            "bundle hash mismatch: expected {} got {}",
            expected_bundle_hash, actual_bundle_hash
        ));
    }

    let verified = issues.is_empty();

    let report = VerifyReport {
        verified,
        issues,
        bundle_hash_expected: expected_bundle_hash,
        bundle_hash_actual: actual_bundle_hash,
    };

    let report_path = witness_dir.join("verify_report.json");
    canonical_json::write_json(&report_path, &report, "verify_report.json")?;

    Ok(report)
}
