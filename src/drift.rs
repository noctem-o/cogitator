use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::agent::AgentTraceEntry;
use crate::canonical_json;
use crate::report::DriftIssue;
use crate::tooling::{ToolCall, ToolTranscriptRecord};

pub const DRIFT_SCHEMA_VERSION: u32 = 1;
pub const VERIFY_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftReport {
    pub schema_version: u32,
    pub drifted: bool,
    pub issues: Vec<DriftIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyReport {
    pub schema_version: u32,
    pub verified: bool,
    pub issues: Vec<String>,
    pub bundle_hash_expected: String,
    pub bundle_hash_actual: String,
}

/// Compare two tool transcripts and detect drift.
///
/// NOTE: `expected` is borrowed so callers can pass `.as_ref()` results without cloning.
pub fn detect_transcript_drift(
    expected: &ToolTranscriptRecord,
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
            issues.push(DriftIssue::ToolRequestMismatch { index: index as u32 });
        }

        if exp.outcome != act.outcome {
            issues.push(DriftIssue::ToolOutcomeMismatch { index: index as u32 });
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
pub fn artifact_hashes(paths: &[&Path]) -> Result<BTreeMap<String, String>> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();

    for path in paths {
        let content = std::fs::read(path)
            .with_context(|| format!("failed to read artifact: {}", path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let hash = crate::hex::encode(&hasher.finalize());

        let key = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        if out.insert(key.clone(), hash).is_some() {
            anyhow::bail!("duplicate artifact key in bundle: {}", key);
        }
    }

    Ok(out)
}

/// Compute bundle hash from artifact hashes
pub fn bundle_hash(artifact_hashes: &BTreeMap<String, String>) -> Result<String> {
    let mut hasher = Sha256::new();
    for (name, hash) in artifact_hashes {
        // Include names so a swap/reorder can't preserve the bundle hash.
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }
    Ok(crate::hex::encode(&hasher.finalize()))
}

/// Best-effort resolver for a manifest path string.
///
/// The manifest may contain:
/// - absolute paths
/// - paths relative to CWD
/// - paths that redundantly include `witness_dir` (e.g. `out/run_0001/meta.json`
///   while `witness_dir` is `out/run_0001`)
///
/// For verification, we primarily want to locate the artifact within `witness_dir`.
fn resolve_manifest_artifact_path(witness_dir: &Path, raw: &str, key_name: &str) -> PathBuf {
    let raw_path = PathBuf::from(raw);

    // 1) Absolute path: use as-is.
    if raw_path.is_absolute() {
        return raw_path;
    }

    // 2) Preferred: `witness_dir/<file_name>`
    if let Some(fname) = raw_path.file_name() {
        let candidate = witness_dir.join(fname);
        if candidate.exists() {
            return candidate;
        }
    }

    // 3) If raw_path already starts with witness_dir (both relative), strip it.
    if let Ok(stripped) = raw_path.strip_prefix(witness_dir) {
        return witness_dir.join(stripped);
    }

    // 4) Next: `witness_dir/<raw>`
    let candidate = witness_dir.join(&raw_path);
    if candidate.exists() {
        return candidate;
    }

    // 5) Fall back to `witness_dir/<key_name>` (e.g. "meta.json")
    witness_dir.join(key_name)
}

/// Verify witness bundle integrity (hashes + bundle hash) and emit verify_report.json
pub fn verify_witness_bundle(witness_dir: &Path) -> Result<VerifyReport> {
    let manifest_path = witness_dir.join("witness_manifest.json");

    if !manifest_path.exists() {
        let report = VerifyReport {
            schema_version: VERIFY_REPORT_SCHEMA_VERSION,
            verified: false,
            issues: vec!["witness_manifest.json not found".to_string()],
            bundle_hash_expected: String::new(),
            bundle_hash_actual: String::new(),
        };
        let report_path = witness_dir.join("verify_report.json");
        let _ = canonical_json::write_json(&report_path, &report, "verify_report.json");
        return Ok(report);
    }

    let manifest_file =
        std::fs::File::open(&manifest_path).context("failed to open witness_manifest.json")?;
    let manifest: crate::model::WitnessManifest =
        serde_json::from_reader(manifest_file).context("failed to parse witness_manifest.json")?;

    let mut issues: Vec<String> = Vec::new();

    // Compute actual hashes for exactly the keys the manifest claims.
    let mut actual_hashes: BTreeMap<String, String> = BTreeMap::new();

    for (name, expected_hash) in manifest.artifact_hashes.iter() {
        // Map known keys to their manifest path fields (for backwards compatibility).
        let raw_hint = match name.as_str() {
            "meta.json" => Some(manifest.meta_json.as_str()),
            "agent_trace.json" => Some(manifest.agent_trace_json.as_str()),
            "tool_transcript.json" => Some(manifest.tool_transcript_json.as_str()),
            "drift_report.json" => Some(manifest.drift_report_json.as_str()),
            "hash_chain.txt" => Some(manifest.hash_chain_txt.as_str()),
            "chaos_profile.json" => manifest.chaos_profile_json.as_deref(),
            "witness_root.txt" => manifest.witness_root_txt.as_deref(),
            "nix_provenance.json" => manifest.nix_provenance_json.as_deref(),
            _ => None,
        };

        let path = if let Some(raw) = raw_hint {
            resolve_manifest_artifact_path(witness_dir, raw, name)
        } else {
            witness_dir.join(name)
        };

        if !path.exists() {
            issues.push(format!("{} missing (looked for {})", name, path.display()));
            continue;
        }

        let content = std::fs::read(&path).with_context(|| format!("failed to read {}", name))?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let actual = crate::hex::encode(&hasher.finalize());

        if &actual != expected_hash {
            issues.push(format!(
                "{} hash mismatch: expected {}, computed {}",
                name, expected_hash, actual
            ));
        }

        actual_hashes.insert(name.clone(), actual);
    }

    // Verify bundle hash.
    let computed_bundle = bundle_hash(&actual_hashes)?;
    if computed_bundle != manifest.bundle_hash {
        issues.push(format!(
            "bundle hash mismatch: expected {}, computed {}",
            manifest.bundle_hash, computed_bundle
        ));
    }

    let report = VerifyReport {
        schema_version: VERIFY_REPORT_SCHEMA_VERSION,
        verified: issues.is_empty(),
        issues,
        bundle_hash_expected: manifest.bundle_hash.clone(),
        bundle_hash_actual: computed_bundle,
    };

    let report_path = witness_dir.join("verify_report.json");
    canonical_json::write_json(&report_path, &report, "verify_report.json")?;

    Ok(report)
}
