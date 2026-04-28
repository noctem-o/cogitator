use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriftIssue {
    // Existing variants
    TranscriptSchemaMismatch,
    TranscriptModeMismatch,
    TranscriptLengthMismatch,
    ToolFaultMismatch,
}
