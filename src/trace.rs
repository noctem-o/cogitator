use anyhow::Result;
use serde::Serialize;

use crate::model::{RunMetadata, TraceEvent, WitnessedMetadata};

pub fn encode_metadata(metadata: &RunMetadata) -> Result<Vec<u8>> {
    to_canonical_json(metadata)
}

pub fn encode_witnessed_metadata(metadata: &WitnessedMetadata) -> Result<Vec<u8>> {
    to_canonical_json(metadata)
}

pub fn encode_event(event: &TraceEvent) -> Result<Vec<u8>> {
    to_canonical_json(event)
}

fn to_canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let formatter = serde_json::ser::CompactFormatter;
    let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
    value.serialize(&mut serializer)?;
    Ok(buffer)
}
