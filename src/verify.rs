use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::agent::AgentTraceEntry;
use crate::model::{RunMetadata, TraceEvent, WitnessManifest, TRACE_SCHEMA_VERSION};
use crate::tooling::ToolTranscriptRecord;
use crate::trace::{
    compute_agent_witness_root, encode_agent_trace_entry, encode_event, encode_tool_call_witness,
    encode_witnessed_metadata, index_tool_calls_by_step,
};
use crate::witness::Witness;

#[derive(Debug, Clone)]
pub struct WitnessRootRecomputeReceipt {
    pub expected: String,
    pub computed: String,
    pub matched: bool,
    pub differing_component: Option<String>,
}

fn preview_80(s: &str) -> String {
    const N: usize = 80;
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= N {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn hint_for_line(trimmed: &str) -> Option<&'static str> {
    if trimmed.starts_with('[') {
        return Some(
            "This looks like a JSON array (e.g., agent_trace.json). verify expects NDJSON: one TraceEvent JSON object per line (trace.jsonl).",
        );
    }

    if trimmed.starts_with('{')
        && (trimmed.contains("\"action\"")
            || trimmed.contains("\"role\"")
            || trimmed.contains("\"tool_requests\"")
            || trimmed.contains("\"is_final\""))
    {
        return Some(
            "This looks like an AgentTraceEntry (agent_trace.json), not a TraceEvent. Point verify at trace.jsonl (NDJSON) from `run`, or emit trace.jsonl for demos.",
        );
    }

    None
}

pub fn verify(meta_path: &Path, trace_path: &Path, expect: &str) -> Result<String> {
    let expect = expect.trim();

    let metadata: RunMetadata = crate::strict_json::from_path(meta_path, "meta.json")?;

    if metadata.witnessed.schema_version != TRACE_SCHEMA_VERSION {
        anyhow::bail!(
            "schema version mismatch: expected {}, got {}",
            TRACE_SCHEMA_VERSION,
            metadata.witnessed.schema_version
        );
    }

    let metadata_bytes = encode_witnessed_metadata(&metadata.witnessed)?;
    let mut witness = Witness::new(&metadata_bytes)?;

    let trace_file = File::open(trace_path).with_context(|| "failed to open trace.jsonl")?;
    let reader = BufReader::new(trace_file);

    let mut last_key: Option<(u32, u32)> = None;

    for (line_idx, line) in reader.lines().enumerate() {
        let line = line.with_context(|| "failed to read trace line")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let event: TraceEvent =
            match crate::strict_json::from_slice(trimmed.as_bytes(), "trace.jsonl line") {
                Ok(ev) => ev,
                Err(e) => {
                    let pv = preview_80(trimmed);
                    if let Some(hint) = hint_for_line(trimmed) {
                        anyhow::bail!(
                            "failed to parse trace line {}: {}\n  preview: {}\n  hint: {}",
                            line_idx + 1,
                            e,
                            pv,
                            hint
                        );
                    } else {
                        anyhow::bail!(
                            "failed to parse trace line {}: {}\n  preview: {}",
                            line_idx + 1,
                            e,
                            pv
                        );
                    }
                }
            };

        if event.schema_version != TRACE_SCHEMA_VERSION {
            anyhow::bail!(
                "trace schema version mismatch at line {}: expected {}, got {}",
                line_idx + 1,
                TRACE_SCHEMA_VERSION,
                event.schema_version
            );
        }

        let key = (event.run_id, event.step);
        if let Some(prev) = last_key {
            if key <= prev {
                anyhow::bail!(
                    "trace ordering violation at line {}: ({}, {}) after ({}, {})",
                    line_idx + 1,
                    key.0,
                    key.1,
                    prev.0,
                    prev.1
                );
            }
        }
        last_key = Some(key);

        let event_bytes = encode_event(&event)?;
        witness.update(&event_bytes)?;
    }

    let computed = witness.finalize_hex();
    if computed != expect {
        anyhow::bail!(
            "witness_root mismatch: expected {}, computed {}",
            expect,
            computed
        );
    }

    Ok(computed)
}

pub fn recompute_agent_witness_root_from_bundle(
    witness_dir: &Path,
    expect_override: Option<&str>,
) -> Result<WitnessRootRecomputeReceipt> {
    let manifest_path = witness_dir.join("witness_manifest.json");
    let manifest: WitnessManifest =
        crate::strict_json::from_path(&manifest_path, "witness_manifest.json")?;

    let meta_path = witness_dir.join(&manifest.meta_json);
    let trace_path = witness_dir.join(&manifest.agent_trace_json);
    let transcript_path = witness_dir.join(&manifest.tool_transcript_json);

    let metadata: RunMetadata = crate::strict_json::from_path(&meta_path, "meta.json")?;

    let agent_trace: Vec<AgentTraceEntry> =
        crate::strict_json::from_path(&trace_path, "agent_trace.json")?;

    let transcript: ToolTranscriptRecord =
        crate::strict_json::from_path(&transcript_path, "tool_transcript.json")?;

    let expected = if let Some(expect) = expect_override {
        expect.trim().to_string()
    } else if let Some(path) = manifest.witness_root_txt.as_ref() {
        let root_path = witness_dir.join(path);
        std::fs::read_to_string(&root_path)
            .with_context(|| format!("failed to read {}", root_path.display()))?
            .trim()
            .to_string()
    } else {
        anyhow::bail!(
            "expected witness root missing; provide --expect or witness_root_txt in manifest"
        )
    };

    let computed =
        compute_agent_witness_root(&metadata.witnessed, &agent_trace, &transcript.entries)?;

    let differing_component = if computed == expected {
        None
    } else {
        detect_agent_witness_component_diff(&metadata.witnessed, &agent_trace, &transcript)?
    };

    Ok(WitnessRootRecomputeReceipt {
        matched: computed == expected,
        expected,
        computed,
        differing_component,
    })
}

fn detect_agent_witness_component_diff(
    witnessed: &crate::model::WitnessedMetadata,
    agent_trace: &[AgentTraceEntry],
    transcript: &ToolTranscriptRecord,
) -> Result<Option<String>> {
    let metadata_bytes = encode_witnessed_metadata(witnessed)?;
    let mut calls_by_step = index_tool_calls_by_step(&transcript.entries);
    for calls in calls_by_step.values_mut() {
        calls.sort_by_key(|call| call.tool_call_idx());
    }

    let witness_metadata_only = Witness::new(&metadata_bytes)?;
    let metadata_only = witness_metadata_only.finalize_hex();

    let mut witness_trace_only = Witness::new(&metadata_bytes)?;
    for entry in agent_trace {
        witness_trace_only.update(&encode_agent_trace_entry(entry)?)?;
    }
    let trace_only = witness_trace_only.finalize_hex();

    let mut witness_full = Witness::new(&metadata_bytes)?;
    for entry in agent_trace {
        witness_full.update(&encode_agent_trace_entry(entry)?)?;
        if let Some(calls) = calls_by_step.get_mut(&entry.step) {
            for call in calls.iter() {
                witness_full.update(&encode_tool_call_witness(call)?)?;
            }
        }
    }
    let full = witness_full.finalize_hex();

    Ok(Some(format!(
        "semantic recompute mismatch; metadata_only={} trace_only={} full_semantic={} (full root is metadata+agent_trace+tool_calls)",
        metadata_only, trace_only, full
    )))
}
