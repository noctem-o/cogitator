use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const TOOL_TRANSCRIPT_SCHEMA_VERSION: u32 = 2;
pub const TOOL_SCHEMA_VERSION: u32 = 1;

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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub index: u32,
    pub step: u32,
    pub request: ToolRequest,
    pub response: ToolResponse,
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
    pub tool_schema_version: u32,
    pub recorded_by: String,
    pub entries: Vec<ToolCall>,
}

pub struct ToolTranscript {
    mode: ToolMode,
    expected: Vec<ToolCall>,
    recorded: Vec<ToolCall>,
    cursor: usize,
    mismatches: Vec<ToolMismatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolMismatchKind {
    OrderingMismatch,
    RequestMismatch,
    ResponseMismatch,
    OutputHashMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolMismatch {
    pub index: usize,
    pub step: Option<u32>,
    pub kind: ToolMismatchKind,
    pub expected: Option<serde_json::Value>,
    pub actual: Option<serde_json::Value>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolTranscriptHandle {
    pub schema_version: u32,
    pub tool_schema_version: u32,
    pub mode: ToolMode,
}

impl ToolTranscript {
    pub fn new_live() -> Self {
        Self {
            mode: ToolMode::Live,
            expected: Vec::new(),
            recorded: Vec::new(),
            cursor: 0,
            mismatches: Vec::new(),
        }
    }

    pub fn new_replay(expected: ToolTranscriptRecord) -> Self {
        Self {
            mode: ToolMode::Replay,
            expected: expected.entries,
            recorded: Vec::new(),
            cursor: 0,
            mismatches: Vec::new(),
        }
    }

    pub fn mode(&self) -> ToolMode {
        self.mode.clone()
    }

    pub fn mismatches(&self) -> &[ToolMismatch] {
        &self.mismatches
    }

    pub fn execute(&mut self, step: u32, request: ToolRequest) -> ToolResponse {
        match self.mode {
            ToolMode::Live => {
                let response = stub_response(&request);
                self.recorded.push(ToolCall {
                    index: self.cursor as u32,
                    step,
                    request,
                    response: response.clone(),
                });
                self.cursor += 1;
                response
            }
            ToolMode::Replay => {
                let index = self.cursor;
                let response = if let Some(expected) = self.expected.get(index) {
                    if expected.index != index as u32 {
                        self.mismatches.push(ToolMismatch {
                            index,
                            step: Some(step),
                            kind: ToolMismatchKind::OrderingMismatch,
                            expected: Some(serde_json::json!({ "index": expected.index })),
                            actual: Some(serde_json::json!({ "index": index })),
                            message: format!(
                                "tool ordering mismatch: expected index {}, got {}",
                                expected.index, index
                            ),
                        });
                    }
                    if expected.step != step {
                        self.mismatches.push(ToolMismatch {
                            index,
                            step: Some(step),
                            kind: ToolMismatchKind::OrderingMismatch,
                            expected: Some(serde_json::json!({ "step": expected.step })),
                            actual: Some(serde_json::json!({ "step": step })),
                            message: format!(
                                "tool step mismatch: expected {}, got {}",
                                expected.step, step
                            ),
                        });
                    }
                    if expected.request != request {
                        self.mismatches.push(ToolMismatch {
                            index,
                            step: Some(step),
                            kind: ToolMismatchKind::RequestMismatch,
                            expected: Some(
                                serde_json::to_value(&expected.request).unwrap_or_default(),
                            ),
                            actual: Some(serde_json::to_value(&request).unwrap_or_default()),
                            message: format!("tool request mismatch at index {}", index),
                        });
                    }
                    expected.response.clone()
                } else {
                    self.mismatches.push(ToolMismatch {
                        index,
                        step: Some(step),
                        kind: ToolMismatchKind::OrderingMismatch,
                        expected: None,
                        actual: Some(serde_json::to_value(&request).unwrap_or_default()),
                        message: format!("unexpected tool request at index {}", index),
                    });
                    stub_response(&request)
                };
                self.recorded.push(ToolCall {
                    index: index as u32,
                    step,
                    request,
                    response: response.clone(),
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
            tool_schema_version: TOOL_SCHEMA_VERSION,
            recorded_by: "cogitator".to_string(),
            entries: self.recorded,
        }
    }

    pub fn expected_record(&self) -> Option<ToolTranscriptRecord> {
        if self.expected.is_empty() {
            None
        } else {
            Some(ToolTranscriptRecord {
                schema_version: TOOL_TRANSCRIPT_SCHEMA_VERSION,
                mode: ToolMode::Replay,
                tool_schema_version: TOOL_SCHEMA_VERSION,
                recorded_by: "cogitator".to_string(),
                entries: self.expected.clone(),
            })
        }
    }

    pub fn handle(&self) -> ToolTranscriptHandle {
        ToolTranscriptHandle {
            schema_version: TOOL_TRANSCRIPT_SCHEMA_VERSION,
            tool_schema_version: TOOL_SCHEMA_VERSION,
            mode: self.mode.clone(),
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
    let file = std::fs::File::create(path).with_context(|| "failed to write tool transcript")?;
    serde_json::to_writer_pretty(file, record)
        .with_context(|| "failed to serialize tool transcript")?;
    Ok(())
}

fn stub_response(request: &ToolRequest) -> ToolResponse {
    let mut hasher = Sha256::new();
    if let Ok(bytes) = serde_json::to_vec(request) {
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
    }
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}
