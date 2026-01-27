use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::model::{RunMetadata, TraceEvent, TRACE_SCHEMA_VERSION};
use crate::trace::{encode_event, encode_witnessed_metadata};
use crate::witness::Witness;

pub fn verify(meta_path: &Path, trace_path: &Path, expect: &str) -> Result<String> {
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
        if line.trim().is_empty() {
            continue;
        }
        let event: TraceEvent = serde_json::from_str(&line)
            .with_context(|| format!("failed to parse trace line {}", line_idx + 1))?;
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
