use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::canonical_json;
use crate::chaos::{apply_fault, ChaosEngine, FaultRecord};
use crate::llm;
use crate::llm::LlmBackend;
use crate::report::DriftIssue;

pub const TOOL_TRANSCRIPT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolRequest {
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolResponse {
    pub tool_name: String,
    pub output: serde_json::Value,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub simulated_latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub step: u32,
    pub tool_call_idx: u32,
    pub request: ToolRequest,
    pub response: ToolResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<FaultRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolMode {
    Live,
    Replay,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolTranscriptRecord {
    pub schema_version: u32,
    pub mode: ToolMode,
    pub entries: Vec<ToolCall>,
}

pub struct ToolTranscript {
    mode: ToolMode,
    expected: Vec<ToolCall>,
    recorded: Vec<ToolCall>,
    cursor: usize,
    mismatches: Vec<DriftIssue>,
    chaos: Option<ChaosEngine>,
    next_call_idx: u32,
}

impl ToolTranscript {
    pub fn new_live(chaos: Option<ChaosEngine>) -> Self {
        Self {
            mode: ToolMode::Live,
            expected: Vec::new(),
            recorded: Vec::new(),
            cursor: 0,
            mismatches: Vec::new(),
            chaos,
            next_call_idx: 0,
        }
    }

    pub fn new_replay(expected: ToolTranscriptRecord) -> Self {
        Self {
            mode: ToolMode::Replay,
            expected: expected.entries,
            recorded: Vec::new(),
            cursor: 0,
            mismatches: Vec::new(),
            chaos: None,
            next_call_idx: 0,
        }
    }

    #[allow(dead_code)]
    pub fn mode(&self) -> ToolMode {
        self.mode.clone()
    }

    pub fn mismatches(&self) -> &[DriftIssue] {
        &self.mismatches
    }

    pub fn execute(&mut self, step: u32, request: ToolRequest) -> ToolResponse {
        let tool_call_idx = self.next_call_idx;
        self.next_call_idx = self.next_call_idx.saturating_add(1);
        match self.mode {
            ToolMode::Live => {
                let mut response = if request.tool_name == llm::LlmRequest::tool_name() {
                    llm_live_response(&request).unwrap_or_else(|| stub_response(&request))
                } else {
                    stub_response(&request)
                };
                let fault = if let Some(chaos) = &self.chaos {
                    chaos.decide_fault(step, tool_call_idx, &request.tool_name)
                } else {
                    None
                };
                if let Some(fault_record) = fault.as_ref() {
                    let original = response.clone();
                    response = apply_fault(&request, response, fault_record).unwrap_or(original);
                }
                self.recorded.push(ToolCall {
                    step,
                    tool_call_idx,
                    request,
                    response: response.clone(),
                    fault,
                });
                response
            }
            ToolMode::Replay => {
                let response = if let Some(expected) = self.expected.get(self.cursor) {
                    if expected.step != step {
                        self.mismatches.push(DriftIssue::ToolStepMismatch {
                            index: self.cursor as u32,
                            expected: expected.step,
                            actual: step,
                        });
                    }
                    if expected.tool_call_idx != tool_call_idx {
                        self.mismatches.push(DriftIssue::ToolCallIndexMismatch {
                            index: self.cursor as u32,
                            expected: expected.tool_call_idx,
                            actual: tool_call_idx,
                        });
                    }
                    if expected.request != request {
                        self.mismatches.push(DriftIssue::ToolRequestMismatch {
                            index: self.cursor as u32,
                        });
                    }
                    expected.response.clone()
                } else {
                    self.mismatches.push(DriftIssue::UnexpectedToolRequest {
                        index: self.cursor as u32,
                    });
                    stub_response(&request)
                };
                self.recorded.push(ToolCall {
                    step,
                    tool_call_idx,
                    request,
                    response: response.clone(),
                    fault: self
                        .expected
                        .get(self.cursor)
                        .and_then(|expected| expected.fault.clone()),
                });
                self.cursor += 1;
                response
            }
        }
    }

    pub fn into_record(self) -> ToolTranscriptRecord {
        ToolTranscriptRecord {
            schema_version: TOOL_TRANSCRIPT_SCHEMA_VERSION,
            mode: self.mode,
            entries: self.recorded,
        }
    }

    #[allow(dead_code)]
    pub fn expected_record(&self) -> Option<ToolTranscriptRecord> {
        if self.expected.is_empty() {
            None
        } else {
            Some(ToolTranscriptRecord {
                schema_version: TOOL_TRANSCRIPT_SCHEMA_VERSION,
                mode: ToolMode::Replay,
                entries: self.expected.clone(),
            })
        }
    }
}

pub fn read_transcript(path: &Path) -> Result<ToolTranscriptRecord> {
    let file = std::fs::File::open(path).with_context(|| "failed to open tool transcript")?;
    let record: ToolTranscriptRecord =
        serde_json::from_reader(file).with_context(|| "failed to parse tool transcript")?;
    Ok(record)
}

pub fn write_transcript(path: &Path, record: &ToolTranscriptRecord) -> Result<()> {
    canonical_json::write_json(path, record, "tool transcript")?;
    Ok(())
}

fn stub_response(request: &ToolRequest) -> ToolResponse {
    let mut hasher = Sha256::new();
    if let Ok(bytes) = canonical_json::to_vec(request) {
        hasher.update(bytes);
    }
    let digest = hasher.finalize();
    let hash = hex_string(&digest);
    ToolResponse {
        tool_name: request.tool_name.clone(),
        output: serde_json::json!({
            "stub": true,
            "hash": hash,
        }),
        success: true,
        simulated_latency_ms: None,
    }
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}

fn llm_live_response(request: &ToolRequest) -> Option<ToolResponse> {
    let parsed = llm::parse_tool_request(request).ok()?;
    let backend = llm::StubLlmBackend;
    let response = backend.generate(&parsed).ok()?;
    let output = llm::response_to_tool_output(&response).ok()?;
    Some(ToolResponse {
        tool_name: request.tool_name.clone(),
        output,
        success: true,
        simulated_latency_ms: None,
    })
}
