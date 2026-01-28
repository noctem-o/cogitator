use serde::{Deserialize, Serialize};

use crate::model::RunMetadata;
use crate::tooling::{ToolRequest, ToolResponse, ToolTranscriptHandle};

pub const AGENT_TRACE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentCaseContext {
    pub run_id: u32,
    pub case_id: String,
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct AgentInput {
    pub case_context: AgentCaseContext,
    pub step: u32,
    pub seed: u64,
    pub run_metadata: RunMetadata,
    pub transcript: ToolTranscriptHandle,
    pub prior_tool_outputs: Vec<ToolResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOutput {
    pub assistant_message: String,
    pub tool_requests: Vec<ToolRequest>,
    pub is_final: bool,
    pub decision: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTraceEntry {
    pub step: u32,
    pub role: String,
    pub assistant_message: String,
    pub tool_requests: Vec<ToolRequest>,
    pub is_final: bool,
    pub decision: Option<serde_json::Value>,
}

pub trait Agent {
    fn step(&mut self, input: AgentInput) -> AgentOutput;
}

#[derive(Debug, Clone)]
pub struct ClawdbotAgent {
    seed: u64,
    variant: ClawdbotVariant,
}

#[derive(Debug, Clone, Copy)]
pub enum ClawdbotVariant {
    Baseline,
    Regressed,
}

impl ClawdbotAgent {
    pub fn new(seed: u64, variant: ClawdbotVariant) -> Self {
        Self { seed, variant }
    }
}

impl Agent for ClawdbotAgent {
    fn step(&mut self, input: AgentInput) -> AgentOutput {
        let step = input.step;
        let case_hint = format!(
            "run={} case={}",
            input.case_context.run_id, input.case_context.case_id
        );
        let schema_hint = format!(
            "trace_schema={}",
            input.run_metadata.witnessed.schema_version
        );
        let prior_count = input.prior_tool_outputs.len();
        if step == 0 {
            let variant_label = match self.variant {
                ClawdbotVariant::Baseline => "baseline",
                ClawdbotVariant::Regressed => "regressed",
            };
            let hint = match self.variant {
                ClawdbotVariant::Baseline => format!("{} {}", case_hint, schema_hint),
                ClawdbotVariant::Regressed => {
                    format!("{} {} variant=regressed", case_hint, schema_hint)
                }
            };
            let tool_request = ToolRequest {
                tool_name: "clawdbot.lookup".to_string(),
                arguments: serde_json::json!({
                    "seed": input.seed,
                    "case": input.case_context.case_id,
                    "hint": hint,
                    "variant": variant_label,
                    "transcript_mode": format!("{:?}", input.transcript.mode).to_lowercase(),
                    "prior_outputs": prior_count,
                }),
            };
            AgentOutput {
                assistant_message: format!(
                    "Scan the case context for deterministic signals ({}).",
                    schema_hint
                ),
                tool_requests: vec![tool_request],
                is_final: false,
                decision: None,
            }
        } else {
            let decision = serde_json::json!({
                "status": "pass",
                "confidence": 0.73,
                "seed": self.seed,
            });
            AgentOutput {
                assistant_message: format!("Finalize assessment for {}", case_hint),
                tool_requests: Vec::new(),
                is_final: true,
                decision: Some(decision),
            }
        }
    }
}

pub fn trace_entry_from_output(step: u32, output: &AgentOutput) -> AgentTraceEntry {
    AgentTraceEntry {
        step,
        role: "assistant".to_string(),
        assistant_message: output.assistant_message.clone(),
        tool_requests: output.tool_requests.clone(),
        is_final: output.is_final,
        decision: output.decision.clone(),
    }
}
