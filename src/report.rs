use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum DriftIssue {
    // Existing variants
    TranscriptSchemaMismatch,
    TranscriptModeMismatch,
    TranscriptLengthMismatch,
    ToolFaultMismatch,
}