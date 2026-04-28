use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum DriftIssue {
    // Existing variants
    TranscriptSchemaMismatch,
    TranscriptModeMismatch,
    TranscriptLengthMismatch,
    ToolFaultMismatch,
}
