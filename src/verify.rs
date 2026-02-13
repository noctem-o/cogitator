use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::agent::AgentTraceEntry;
use crate::model::{RunMetadata, TraceEvent, WitnessManifest, TRACE_SCHEMA_VERSION};
use crate::tooling::ToolTranscriptRecord;
use crate::trace::{compute_agent_witness_root, encode_event, encode_witnessed_metadata};
use crate::witness::Witness;

#[derive(Debug, Clone)]
pub struct WitnessRootRecomputeReceipt {
    pub expected: String,
    pub computed: String,
    pub matched: bool,
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

    let meta_file = File::open(meta_path).with_context(|| "failed to open meta.json")?;
    let metadata: RunMetadata =
        serde_json::from_reader(meta_file).with_context(|| "failed to parse meta.json")?;

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

        let event: TraceEvent = match serde_json::from_str(trimmed) {
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
    let manifest_file = File::open(&manifest_path)
        .with_context(|| format!("failed to open {}", manifest_path.display()))?;
    let manifest: WitnessManifest = serde_json::from_reader(manifest_file)
        .with_context(|| "failed to parse witness_manifest.json")?;

    let meta_path = witness_dir.join(&manifest.meta_json);
    let trace_path = witness_dir.join(&manifest.agent_trace_json);
    let transcript_path = witness_dir.join(&manifest.tool_transcript_json);

    let metadata: RunMetadata = serde_json::from_reader(
        File::open(&meta_path)
            .with_context(|| format!("failed to open {}", meta_path.display()))?,
    )
    .with_context(|| "failed to parse meta.json")?;

    let agent_trace: Vec<AgentTraceEntry> = serde_json::from_reader(
        File::open(&trace_path)
            .with_context(|| format!("failed to open {}", trace_path.display()))?,
    )
    .with_context(|| "failed to parse agent_trace.json")?;

    let transcript: ToolTranscriptRecord = serde_json::from_reader(
        File::open(&transcript_path)
            .with_context(|| format!("failed to open {}", transcript_path.display()))?,
    )
    .with_context(|| "failed to parse tool_transcript.json")?;

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

    Ok(WitnessRootRecomputeReceipt {
        matched: computed == expected,
        expected,
        computed,
    })
}
