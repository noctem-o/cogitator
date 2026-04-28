use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

use crate::agent::AgentTraceEntry;
use crate::canonical_json;
use crate::model::{RunMetadata, TraceEvent, WitnessedMetadata};
use crate::tooling::{PhantomEntry, ToolCall, ToolOutcome, ToolRequest, TranscriptFault};
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

pub fn encode_tool_call_witness(call: &ToolCallWitnessView) -> Result<Vec<u8>> {
    to_canonical_json(call)
}

pub fn encode_phantom_entry_witness(entry: &PhantomEntryWitnessView) -> Result<Vec<u8>> {
    to_canonical_json(entry)
}

#[allow(dead_code)]
pub fn tool_call_witness_value_canonical(call: &ToolCall) -> Result<serde_json::Value> {
    canonical_json::to_value(&ToolCallWitnessView::from(call))
}

fn to_canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    canonical_json::to_vec(value)
}

fn normalize_ordeal_tool_name_for_witness(tool_name: &str) -> String {
    tool_name.to_string()
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
        let thought = entry.thought.clone();
        let action = entry.action.clone();

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

impl ToolCallWitnessView {
    pub fn tool_call_idx(&self) -> u32 {
        self.tool_call_idx
    }
}

pub fn tool_call_witness_view(call: &ToolCall) -> ToolCallWitnessView {
    ToolCallWitnessView::from(call)
}

#[derive(Debug, Clone, Serialize)]
pub struct PhantomEntryWitnessView {
    step: u32,
    tool_call_idx: u32,
    tool_name: String,
    request: serde_json::Value,
    disposition: crate::policy::PhantomDisposition,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule_id: Option<String>,
    reason: String,
}

impl PhantomEntryWitnessView {
    pub fn tool_call_idx(&self) -> u32 {
        self.tool_call_idx
    }
}

impl From<&PhantomEntry> for PhantomEntryWitnessView {
    fn from(entry: &PhantomEntry) -> Self {
        Self {
            step: entry.step,
            tool_call_idx: entry.tool_call_idx,
            tool_name: normalize_ordeal_tool_name_for_witness(&entry.tool_name),
            request: entry.request.clone(),
            disposition: entry.disposition.clone(),
            rule_id: entry.rule_id.clone(),
            reason: entry.reason.clone(),
        }
    }
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
    phantom_entries: &[PhantomEntry],
) -> Result<String> {
    let metadata_bytes = encode_witnessed_metadata(metadata)?;
    let mut witness = witness::Witness::new(&metadata_bytes)?;
    let mut ops_by_step = index_tool_ops_by_step(tool_calls, phantom_entries);
    for ops in ops_by_step.values_mut() {
        ops.sort_by_key(|op| op.tool_call_idx());
    }

    for entry in agent_trace {
        let entry_bytes = encode_agent_trace_entry(entry)?;
        witness.update(&entry_bytes)?;
        if let Some(ops) = ops_by_step.get(&entry.step) {
            for op in ops {
                match op {
                    ToolOpWitnessView::Executed(call) => {
                        let call_bytes = encode_tool_call_witness(call)?;
                        witness.update(&call_bytes)?;
                    }
                    ToolOpWitnessView::Intercepted(phantom) => {
                        let phantom_bytes = encode_phantom_entry_witness(phantom)?;
                        witness.update(&phantom_bytes)?;
                    }
                }
            }
        }
    }

    Ok(witness.finalize_hex())
}

pub fn index_tool_calls_by_step(tool_calls: &[ToolCall]) -> HashMap<u32, Vec<ToolCallWitnessView>> {
    let mut map: HashMap<u32, Vec<ToolCallWitnessView>> = HashMap::new();
    for call in tool_calls {
        map.entry(call.step)
            .or_default()
            .push(tool_call_witness_view(call));
    }
    map
}

pub enum ToolOpWitnessView {
    Executed(ToolCallWitnessView),
    Intercepted(PhantomEntryWitnessView),
}

impl ToolOpWitnessView {
    pub fn tool_call_idx(&self) -> u32 {
        match self {
            ToolOpWitnessView::Executed(call) => call.tool_call_idx(),
            ToolOpWitnessView::Intercepted(entry) => entry.tool_call_idx(),
        }
    }
}

pub fn index_tool_ops_by_step(
    tool_calls: &[ToolCall],
    phantom_entries: &[PhantomEntry],
) -> HashMap<u32, Vec<ToolOpWitnessView>> {
    let mut map: HashMap<u32, Vec<ToolOpWitnessView>> = HashMap::new();
    for call in tool_calls {
        map.entry(call.step)
            .or_default()
            .push(ToolOpWitnessView::Executed(tool_call_witness_view(call)));
    }
    for entry in phantom_entries {
        map.entry(entry.step)
            .or_default()
            .push(ToolOpWitnessView::Intercepted(
                PhantomEntryWitnessView::from(entry),
            ));
    }
    map
}
