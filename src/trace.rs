use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

use crate::agent::AgentTraceEntry;
use crate::canonical_json;
use crate::chaos::FaultRecord;
use crate::model::{RunMetadata, TraceEvent, WitnessedMetadata};
use crate::tooling::{ToolCall, ToolRequest};
use crate::witness;

#[allow(dead_code)]
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

pub fn tool_call_witness_value_canonical(call: &ToolCall) -> Result<serde_json::Value> {
    let witness_call = ToolCallWitness::from(call);
    let value = serde_json::to_value(&witness_call)?;
    debug_assert_no_floats(&value);
    canonical_json::to_value(&witness_call)
}

fn to_canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value)?;
    debug_assert_no_floats(&value);
    canonical_json::to_vec(&value)
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

pub fn compute_agent_witness_root(
    metadata: &WitnessedMetadata,
    agent_trace: &[AgentTraceEntry],
    tool_calls: &[ToolCall],
) -> Result<String> {
    let metadata_bytes = encode_witnessed_metadata(metadata)?;
    let mut witness = witness::Witness::new(&metadata_bytes)?;
    let mut calls_by_step = index_tool_calls_by_step(tool_calls);
    for calls in calls_by_step.values_mut() {
        calls.sort_by_key(|call| call.tool_call_idx);
    }

    for entry in agent_trace {
        let entry_bytes = encode_agent_trace_entry(entry)?;
        witness.update(&entry_bytes)?;
        if let Some(calls) = calls_by_step.get_mut(&entry.step) {
            for call in calls.iter() {
                let call_bytes = encode_tool_call(call)?;
                witness.update(&call_bytes)?;
            }
        }
    }

    Ok(witness.finalize_hex())
}

pub fn index_tool_calls_by_step(tool_calls: &[ToolCall]) -> HashMap<u32, Vec<&ToolCall>> {
    let mut map: HashMap<u32, Vec<&ToolCall>> = HashMap::new();
    for call in tool_calls {
        map.entry(call.step).or_default().push(call);
    }
    map
}

fn debug_assert_no_floats(_value: &serde_json::Value) {
    #[cfg(debug_assertions)]
    {
        if contains_float(_value) {
            panic!("witnessed artifact contains floating-point number");
        }
    }
}

#[cfg(debug_assertions)]
fn contains_float(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Number(num) => num.is_f64(),
        serde_json::Value::Array(values) => values.iter().any(contains_float),
        serde_json::Value::Object(map) => map.values().any(contains_float),
        _ => false,
    }
}
