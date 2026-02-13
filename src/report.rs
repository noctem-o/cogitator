use serde::{Deserialize, Serialize};

/// Represents a detected drift or mismatch during verification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriftIssue {
    /// Tool call step number mismatch
    ToolStepMismatch {
        index: u32,
        expected: u32,
        actual: u32,
    },

    /// Tool call index mismatch within a step
    ToolCallIndexMismatch {
        index: u32,
        expected: u32,
        actual: u32,
    },

    /// Tool request (name or arguments) mismatch
    ToolRequestMismatch { index: u32 },

    /// Tool transcript schema version differs
    TranscriptSchemaMismatch { expected: u32, actual: u32 },

    /// Tool transcript mode differs (record vs replay, etc.)
    TranscriptModeMismatch { expected: String, actual: String },

    /// Tool transcript entry count differs
    TranscriptLengthMismatch { expected: u32, actual: u32 },

    /// Tool fault injection differs for a given tool call
    ToolFaultMismatch { index: u32 },

    /// Tool outcome (response) mismatch
    ToolOutcomeMismatch { index: u32 },

    /// Number of tool calls differs
    ToolCallCountMismatch { expected: u32, actual: u32 },

    /// Unexpected tool request during replay
    UnexpectedToolRequest { index: u32 },

    /// Ordeal-specific output mismatch (for ordeal.rs tests)
    OrdealOutputMismatch {
        step: u32,
        tool_name: String,
        tool_call_idx: u32,
        json_pointer: String,
        label: String,
        issue_kind: String,
        expected: String,
        actual: String,
    },
}
