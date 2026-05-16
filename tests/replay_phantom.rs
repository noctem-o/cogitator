use cogitator::drift;
use cogitator::policy::PhantomDisposition;
use cogitator::report::DriftIssue;
use cogitator::tooling::{
    PhantomEntry, ToolMode, ToolRequest, ToolTranscript, ToolTranscriptRecord,
    TOOL_TRANSCRIPT_SCHEMA_VERSION,
};

fn phantom_entry() -> PhantomEntry {
    PhantomEntry {
        step: 0,
        tool_call_idx: 0,
        tool_name: "trade.buy".to_string(),
        request: serde_json::json!({"qty": 1}),
        disposition: PhantomDisposition::Blocked,
        rule_id: Some("deny-trade".to_string()),
        reason: "blocked by policy".to_string(),
    }
}

#[test]
fn replay_preserves_phantom_entries() {
    let expected = ToolTranscriptRecord {
        schema_version: TOOL_TRANSCRIPT_SCHEMA_VERSION,
        mode: ToolMode::Live,
        entries: vec![],
        phantom_entries: vec![phantom_entry()],
        policy_digest: "abc".to_string(),
    };
    let mut replay = ToolTranscript::new_replay(expected);
    let response = replay.execute(
        0,
        ToolRequest {
            tool_name: "trade.buy".to_string(),
            arguments: serde_json::json!({"qty": 1}),
        },
    );
    assert!(!response.success);
    let record = replay.into_record();
    assert!(record.entries.is_empty());
    assert_eq!(record.phantom_entries.len(), 1);
    assert_eq!(record.phantom_entries[0].tool_name, "trade.buy");
}

#[test]
fn drift_reports_policy_and_phantom_mismatch() {
    let expected = ToolTranscriptRecord {
        policy_digest: "a".to_string(),
        phantom_entries: vec![phantom_entry()],
        ..Default::default()
    };

    let mut actual = expected.clone();
    actual.policy_digest = "b".to_string();
    actual.phantom_entries[0].reason = "other".to_string();

    let report = drift::detect_transcript_drift(&expected, &actual);
    assert!(report
        .issues
        .iter()
        .any(|i| matches!(i, DriftIssue::PolicyDigestMismatch { .. })));
    assert!(report
        .issues
        .iter()
        .any(|i| matches!(i, DriftIssue::PhantomReasonMismatch { .. })));
}
