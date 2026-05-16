use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
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
    PolicyDigestMismatch {
        expected: String,
        actual: String,
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
    ToolOutcomeMismatch {
        index: u32,
    },
    ToolFaultMismatch {
        index: u32,
    },
    PhantomLengthMismatch {
        expected: u32,
        actual: u32,
    },
    PhantomStepMismatch {
        index: u32,
        expected: u32,
        actual: u32,
    },
    PhantomToolCallIndexMismatch {
        index: u32,
        expected: u32,
        actual: u32,
    },
    PhantomRequestMismatch {
        index: u32,
    },
    PhantomDispositionMismatch {
        index: u32,
        expected: String,
        actual: String,
    },
    PhantomRuleMismatch {
        index: u32,
        expected: Option<String>,
        actual: Option<String>,
    },
    PhantomReasonMismatch {
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
