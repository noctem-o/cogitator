//! Integration tests for the 2.0 pre-call policy interception layer.

use cogitator::policy::{
    PhantomDisposition, PolicyDocument, PolicyEngine, PolicyRule, PolicyVerdict,
};
use cogitator::tooling::{ToolRequest, ToolTranscript};

fn engine_with_rules(rules: Vec<PolicyRule>) -> PolicyEngine {
    PolicyEngine {
        document: PolicyDocument {
            schema_version: cogitator::policy::POLICY_SCHEMA_VERSION,
            rules,
        },
        digest: "deadbeef".to_string(),
    }
}

fn req(name: &str) -> ToolRequest {
    ToolRequest {
        tool_name: name.to_string(),
        arguments: serde_json::json!({ "query": "test" }),
    }
}

fn block_rule(id: &str, pattern: &str, reason: &str) -> PolicyRule {
    PolicyRule {
        id: id.to_string(),
        tool_pattern: pattern.to_string(),
        history_tool_pattern: None,
        history_max_calls: None,
        verdict: PolicyVerdict::Block,
        reason: reason.to_string(),
    }
}

fn phantom_rule(id: &str, pattern: &str, reason: &str) -> PolicyRule {
    PolicyRule {
        id: id.to_string(),
        tool_pattern: pattern.to_string(),
        history_tool_pattern: None,
        history_max_calls: None,
        verdict: PolicyVerdict::Phantom,
        reason: reason.to_string(),
    }
}

fn history_guard_rule(id: &str, pattern: &str, hist_pattern: &str, max: usize) -> PolicyRule {
    PolicyRule {
        id: id.to_string(),
        tool_pattern: pattern.to_string(),
        history_tool_pattern: Some(hist_pattern.to_string()),
        history_max_calls: Some(max),
        verdict: PolicyVerdict::Block,
        reason: format!(
            "budget exceeded: max {} calls matching {}",
            max, hist_pattern
        ),
    }
}

#[test]
fn blocked_call_produces_phantom_entry_not_tool_call() {
    let engine = engine_with_rules(vec![block_rule(
        "no-lookup",
        "clawdbot.lookup",
        "lookups disabled in this run",
    )]);

    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    let response = transcript.execute(0, req("clawdbot.lookup"));

    assert!(!response.success);
    assert_eq!(
        response.output.get("blocked").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(response.output.get("reason").is_some());

    let record = transcript.into_record();
    assert!(record.entries.is_empty());
    assert_eq!(record.phantom_entries.len(), 1);
    let phantom = &record.phantom_entries[0];
    assert_eq!(phantom.tool_name, "clawdbot.lookup");
    assert_eq!(phantom.disposition, PhantomDisposition::Blocked);
    assert_eq!(phantom.rule_id.as_deref(), Some("no-lookup"));
    assert_eq!(phantom.reason, "lookups disabled in this run");
    assert_eq!(phantom.step, 0);
    assert_eq!(phantom.tool_call_idx, 0);
}

#[test]
fn phantom_call_has_phantom_disposition() {
    let engine = engine_with_rules(vec![phantom_rule(
        "observe-research",
        "research.**",
        "research tools are observe-only",
    )]);

    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    let response = transcript.execute(1, req("research.fetch"));

    assert!(!response.success);
    assert_eq!(
        response.output.get("blocked").and_then(|v| v.as_bool()),
        Some(true)
    );

    let record = transcript.into_record();
    assert!(record.entries.is_empty());
    assert_eq!(record.phantom_entries.len(), 1);
    assert_eq!(
        record.phantom_entries[0].disposition,
        PhantomDisposition::Phantom
    );
    assert_eq!(
        record.phantom_entries[0].rule_id.as_deref(),
        Some("observe-research")
    );
}

#[test]
fn allowed_call_appears_in_entries_not_phantom() {
    let mut transcript = ToolTranscript::new_live(None);
    let response = transcript.execute(0, req("clawdbot.lookup"));
    assert!(response.success);
    let record = transcript.into_record();
    assert_eq!(record.entries.len(), 1);
    assert!(record.phantom_entries.is_empty());
}

#[test]
fn policy_digest_committed_into_transcript_record() {
    let engine = engine_with_rules(vec![block_rule("any-block", "clawdbot.*", "blocked")]);
    let digest = engine.digest.clone();
    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    let _ = transcript.execute(0, req("clawdbot.lookup"));
    let record = transcript.into_record();
    assert_eq!(record.policy_digest, digest);
}

#[test]
fn allow_all_engine_produces_empty_policy_digest() {
    let mut transcript = ToolTranscript::new_live(None);
    let _ = transcript.execute(0, req("clawdbot.lookup"));
    let record = transcript.into_record();
    assert!(record.policy_digest.is_empty());
}

#[test]
fn history_guard_allows_calls_within_budget() {
    let engine = engine_with_rules(vec![history_guard_rule(
        "lookup-budget",
        "clawdbot.*",
        "clawdbot.*",
        2,
    )]);
    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    let r1 = transcript.execute(0, req("clawdbot.lookup"));
    assert!(r1.success);
    let r2 = transcript.execute(1, req("clawdbot.lookup"));
    assert!(r2.success);
    let record = transcript.into_record();
    assert_eq!(record.entries.len(), 2);
    assert!(record.phantom_entries.is_empty());
}

#[test]
fn history_guard_blocks_calls_over_budget() {
    let engine = engine_with_rules(vec![history_guard_rule(
        "lookup-budget",
        "clawdbot.*",
        "clawdbot.*",
        2,
    )]);
    // max = 2: calls 1 and 2 are allowed, call 3 is blocked.
    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    transcript.execute(0, req("clawdbot.lookup"));
    transcript.execute(1, req("clawdbot.lookup"));
    let r3 = transcript.execute(2, req("clawdbot.lookup"));
    assert!(!r3.success);
    assert_eq!(
        r3.output.get("blocked").and_then(|v| v.as_bool()),
        Some(true)
    );
    let record = transcript.into_record();
    assert_eq!(record.entries.len(), 2);
    assert_eq!(record.phantom_entries.len(), 1);
    assert_eq!(
        record.phantom_entries[0].rule_id.as_deref(),
        Some("lookup-budget")
    );
}

#[test]
fn mixed_run_interleaves_entries_and_phantom_entries() {
    let engine = engine_with_rules(vec![block_rule(
        "no-trade",
        "clawdbot.trade",
        "trading not permitted",
    )]);
    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    transcript.execute(0, req("clawdbot.lookup"));
    transcript.execute(1, req("clawdbot.trade"));
    transcript.execute(2, req("clawdbot.lookup"));
    let record = transcript.into_record();
    assert_eq!(record.entries.len(), 2);
    assert_eq!(record.phantom_entries.len(), 1);
    assert_eq!(record.phantom_entries[0].tool_name, "clawdbot.trade");
    assert_eq!(record.phantom_entries[0].step, 1);
}

#[test]
fn phantom_entry_tool_call_idx_is_sequential_across_mixed_run() {
    let engine = engine_with_rules(vec![block_rule(
        "no-trade",
        "clawdbot.trade",
        "trading not permitted",
    )]);
    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    transcript.execute(0, req("clawdbot.lookup"));
    transcript.execute(0, req("clawdbot.trade"));
    transcript.execute(0, req("clawdbot.lookup"));
    let record = transcript.into_record();
    assert_eq!(record.phantom_entries[0].tool_call_idx, 1);
    assert_eq!(record.entries[0].tool_call_idx, 0);
    assert_eq!(record.entries[1].tool_call_idx, 2);
}
