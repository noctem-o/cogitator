use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

use crate::agent::AgentTraceEntry;
use crate::canonical_json;
use crate::model::{RunMetadata, TraceEvent, WitnessedMetadata};
use crate::tooling::{ToolCall, ToolOutcome, ToolRequest, TranscriptFault};
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
    let witness_entry = AgentTraceEntryWitness::from(entry);
    to_canonical_json(&witness_entry)
}

pub fn encode_tool_call(call: &ToolCall) -> Result<Vec<u8>> {
    let witness_call = ToolCallWitnessView::from(call);
    to_canonical_json(&witness_call)
}

#[allow(dead_code)]
pub fn tool_call_witness_value_canonical(call: &ToolCall) -> Result<serde_json::Value> {
    let witness_call = ToolCallWitnessView::from(call);
    canonical_json::to_value(&witness_call)
}

fn to_canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    canonical_json::to_vec(value)
}

fn normalize_ordeal_tool_name_for_witness(tool_name: &str) -> String {
    if let Some(suffix) = tool_name.strip_prefix("ordeal.") {
        format!("gauntlet.{}", suffix)
    } else {
        tool_name.to_string()
    }
}

fn is_ordealish_entry(entry: &AgentTraceEntry) -> bool {
    if entry
        .tool_requests
        .iter()
        .any(|r| r.tool_name.starts_with("ordeal.") || r.tool_name.starts_with("gauntlet."))
    {
        return true;
    }

    let haystacks = [&entry.thought, &entry.action];
    haystacks.iter().any(|s| {
        s.contains("Ordeal")
            || s.contains("Gauntlet")
            || s.contains("ordeal")
            || s.contains("gauntlet")
    })
}

fn normalize_ordeal_text_for_witness(text: &str) -> String {
    text.replace("Ordeal", "Gauntlet")
        .replace("ordeal", "gauntlet")
}

#[derive(Serialize)]
struct AgentTraceEntryWitness {
    step: u32,
    role: String,
    thought: String,
    action: String,
    tool_requests: Vec<ToolRequest>,
    is_final: bool,
}

impl From<&AgentTraceEntry> for AgentTraceEntryWitness {
    fn from(entry: &AgentTraceEntry) -> Self {
        let ordealish = is_ordealish_entry(entry);
        let thought = if ordealish {
            normalize_ordeal_text_for_witness(&entry.thought)
        } else {
            entry.thought.clone()
        };
        let action = if ordealish {
            normalize_ordeal_text_for_witness(&entry.action)
        } else {
            entry.action.clone()
        };

        let tool_requests = entry
            .tool_requests
            .iter()
            .map(|req| ToolRequest {
                tool_name: normalize_ordeal_tool_name_for_witness(&req.tool_name),
                arguments: req.arguments.clone(),
            })
            .collect();

        Self {
            step: entry.step,
            role: entry.role.clone(),
            thought,
            action,
            tool_requests,
            is_final: entry.is_final,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallOutcomeWitnessView {
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<crate::tooling::ToolError>,
}

impl From<&ToolOutcome> for ToolCallOutcomeWitnessView {
    fn from(outcome: &ToolOutcome) -> Self {
        match outcome {
            ToolOutcome::Ok { output, .. } => Self {
                output: Some(output.clone()),
                error: None,
            },
            ToolOutcome::Err { error, .. } => Self {
                output: None,
                error: Some(error.clone()),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallWitnessView {
    step: u32,
    tool_call_idx: u32,
    tool_name: String,
    request: serde_json::Value,
    outcome: ToolCallOutcomeWitnessView,
    #[serde(skip_serializing_if = "Option::is_none")]
    fault: Option<TranscriptFaultWitnessView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TranscriptFaultWitnessView {
    Timeout { domain: String },
    Drop { domain: String },
    Corrupt { domain: String },
    LatencySim { domain: String },
}

impl From<&TranscriptFault> for TranscriptFaultWitnessView {
    fn from(value: &TranscriptFault) -> Self {
        match value {
            TranscriptFault::Timeout { domain, .. } => Self::Timeout {
                domain: domain.clone(),
            },
            TranscriptFault::Drop { domain } => Self::Drop {
                domain: domain.clone(),
            },
            TranscriptFault::Corrupt { domain, .. } => Self::Corrupt {
                domain: domain.clone(),
            },
            TranscriptFault::LatencySim { domain, .. } => Self::LatencySim {
                domain: domain.clone(),
            },
        }
    }
}

impl From<&ToolCall> for ToolCallWitnessView {
    fn from(call: &ToolCall) -> Self {
        Self {
            step: call.step,
            tool_call_idx: call.tool_call_idx,
            tool_name: normalize_ordeal_tool_name_for_witness(&call.tool_name),
            request: call.request.clone(),
            outcome: ToolCallOutcomeWitnessView::from(&call.outcome),
            fault: call.fault.as_ref().map(TranscriptFaultWitnessView::from),
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
