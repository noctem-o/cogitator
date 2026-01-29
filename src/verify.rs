use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::model::{RunMetadata, TraceEvent, TRACE_SCHEMA_VERSION};
use crate::trace::{encode_event, encode_witnessed_metadata};
use crate::witness::Witness;

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
    // Common user mistake: passing agent_trace.json (JSON array) or AgentTraceEntry objects.
    if trimmed.starts_with('[') {
        return Some(
            "This looks like a JSON array (e.g., agent_trace.json). \
verify expects NDJSON: one TraceEvent JSON object per line (trace.jsonl).",
        );
    }

    if trimmed.starts_with('{')
        && (trimmed.contains("\"action\"")
            || trimmed.contains("\"role\"")
            || trimmed.contains("\"tool_requests\"")
            || trimmed.contains("\"is_final\""))
    {
        return Some(
            "This looks like an AgentTraceEntry (agent_trace.json), not a TraceEvent. \
Point verify at trace.jsonl (NDJSON) from `run`, or emit trace.jsonl for demos.",
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
