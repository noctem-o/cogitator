use anyhow::{Context, Result};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::agent::AgentTraceEntry;
use crate::canonical_json;
use crate::model::WitnessManifest;
use crate::tooling::{ToolCall, ToolTranscriptRecord};
use crate::{model::RunMetadata, trace, witness};

pub const DRIFT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftReport {
    pub schema_version: u32,
    pub drifted: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyReport {
    pub schema_version: u32,
    pub verified: bool,
    pub issues: Vec<String>,
    pub artifact_results: Vec<ArtifactCheck>,
    pub bundle_hash_expected: String,
    pub bundle_hash_actual: String,
    pub witness_root_expected: Option<String>,
    pub witness_root_actual: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCheck {
    pub path: String,
    pub expected_hash: String,
    pub actual_hash: Option<String>,
    pub ok: bool,
}

pub const VERIFY_SCHEMA_VERSION: u32 = 1;

pub fn detect_transcript_drift(
    expected: &ToolTranscriptRecord,
    actual: &ToolTranscriptRecord,
) -> DriftReport {
    let mut issues = Vec::new();

    if expected.entries.len() != actual.entries.len() {
        issues.push(format!(
            "tool call count mismatch: expected {}, got {}",
            expected.entries.len(),
            actual.entries.len()
        ));
    }

    for (index, (exp, act)) in expected
        .entries
        .iter()
        .zip(actual.entries.iter())
        .enumerate()
    {
        if exp.step != act.step {
            issues.push(format!(
                "tool call step mismatch at {}: expected {}, got {}",
                index, exp.step, act.step
            ));
        }
        if exp.request != act.request {
            issues.push(format!("tool request mismatch at {}", index));
        }
        if exp.tool_call_idx != act.tool_call_idx {
            issues.push(format!(
                "tool call index mismatch at {}: expected {}, got {}",
                index, exp.tool_call_idx, act.tool_call_idx
            ));
        }
        if exp.fault != act.fault {
            issues.push(format!("tool fault mismatch at {}", index));
        }
        let exp_hash = response_hash(&exp.response);
        let act_hash = response_hash(&act.response);
        if exp_hash != act_hash {
            issues.push(format!("tool response hash mismatch at {}", index));
        }
    }

    DriftReport {
        schema_version: DRIFT_SCHEMA_VERSION,
        drifted: !issues.is_empty(),
        issues,
    }
}

pub fn build_hash_chain(
    agent_trace: &[AgentTraceEntry],
    tool_calls: &[ToolCall],
) -> Result<Vec<String>> {
    let mut chain = Vec::new();
    let mut current = initial_hash();

    for entry in agent_trace {
        let payload = serde_json::json!({
            "kind": "agent_trace",
            "payload": entry,
        });
        current = chained_hash(&current, &payload)?;
        chain.push(hex_string(&current));

        for call in tool_calls.iter().filter(|call| call.step == entry.step) {
            let payload = serde_json::json!({
                "kind": "tool_call",
                "payload": trace::tool_call_witness_value(call)?,
            });
            current = chained_hash(&current, &payload)?;
            chain.push(hex_string(&current));
        }
    }

    Ok(chain)
}

pub fn verify_witness_bundle(dir: &Path) -> Result<VerifyReport> {
    let manifest_path = dir.join("witness_manifest.json");
    let manifest_file = std::fs::File::open(&manifest_path)
        .with_context(|| "failed to open witness_manifest.json")?;
    let manifest: WitnessManifest = serde_json::from_reader(manifest_file)
        .with_context(|| "failed to parse witness_manifest.json")?;

    let agent_trace_file = std::fs::File::open(&manifest.agent_trace_json)
        .with_context(|| "failed to open agent_trace.json")?;
    let agent_trace: Vec<AgentTraceEntry> = serde_json::from_reader(agent_trace_file)
        .with_context(|| "failed to parse agent_trace.json")?;

    let tool_file = std::fs::File::open(&manifest.tool_transcript_json)
        .with_context(|| "failed to open tool_transcript.json")?;
    let tool_transcript: ToolTranscriptRecord = serde_json::from_reader(tool_file)
        .with_context(|| "failed to parse tool_transcript.json")?;

    let chain = build_hash_chain(&agent_trace, &tool_transcript.entries)?;
    let expected_chain = std::fs::read_to_string(&manifest.hash_chain_txt)
        .with_context(|| "failed to read hash_chain.txt")?;
    let expected_lines: Vec<&str> = expected_chain
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();

    if chain.len() != expected_lines.len() {
        anyhow::bail!(
            "hash chain length mismatch: expected {}, got {}",
            expected_lines.len(),
            chain.len()
        );
    }

    for (idx, (expected, actual)) in expected_lines.iter().zip(chain.iter()).enumerate() {
        if expected != actual {
            anyhow::bail!(
                "hash chain mismatch at line {}: expected {}, got {}",
                idx + 1,
                expected,
                actual
            );
        }
    }

    let mut issues = Vec::new();
    let mut artifact_results = Vec::new();

    for (path, expected_hash) in manifest.artifact_hashes.iter() {
        let actual_hash = hash_file(Path::new(path)).ok();
        let ok = actual_hash
            .as_ref()
            .map(|actual| actual == expected_hash)
            .unwrap_or(false);
        if !ok {
            issues.push(format!("artifact hash mismatch: {}", path));
        }
        artifact_results.push(ArtifactCheck {
            path: path.clone(),
            expected_hash: expected_hash.clone(),
            actual_hash,
            ok,
        });
    }

    let bundle_hash_actual = bundle_hash_from_map(&manifest.artifact_hashes)?;
    if bundle_hash_actual != manifest.bundle_hash {
        issues.push("bundle hash mismatch".to_string());
    }

    let witness_root_expected = manifest
        .witness_root_txt
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|value| value.trim().to_string());

    let witness_root_actual = compute_agent_witness_root(&manifest, &agent_trace, &tool_transcript)
        .ok();

    if let (Some(expected), Some(actual)) =
        (witness_root_expected.as_ref(), witness_root_actual.as_ref())
    {
        if expected != actual {
            issues.push("witness_root mismatch".to_string());
        }
    } else if manifest.witness_root_txt.is_none() {
        issues.push("witness_root missing (cannot verify)".to_string());
    }

    let report = VerifyReport {
        schema_version: VERIFY_SCHEMA_VERSION,
        verified: issues.is_empty(),
        issues,
        artifact_results,
        bundle_hash_expected: manifest.bundle_hash.clone(),
        bundle_hash_actual,
        witness_root_expected,
        witness_root_actual,
    };

    let report_path = dir.join("verify_report.json");
    canonical_json::write_json(&report_path, &report, "verify_report.json")?;

    Ok(report)
}

fn response_hash(response: &crate::tooling::ToolResponse) -> String {
    let payload = serde_json::json!({
        "tool_name": response.tool_name,
        "output": response.output,
        "success": response.success,
    });
    let mut hasher = Hasher::new();
    if let Ok(bytes) = canonical_json::to_vec(&payload) {
        hasher.update(&bytes);
    }
    hex_string(hasher.finalize().as_bytes())
}

fn initial_hash() -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"COGITATOR_WITNESS_CHAIN");
    *hasher.finalize().as_bytes()
}

fn chained_hash(previous: &[u8; 32], payload: &serde_json::Value) -> Result<[u8; 32]> {
    let mut hasher = Hasher::new();
    hasher.update(previous);
    let bytes = canonical_json::to_vec(payload).context("serialize hash payload")?;
    hasher.update(&bytes);
    Ok(*hasher.finalize().as_bytes())
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}

pub fn artifact_hashes(paths: &[&Path]) -> Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();
    for path in paths {
        let hash = hash_file(path)?;
        hashes.insert(path.display().to_string(), hash);
    }
    Ok(hashes)
}

pub fn bundle_hash(artifacts: &BTreeMap<String, String>) -> Result<String> {
    bundle_hash_from_map(artifacts)
}

fn bundle_hash_from_map(artifacts: &BTreeMap<String, String>) -> Result<String> {
    let bytes = canonical_json::to_vec(artifacts)?;
    let mut hasher = Hasher::new();
    hasher.update(&bytes);
    Ok(hex_string(hasher.finalize().as_bytes()))
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read artifact {}", path.display()))?;
    let mut hasher = Hasher::new();
    hasher.update(&bytes);
    Ok(hex_string(hasher.finalize().as_bytes()))
}

fn compute_agent_witness_root(
    manifest: &WitnessManifest,
    agent_trace: &[AgentTraceEntry],
    tool_transcript: &ToolTranscriptRecord,
) -> Result<String> {
    let meta_file = std::fs::File::open(&manifest.meta_json)
        .with_context(|| "failed to open meta.json")?;
    let metadata: RunMetadata =
        serde_json::from_reader(meta_file).with_context(|| "failed to parse meta.json")?;

    let metadata_bytes = trace::encode_witnessed_metadata(&metadata.witnessed)?;
    let mut witness = witness::Witness::new(&metadata_bytes)?;

    for entry in agent_trace {
        let entry_bytes = trace::encode_agent_trace_entry(entry)?;
        witness.update(&entry_bytes)?;
        for call in tool_transcript
            .entries
            .iter()
            .filter(|call| call.step == entry.step)
        {
            let call_bytes = trace::encode_tool_call(call)?;
            witness.update(&call_bytes)?;
        }
    }

    Ok(witness.finalize_hex())
}
