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

#[derive(Debug, Cl
