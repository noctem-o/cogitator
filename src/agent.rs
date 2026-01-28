use serde::{Deserialize, Serialize};

use crate::llm;
use crate::tooling::ToolRequest;

#[derive(Debug, Clone)]
pub struct AgentInput {
    pub run_id: u32,
    pub case_id: String,
    pub step: u32,
    pub seed: u64,
    pub prior_tool_outputs: Vec<crate::tooling::ToolResponse>,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub enabled: bool,
    pub model: String,
    pub seed: Option<u64>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "stub".to_string(),
            seed: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOutput {
    pub thought: String,
    pub action: String,
    pub tool_requests: Vec<ToolRequest>,
    pub is_final: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTraceEntry {
    pub step: u32,
    pub role: String,
    pub thought: String,
    pub action: String,
    pub tool_requests: Vec<ToolRequest>,
    pub is_final: bool,
}

pub trait Agent {
    fn step(&mut self, input: AgentInput) -> AgentOutput;
}

#[derive(Debug, Clone)]
pub struct ClawdbotAgent {
    llm: LlmConfig,
}

impl ClawdbotAgent {
    pub fn new(llm: LlmConfig) -> Self {
        Self { llm }
    }
}

impl Agent for ClawdbotAgent {
    fn step(&mut self, input: AgentInput) -> AgentOutput {
        let step = input.step;
        let case_hint = format!("run={} case={}", input.run_id, input.case_id);
        let _ = input.prior_tool_outputs.len();
        if step == 0 {
            let tool_request = ToolRequest {
                tool_name: "clawdbot.lookup".to_string(),
                arguments: serde_json::json!({
                    "seed": input.seed,
                    "case": input.case_id,
                    "hint": case_hint,
                }),
            };
            let mut tool_requests = vec![tool_request];
            if self.llm.enabled {
                let llm_request = llm::LlmRequest {
                    schema_version: llm::LLM_REQUEST_SCHEMA_VERSION,
                    model: self.llm.model.clone(),
                    messages: vec![llm::LlmMessage {
                        role: "user".to_string(),
                        content: format!("Case hint: {}", case_hint),
                    }],
                    temperature: None,
                    max_tokens: None,
                    seed: self.llm.seed,
                };
                if let Ok(request) = llm::make_tool_request(&llm_request) {
                    tool_requests.push(request);
                }
            }
            AgentOutput {
                thought: "Scan the case context for deterministic signals.".to_string(),
                action: "Request structured lookup.".to_string(),
                tool_requests,
                is_final: false,
            }
        } else {
            AgentOutput {
                thought: "Synthesize tool output into a final verdict.".to_string(),
                action: format!("Finalize assessment for {}", case_hint),
                tool_requests: Vec::new(),
                is_final: true,
            }
        }
    }
}

pub fn trace_entry_from_output(step: u32, output: &AgentOutput) -> AgentTraceEntry {
    AgentTraceEntry {
        step,
        role: "assistant".to_string(),
        thought: output.thought.clone(),
        action: output.action.clone(),
        tool_requests: output.tool_requests.clone(),
        is_final: output.is_final,
    }
}
