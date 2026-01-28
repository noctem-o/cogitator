use anyhow::{Context, Result};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::agent::AgentTraceEntry;
use crate::model::{WitnessArtifact, WitnessManifest};
use crate::tooling::{ToolCall, ToolMismatch, ToolMismatchKind, ToolTranscriptRecord};

pub const DRIFT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftReport {
    pub schema_version: u32,
    pub drifted: bool,
    pub issues: Vec<DriftIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftIssueKind {
    OrderingMismatch,
    RequestMismatch,
    ResponseMismatch,
    OutputHashMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftIssue {
    pub index: usize,
    pub step: Option<u32>,
    pub kind: DriftIssueKind,
    pub message: String,
    pub expected: Option<serde_json::Value>,
    pub actual: Option<serde_json::Value>,
}

pub fn detect_transcript_drift(
    expected: &ToolTranscriptRecord,
    actual: &ToolTranscriptRecord,
) -> DriftReport {
    let mut issues = Vec::new();

    if expected.entries.len() != actual.entries.len() {
        issues.push(DriftIssue {
            index: 0,
            step: None,
            kind: DriftIssueKind::OrderingMismatch,
            message: format!(
                "tool call count mismatch: expected {}, got {}",
                expected.entries.len(),
                actual.entries.len()
            ),
            expected: Some(serde_json::json!({ "count": expected.entries.len() })),
            actual: Some(serde_json::json!({ "count": actual.entries.len() })),
        });
    }

    let max_len = expected.entries.len().max(actual.entries.len());
    for index in 0..max_len {
        let exp = expected.entries.get(index);
        let act = actual.entries.get(index);

        match (exp, act) {
            (Some(exp), Some(act)) => {
                if exp.index != act.index {
                    issues.push(DriftIssue {
                        index,
                        step: Some(act.step),
                        kind: DriftIssueKind::OrderingMismatch,
                        message: format!(
                            "tool call ordering mismatch at {}: expected index {}, got {}",
                            index, exp.index, act.index
                        ),
                        expected: Some(serde_json::json!({ "index": exp.index })),
                        actual: Some(serde_json::json!({ "index": act.index })),
                    });
                }
                if exp.step != act.step {
                    issues.push(DriftIssue {
                        index,
                        step: Some(act.step),
                        kind: DriftIssueKind::OrderingMismatch,
                        message: format!(
                            "tool call step mismatch at {}: expected {}, got {}",
                            index, exp.step, act.step
                        ),
                        expected: Some(serde_json::json!({ "step": exp.step })),
                        actual: Some(serde_json::json!({ "step": act.step })),
                    });
                }
                if exp.request != act.request {
                    issues.push(DriftIssue {
                        index,
                        step: Some(act.step),
                        kind: DriftIssueKind::RequestMismatch,
                        message: format!("tool request mismatch at {}", index),
                        expected: Some(serde_json::to_value(&exp.request).unwrap_or_default()),
                        actual: Some(serde_json::to_value(&act.request).unwrap_or_default()),
                    });
                }
                if exp.response != act.response {
                    issues.push(DriftIssue {
                        index,
                        step: Some(act.step),
                        kind: DriftIssueKind::ResponseMismatch,
                        message: format!("tool response mismatch at {}", index),
                        expected: Some(serde_json::to_value(&exp.response).unwrap_or_default()),
                        actual: Some(serde_json::to_value(&act.response).unwrap_or_default()),
                    });
                }
                let exp_hash = response_hash(&exp.response);
                let act_hash = response_hash(&act.response);
                if exp_hash != act_hash {
                    issues.push(DriftIssue {
                        index,
                        step: Some(act.step),
                        kind: DriftIssueKind::OutputHashMismatch,
                        message: format!("tool response hash mismatch at {}", index),
                        expected: Some(serde_json::json!({ "hash": exp_hash })),
                        actual: Some(serde_json::json!({ "hash": act_hash })),
                    });
                }
            }
            (Some(exp), None) => {
                issues.push(DriftIssue {
                    index,
                    step: Some(exp.step),
                    kind: DriftIssueKind::OrderingMismatch,
                    message: format!("missing tool call at {}", index),
                    expected: Some(serde_json::to_value(exp).unwrap_or_default()),
                    actual: None,
                });
            }
            (None, Some(act)) => {
                issues.push(DriftIssue {
                    index,
                    step: Some(act.step),
                    kind: DriftIssueKind::OrderingMismatch,
                    message: format!("unexpected tool call at {}", index),
                    expected: None,
                    actual: Some(serde_json::to_value(act).unwrap_or_default()),
                });
            }
            (None, None) => {}
        }
    }

    DriftReport {
        schema_version: DRIFT_SCHEMA_VERSION,
        drifted: !issues.is_empty(),
        issues,
    }
}

pub fn issue_from_mismatch(mismatch: &ToolMismatch) -> DriftIssue {
    DriftIssue {
        index: mismatch.index,
        step: mismatch.step,
        kind: match mismatch.kind {
            ToolMismatchKind::OrderingMismatch => DriftIssueKind::OrderingMismatch,
            ToolMismatchKind::RequestMismatch => DriftIssueKind::RequestMismatch,
            ToolMismatchKind::ResponseMismatch => DriftIssueKind::ResponseMismatch,
            ToolMismatchKind::OutputHashMismatch => DriftIssueKind::OutputHashMismatch,
        },
        message: mismatch.message.clone(),
        expected: mismatch.expected.clone(),
        actual: mismatch.actual.clone(),
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
                "payload": call,
            });
            current = chained_hash(&current, &payload)?;
            chain.push(hex_string(&current));
        }
    }

    Ok(chain)
}

pub fn verify_witness_bundle(dir: &Path) -> Result<()> {
    let manifest_path = dir.join("witness_manifest.json");
    let manifest_file = std::fs::File::open(&manifest_path)
        .with_context(|| "failed to open witness_manifest.json")?;
    let manifest: WitnessManifest = serde_json::from_reader(manifest_file)
        .with_context(|| "failed to parse witness_manifest.json")?;

    for artifact in &manifest.artifacts {
        let actual = hash_file(Path::new(&artifact.path))?;
        if actual != artifact.blake3 {
            anyhow::bail!(
                "artifact hash mismatch for {}: expected {}, got {}",
                artifact.name,
                artifact.blake3,
                actual
            );
        }
    }
    let computed_bundle = bundle_hash(&manifest.artifacts)?;
    if computed_bundle != manifest.bundle_hash {
        anyhow::bail!(
            "bundle hash mismatch: expected {}, got {}",
            manifest.bundle_hash,
            computed_bundle
        );
    }

    let agent_trace_path = artifact_path(&manifest, "agent_trace.json")?;
    let agent_trace_file = std::fs::File::open(&agent_trace_path)
        .with_context(|| "failed to open agent_trace.json")?;
    let agent_trace: Vec<AgentTraceEntry> = serde_json::from_reader(agent_trace_file)
        .with_context(|| "failed to parse agent_trace.json")?;

    let tool_path = artifact_path(&manifest, "tool_transcript.json")?;
    let tool_file =
        std::fs::File::open(&tool_path).with_context(|| "failed to open tool_transcript.json")?;
    let tool_transcript: ToolTranscriptRecord = serde_json::from_reader(tool_file)
        .with_context(|| "failed to parse tool_transcript.json")?;

    let chain = build_hash_chain(&agent_trace, &tool_transcript.entries)?;
    let hash_chain_path = artifact_path(&manifest, "hash_chain.txt")?;
    let expected_chain = std::fs::read_to_string(&hash_chain_path)
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

    Ok(())
}

pub fn artifact_path(manifest: &WitnessManifest, name: &str) -> Result<String> {
    manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.name == name)
        .map(|artifact| artifact.path.clone())
        .ok_or_else(|| anyhow::anyhow!("artifact {} not found in manifest", name))
}

pub fn hash_file(path: &Path) -> Result<String> {
    let data = std::fs::read(path).with_context(|| "failed to read artifact for hash")?;
    Ok(blake3::hash(&data).to_hex().to_string())
}

pub fn bundle_hash(artifacts: &[WitnessArtifact]) -> Result<String> {
    let mut hasher = Hasher::new();
    for artifact in artifacts {
        hasher.update(artifact.name.as_bytes());
        hasher.update(artifact.path.as_bytes());
        hasher.update(artifact.blake3.as_bytes());
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn response_hash(response: &crate::tooling::ToolResponse) -> String {
    let payload = serde_json::json!({
        "tool_name": response.tool_name,
        "output": response.output,
        "success": response.success,
    });
    let mut hasher = Hasher::new();
    if let Ok(bytes) = serde_json::to_vec(&payload) {
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
    let bytes = serde_json::to_vec(payload).context("serialize hash payload")?;
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
