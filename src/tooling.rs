use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::canonical_json;
use crate::chaos::{apply_fault, ChaosEngine, FaultKind, FaultRecord};
use crate::llm;
use crate::llm::LlmBackend;
use crate::report::DriftIssue;

pub const TOOL_TRANSCRIPT_SCHEMA_VERSION: u32 = 3;

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
#[serde(rename_all = "snake_case")]
pub enum ToolErrorKind {
    Timeout,
    Drop,
    Corrupt,
    ToolError,
}

impl ToolErrorKind {
    fn as_str(&self) -> &'static str {
        match self {
            ToolErrorKind::Timeout => "timeout",
            ToolErrorKind::Drop => "drop",
            ToolErrorKind::Corrupt => "corrupt",
            ToolErrorKind::ToolError => "tool_error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolError {
    pub error_kind: ToolErrorKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOutcome {
    Ok {
        output: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        simulated_latency_ms: Option<u64>,
    },
    Err {
        error: ToolError,
        #[serde(skip_serializing_if = "Option::is_none")]
        simulated_latency_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptFault {
    Timeout {
        domain: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    Drop {
        domain: String,
    },
    Corrupt {
        domain: String,
        mask: u64,
    },
    LatencySim {
        domain: String,
        latency_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub step: u32,
    pub tool_call_idx: u32,
    pub tool_name: String,
    pub request: serde_json::Value,
    pub outcome: ToolOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<TranscriptFault>,
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

    /// Returns the current execution mode. Used in tests to assert Live vs Replay.
    #[cfg(test)]
    pub fn mode(&self) -> ToolMode {
        self.mode.clone()
    }

    pub fn mismatches(&self) -> &[DriftIssue] {
        &self.mismatches
    }

    pub fn execute(&mut self, step: u32, request: ToolRequest) -> ToolResponse {
        let response = if request.tool_name == llm::LlmRequest::tool_name() {
            llm_live_response(&request).unwrap_or_else(|| stub_response(&request))
        } else {
            stub_response(&request)
        };

        self.execute_with_response(step, request, response)
    }

    pub fn execute_with_response(
        &mut self,
        step: u32,
        request: ToolRequest,
        response: ToolResponse,
    ) -> ToolResponse {
        let tool_call_idx = self.next_call_idx;
        self.next_call_idx = self.next_call_idx.saturating_add(1);

        match self.mode {
            ToolMode::Live => {
                let mut response = response;
                let fault = if let Some(chaos) = &self.chaos {
                    chaos.decide_fault(step, tool_call_idx, &request.tool_name)
                } else {
                    None
                };

                if let Some(fault_record) = fault.as_ref() {
                    let original = response.clone();
                    response = apply_fault(&request, response, fault_record).unwrap_or(original);
                }

                let outcome = outcome_from_response(&response, fault.as_ref());

                self.recorded.push(ToolCall {
                    step,
                    tool_call_idx,
                    tool_name: request.tool_name.clone(),
                    request: request.arguments.clone(),
                    outcome,
                    fault: fault.as_ref().map(TranscriptFault::from),
                });

                response
            }
            ToolMode::Replay => {
                debug_assert!(self.chaos.is_none(), "replay must not apply chaos");

                // Consolidate to a single borrow so the cursor is read exactly once
                // for both response derivation and outcome/fault recording.
                let expected_entry = self.expected.get(self.cursor);

                let response = if let Some(expected) = expected_entry {
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

                    if expected.tool_name != request.tool_name
                        || expected.request != request.arguments
                    {
                        self.mismatches.push(DriftIssue::ToolRequestMismatch {
                            index: self.cursor as u32,
                        });
                    }

                    response_from_outcome(&expected.tool_name, &expected.outcome)
                } else {
                    self.mismatches.push(DriftIssue::UnexpectedToolRequest {
                        index: self.cursor as u32,
                    });
                    stub_response(&request)
                };

                let (outcome, fault) = if let Some(expected) = expected_entry {
                    (expected.outcome.clone(), expected.fault.clone())
                } else {
                    (outcome_from_response(&response, None), None)
                };

                self.recorded.push(ToolCall {
                    step,
                    tool_call_idx,
                    tool_name: request.tool_name.clone(),
                    request: request.arguments.clone(),
                    outcome,
                    fault,
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

    /// Returns the expected entries as a record, if any are present.
    /// Used in tests to inspect the replay baseline.
    #[cfg(test)]
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

fn outcome_from_response(response: &ToolResponse, fault: Option<&FaultRecord>) -> ToolOutcome {
    if response.success {
        ToolOutcome::Ok {
            output: response.output.clone(),
            simulated_latency_ms: response.simulated_latency_ms,
        }
    } else {
        let error_kind = error_kind_from_fault(response, fault);
        let message = response
            .output
            .get("message")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());

        ToolOutcome::Err {
            error: ToolError {
                error_kind,
                message,
            },
            simulated_latency_ms: response.simulated_latency_ms,
        }
    }
}

fn error_kind_from_fault(response: &ToolResponse, fault: Option<&FaultRecord>) -> ToolErrorKind {
    if let Some(fault) = fault {
        return match fault.kind {
            FaultKind::Timeout => ToolErrorKind::Timeout,
            FaultKind::Drop => ToolErrorKind::Drop,
            FaultKind::Corrupt => ToolErrorKind::Corrupt,
            FaultKind::LatencySim => ToolErrorKind::ToolError,
        };
    }

    match response
        .output
        .get("error")
        .and_then(|value| value.as_str())
    {
        Some("timeout") => ToolErrorKind::Timeout,
        Some("drop") => ToolErrorKind::Drop,
        Some("corrupt") => ToolErrorKind::Corrupt,
        _ => ToolErrorKind::ToolError,
    }
}

fn response_from_outcome(tool_name: &str, outcome: &ToolOutcome) -> ToolResponse {
    match outcome {
        ToolOutcome::Ok {
            output,
            simulated_latency_ms,
        } => ToolResponse {
            tool_name: tool_name.to_string(),
            output: output.clone(),
            success: true,
            simulated_latency_ms: *simulated_latency_ms,
        },
        ToolOutcome::Err {
            error,
            simulated_latency_ms,
        } => {
            let output = match error.error_kind {
                ToolErrorKind::Drop => serde_json::Value::Null,
                _ => {
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "error".to_string(),
                        serde_json::Value::String(error.error_kind.as_str().to_string()),
                    );
                    if let Some(message) = error.message.as_ref() {
                        map.insert(
                            "message".to_string(),
                            serde_json::Value::String(message.clone()),
                        );
                    }
                    serde_json::Value::Object(map)
                }
            };

            ToolResponse {
                tool_name: tool_name.to_string(),
                output,
                success: false,
                simulated_latency_ms: *simulated_latency_ms,
            }
        }
    }
}

impl From<&FaultRecord> for TranscriptFault {
    fn from(value: &FaultRecord) -> Self {
        match value.kind {
            FaultKind::Timeout => TranscriptFault::Timeout {
                domain: value.domain.clone(),
                timeout_ms: None,
            },
            FaultKind::Drop => TranscriptFault::Drop {
                domain: value.domain.clone(),
            },
            FaultKind::Corrupt => TranscriptFault::Corrupt {
                domain: value.domain.clone(),
                mask: value.params.mask.unwrap_or_default(),
            },
            FaultKind::LatencySim => TranscriptFault::LatencySim {
                domain: value.domain.clone(),
                latency_ms: value.params.latency_ms.unwrap_or_default(),
            },
        }
    }
}

fn stub_response(request: &ToolRequest) -> ToolResponse {
    let mut hasher = Sha256::new();
    if let Ok(bytes) = canonical_json::to_vec(request) {
        hasher.update(bytes);
    }

    let digest = hasher.finalize();
    let hash = crate::hex::encode(&digest);

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
