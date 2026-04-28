use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriftIssue {
    TranscriptSchemaMismatch {
        expected: u32,
        actual: u32,
    },
    TranscriptModeMismatch {
        expected: String,
        actual: String,
    },
    TranscriptLengthMismatch {
        expected: u32,
        actual: u32,
    },
    ToolCallCountMismatch {
        expected: u32,
        actual: u32,
    },
    ToolStepMismatch {
        index: u32,
        expected: u32,
        actual: u32,
    },
    ToolCallIndexMismatch {
        index: u32,
        expected: u32,
        actual: u32,
    },
    ToolRequestMismatch {
        index: u32,
    },
    #[serde(rename = "tool_response_hash_mismatch")]
    ToolOutcomeMismatch {
        index: u32,
    },
    ToolFaultMismatch {
        index: u32,
    },
    UnexpectedToolRequest {
        index: u32,
    },
    OrdealOutputMismatch {
        step: u32,
        tool_call_idx: u32,
        tool_name: String,
        json_pointer: String,
        label: String,
        issue_kind: String,
        expected: String,
        actual: String,
    },
}
