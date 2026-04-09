//! Integration tests for the 2.0 pre-call policy interception layer.
//!
//! These tests verify the complete end-to-end path:
//!   1. A `PolicyEngine` loaded from a rule set blocks / phantoms a call.
//!   2. A `PhantomEntry` is recorded with the correct disposition, rule id,
//!      and reason — and no real `ToolCall` is emitted for that request.
//!   3. The blocked call produces a synthetic `{ blocked: true, reason: "..." }`
//!      response that is returned to the agent (not an error).
//!   4. `ToolTranscriptRecord.policy_digest` is non-empty when a real policy
//!      was attached, confirming the digest is committed into the witness chain.
//!   5. History-guard rules fire after the call budget is exceeded, not before.

use cogitator::policy::{CallHistory, PhantomDisposition, PolicyDocument, PolicyEngine, PolicyRule, PolicyVerdict};
use cogitator::tooling::{ToolRequest, ToolTranscript};

// ─── helpers ────────────────────────────────────────────────────────────────

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
        reason: format!("budget exceeded: max {} calls matching {}", max, hist_pattern),
    }
}

// ─── Block verdict ───────────────────────────────────────────────────────────

#[test]
fn blocked_call_produces_phantom_entry_not_tool_call() {
    let engine = engine_with_rules(vec![
        block_rule("no-lookup", "clawdbot.lookup", "lookups disabled in this run"),
    ]);

    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    let response = transcript.execute(0, req("clawdbot.lookup"));

    // Agent receives a blocked response — not an error, not a real outcome.
    assert!(!response.success, "blocked response must have success=false");
    assert_eq!(
        response.output.get("blocked").and_then(|v| v.as_bool()),
        Some(true),
        "blocked response must carry blocked:true"
    );
    assert!(
        response.output.get("reason").is_some(),
        "blocked response must carry a reason"
    );

    let record = transcript.into_record();

    // No real ToolCall was dispatched.
    assert!(
        record.entries.is_empty(),
        "a blocked call must not appear in ToolCall entries"
    );

    // One PhantomEntry was recorded.
    assert_eq!(record.phantom_entries.len(), 1);
    let phantom = &record.phantom_entries[0];
    assert_eq!(phantom.tool_name, "clawdbot.lookup");
    assert_eq!(phantom.disposition, PhantomDisposition::Blocked);
    assert_eq!(phantom.rule_id.as_deref(), Some("no-lookup"));
    assert_eq!(phantom.reason, "lookups disabled in this run");
    assert_eq!(phantom.step, 0);
    assert_eq!(phantom.tool_call_idx, 0);
}

// ─── Phantom verdict ─────────────────────────────────────────────────────────

#[test]
fn phantom_call_has_phantom_disposition() {
    let engine = engine_with_rules(vec![
        phantom_rule("observe-research", "research.**", "research tools are observe-only"),
    ]);

    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    let response = transcript.execute(1, req("research.fetch"));

    assert!(!response.success);
    assert_eq!(response.output.get("blocked").and_then(|v| v.as_bool()), Some(true));

    let record = transcript.into_record();
    assert!(record.entries.is_empty());
    assert_eq!(record.phantom_entries.len(), 1);
    assert_eq!(record.phantom_entries[0].disposition, PhantomDisposition::Phantom);
    assert_eq!(record.phantom_entries[0].rule_id.as_deref(), Some("observe-research"));
}

// ─── Allow verdict ───────────────────────────────────────────────────────────

#[test]
fn allowed_call_appears_in_entries_not_phantom() {
    // Allow-all engine — everything goes through.
    let mut transcript = ToolTranscript::new_live(None);
    let response = transcript.execute(0, req("clawdbot.lookup"));

    assert!(response.success, "allowed call must succeed");

    let record = transcript.into_record();
    assert_eq!(record.entries.len(), 1, "one real ToolCall must be recorded");
    assert!(record.phantom_entries.is_empty(), "no phantom entries for allowed calls");
}

// ─── Policy digest in transcript record ──────────────────────────────────────

#[test]
fn policy_digest_committed_into_transcript_record() {
    let engine = engine_with_rules(vec![
        block_rule("any-block", "clawdbot.*", "blocked"),
    ]);
    // The engine was constructed with digest = "deadbeef".
    let digest = engine.digest.clone();

    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);
    let _ = transcript.execute(0, req("clawdbot.lookup"));
    let record = transcript.into_record();

    assert_eq!(
        record.policy_digest, digest,
        "policy_digest in transcript record must match the engine digest"
    );
}

#[test]
fn allow_all_engine_produces_empty_policy_digest() {
    let mut transcript = ToolTranscript::new_live(None); // no .with_policy() — allow-all default
    let _ = transcript.execute(0, req("clawdbot.lookup"));
    let record = transcript.into_record();

    assert!(
        record.policy_digest.is_empty(),
        "allow-all (no policy) must produce empty policy_digest"
    );
}

// ─── History guard ───────────────────────────────────────────────────────────

#[test]
fn history_guard_allows_calls_within_budget() {
    // Budget: max 2 calls matching clawdbot.* before blocking.
    let engine = engine_with_rules(vec![
        history_guard_rule("lookup-budget", "clawdbot.*", "clawdbot.*", 2),
    ]);

    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);

    // Call 1 — under budget
    let r1 = transcript.execute(0, req("clawdbot.lookup"));
    assert!(r1.success, "first call must be allowed");

    // Call 2 — still under budget (count=1, max=2 → 1 <= 2, guard does not fire)
    let r2 = transcript.execute(1, req("clawdbot.lookup"));
    assert!(r2.success, "second call must be allowed");

    let record = transcript.into_record();
    assert_eq!(record.entries.len(), 2);
    assert!(record.phantom_entries.is_empty());
}

#[test]
fn history_guard_blocks_calls_over_budget() {
    // Budget: max 2 calls matching clawdbot.* before blocking.
    let engine = engine_with_rules(vec![
        history_guard_rule("lookup-budget", "clawdbot.*", "clawdbot.*", 2),
    ]);

    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);

    // Burn through the budget.
    transcript.execute(0, req("clawdbot.lookup"));
    transcript.execute(1, req("clawdbot.lookup"));
    transcript.execute(2, req("clawdbot.lookup"));

    // Third call (count=3, max=2 → 3 > 2) must be blocked.
    let r3 = transcript.execute(3, req("clawdbot.lookup"));
    assert!(!r3.success, "call over budget must be blocked");
    assert_eq!(r3.output.get("blocked").and_then(|v| v.as_bool()), Some(true));

    let record = transcript.into_record();
    // 3 real calls allowed (steps 0, 1, 2), 1 phantom (step 3).
    assert_eq!(record.entries.len(), 3);
    assert_eq!(record.phantom_entries.len(), 1);
    assert_eq!(record.phantom_entries[0].rule_id.as_deref(), Some("lookup-budget"));
}

// ─── Mixed allowed + blocked in one run ──────────────────────────────────────

#[test]
fn mixed_run_interleaves_entries_and_phantom_entries() {
    // Block only `clawdbot.trade`; allow everything else.
    let engine = engine_with_rules(vec![
        block_rule("no-trade", "clawdbot.trade", "trading not permitted"),
    ]);

    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);

    transcript.execute(0, req("clawdbot.lookup")); // allowed
    transcript.execute(1, req("clawdbot.trade"));  // blocked → phantom
    transcript.execute(2, req("clawdbot.lookup")); // allowed

    let record = transcript.into_record();
    assert_eq!(record.entries.len(), 2, "two real calls");
    assert_eq!(record.phantom_entries.len(), 1, "one phantom");
    assert_eq!(record.phantom_entries[0].tool_name, "clawdbot.trade");
    assert_eq!(record.phantom_entries[0].step, 1);
}

// ─── Phantom entry indexes ────────────────────────────────────────────────────

#[test]
fn phantom_entry_tool_call_idx_is_sequential_across_mixed_run() {
    // Block the second call; allow first and third.
    let engine = engine_with_rules(vec![
        block_rule("no-trade", "clawdbot.trade", "trading not permitted"),
    ]);

    let mut transcript = ToolTranscript::new_live(None).with_policy(engine);

    transcript.execute(0, req("clawdbot.lookup")); // idx=0, allowed
    transcript.execute(0, req("clawdbot.trade"));  // idx=1, blocked
    transcript.execute(0, req("clawdbot.lookup")); // idx=2, allowed

    let record = transcript.into_record();
    assert_eq!(record.phantom_entries[0].tool_call_idx, 1);
    assert_eq!(record.entries[0].tool_call_idx, 0);
    assert_eq!(record.entries[1].tool_call_idx, 2);
}
