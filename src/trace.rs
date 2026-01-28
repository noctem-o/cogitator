use anyhow::Result;
use serde::Serialize;

use crate::agent::AgentTraceEntry;
use crate::canonical_json;
use crate::model::{RunMetadata, TraceEvent, WitnessedMetadata};
use crate::tooling::{ToolCall, ToolRequest};
use crate::chaos::FaultRecord;

pub fn encode_metadata(metadata: &RunMetadata) -> Result<Vec<u8>> {
    to_canonical_json(metadata)
}

pub fn encode_witnessed_metadata(metadata: &WitnessedMetadata) -> Result<Vec<u8>> {
    to_canonical_json(metadata)
}

pub fn encode_event(event: &TraceEvent) -> Result<Vec<u8>> {
    to_canonical_json(event)
}

pub fn encode_agent_trace_entry(entry: &AgentTraceEntry) -> Result<Vec<u8>> {
    to_canonical_json(entry)
}

pub fn encode_tool_call(call: &ToolCall) -> Result<Vec<u8>> {
    let witness_call = ToolCallWitness::from(call);
    to_canonical_json(&witness_call)
}

pub fn tool_call_witness_value(call: &ToolCall) -> Result<serde_json::Value> {
    let witness_call = ToolCallWitness::from(call);
    Ok(serde_json::to_value(witness_call)?)
}

fn to_canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    canonical_json::to_vec(value)
}

#[derive(Serialize)]
struct ToolResponseWitness {
    tool_name: String,
    output: serde_json::Value,
    success: bool,
}

#[derive(Serialize)]
struct ToolCallWitness {
    step: u32,
    tool_call_idx: u32,
    request: ToolRequest,
    response: ToolResponseWitness,
    fault: Option<FaultRecord>,
}

impl From<&ToolCall> for ToolCallWitness {
    fn from(call: &ToolCall) -> Self {
        Self {
            step: call.step,
            tool_call_idx: call.tool_call_idx,
            request: call.request.clone(),
            response: ToolResponseWitness {
                tool_name: call.response.tool_name.clone(),
                output: call.response.output.clone(),
                success: call.response.success,
            },
            fault: call.fault.clone(),
        }
    }
}
