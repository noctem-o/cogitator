use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::agent::AgentTraceEntry;
use crate::canonical_json;
use crate::model::{RunMetadata, WitnessedMetadata};
use crate::report::DriftIssue;
use crate::tooling::{ToolCall, ToolTranscriptRecord};

pub const DRIFT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftReport {
    pub schema_version: u32,
    pub drifted: bool,
    pub issues: Vec<DriftIssue>,
}

/// Compare two tool transcripts and detect drift
pub fn detect_transcript_drift(
    expected: ToolTranscriptRecord,
    actual: &ToolTranscriptRecord,
) -> DriftReport {
    let mut issues = Vec::new();

    if expected.entries.len() != actual.entries.len() {
        issues.push(DriftIssue::ToolCallCountMismatch {
            expected: expected.entries.len() as u32,
            actual: actual.entries.len() as u32,
        });
    }

    for (index, (exp, act)) in expected.entries.iter().zip(&actual.entries).enumerate() {
        if exp.step != act.step {
            issues.push(DriftIssue::ToolStepMismatch {
                index: index as u32,
                expected: exp.step,
                actual: act.step,
            });
        }

        if exp.tool_call_idx != act.tool_call_idx {
            issues.push(DriftIssue::ToolCallIndexMismatch {
                index: index as u32,
                expected: exp.tool_call_idx,
                actual: act.tool_call_idx,
            });
        }

        if exp.tool_name != act.tool_name || exp.request != act.request {
            issues.push(DriftIssue::ToolRequestMismatch {
                index: index as u32,
            });
        }

        if exp.outcome != act.outcome {
            issues.push(DriftIssue::ToolOutcomeMismatch {
                index: index as u32,
            });
        }
    }

    DriftReport {
        schema_version: DRIFT_SCHEMA_VERSION,
        drifted: !issues.is_empty(),
        issues,
    }
}

/// Build hash chain from agent trace and tool calls
pub fn build_hash_chain(
    agent_trace: &[AgentTraceEntry],
    tool_calls: &[ToolCall],
) -> Result<Vec<String>> {
    let mut chain = Vec::new();
    let mut hasher = Sha256::new();

    for entry in agent_trace {
        let entry_bytes = canonical_json::to_vec(&entry)?;
        hasher.update(&entry_bytes);
        chain.push(crate::hex::encode(&hasher.finalize_reset()));
    }

    for call in tool_calls {
        let call_bytes = crate::trace::encode_tool_call(call)?;
        hasher.update(&call_bytes);
        chain.push(crate::hex::encode(&hasher.finalize_reset()));
    }

    Ok(chain)
}

/// Compute artifact hashes for witness bundle
pub fn artifact_hashes(paths: &[&Path]) -> Result<Vec<String>> {
    let mut hashes = Vec::new();

    for path in paths {
        let content = std::fs::read(path)
            .with_context(|| format!("failed to read artifact: {}", path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        hashes.push(crate::hex::encode(&hasher.finalize()));
    }

    Ok(hashes)
}

/// Compute bundle hash from artifact hashes
pub fn bundle_hash(artifact_hashes: &[String]) -> Result<String> {
    let mut hasher = Sha256::new();
    for hash in artifact_hashes {
        hasher.update(hash.as_bytes());
    }
    Ok(crate::hex::encode(&hasher.finalize()))
}

#[derive(Debug)]
pub struct VerifyReport {
    pub verified: bool,
    pub issues: Vec<String>,
}

/// Verify witness bundle integrity
pub fn verify_witness_bundle(witness_dir: &Path) -> Result<VerifyReport> {
    let manifest_path = witness_dir.join("witness_manifest.json");
    
    if !manifest_path.exists() {
        return Ok(VerifyReport {
            verified: false,
            issues: vec!["witness_manifest.json not found".to_string()],
        });
    }

    let manifest_file = std::fs::File::open(&manifest_path)
        .context("failed to open witness_manifest.json")?;
    let manifest: crate::model::WitnessManifest =
        serde_json::from_reader(manifest_file).context("failed to parse witness_manifest.json")?;

    let mut issues = Vec::new();

    // Verify each artifact exists and hash matches
    let artifacts = vec![
        (&manifest.meta_json, "meta.json"),
        (&manifest.agent_trace_json, "agent_trace.json"),
        (&manifest.tool_transcript_json, "tool_transcript.json"),
        (&manifest.drift_report_json, "drift_report.json"),
        (&manifest.hash_chain_txt, "hash_chain.txt"),
    ];

    let mut computed_hashes = Vec::new();

    for (path_str, name) in artifacts {
        let path = witness_dir.join(path_str);
        if !path.exists() {
            issues.push(format!("{} missing", name));
            continue;
        }

        let content = std::fs::read(&path)
            .with_context(|| format!("failed to read {}", name))?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        computed_hashes.push(crate::hex::encode(&hasher.finalize()));
    }

    // Verify bundle hash
    let computed_bundle = bundle_hash(&computed_hashes)?;
    if computed_bundle != manifest.bundle_hash {
        issues.push(format!(
            "bundle hash mismatch: expected {}, computed {}",
            manifest.bundle_hash, computed_bundle
        ));
    }

    Ok(VerifyReport {
        verified: issues.is_empty(),
        issues,
    })
}
