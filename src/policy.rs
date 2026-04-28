//! Policy engine for pre-call tool-call interception.
//!
//! Loads a TOML policy file and evaluates each incoming `ToolRequest` before
//! dispatch.  Returns one of three verdicts:
//!
//! - `Allow`   — execute normally, record as a real `ToolCall`
//! - `Block`   — do not execute; record as a `PhantomEntry` with `blocked` disposition
//! - `Phantom` — do not execute; record as a `PhantomEntry` with `phantom` disposition
//!   (semantically: the agent tried, the harness observed, no side-effect)
//!
//! The policy file path is embedded as a SHA-256 digest into `WitnessedMetadata`
//! so the exact policy version is part of the witness root.
//!
//! # Normalisation
//!
//! Tool names are case-folded to lowercase at two boundaries:
//!
//! - `ToolTranscript::execute` — before policy evaluation and before recording
//!   into `CallHistory`, so `Trade.Buy` and `trade.buy` are treated identically.
//! - `PolicyEngine::load` — rule patterns are lowercased at parse time, so
//!   operator typos in `policy.toml` cannot silently widen the allow surface.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::tooling::ToolRequest;

// ─── Schema ────────────────────────────────────────────────────────────────

pub const POLICY_SCHEMA_VERSION: u32 = 1;

/// The verdict returned by `PolicyEngine::evaluate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyVerdict {
    Allow,
    Block,
    Phantom,
}

/// Disposition stored in a `PhantomEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhantomDisposition {
    Blocked,
    Phantom,
}

// ─── Rule model ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRule {
    pub id: String,
    /// Glob pattern matched against the *lowercased* tool name.
    pub tool_pattern: String,
    #[serde(default)]
    pub history_tool_pattern: Option<String>,
    /// Maximum number of calls to allow through before blocking.
    /// A value of 2 means: allow calls 1, 2, and 3, block call 4 onward.
    /// Block fires when the history count strictly exceeds this value.
    #[serde(default)]
    pub history_max_calls: Option<usize>,
    pub verdict: PolicyVerdict,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
}

impl Default for PolicyDocument {
    fn default() -> Self {
        Self {
            schema_version: POLICY_SCHEMA_VERSION,
            rules: Vec::new(),
        }
    }
}

// ─── Call history ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct CallHistory {
    entries: Vec<HistoryEntry>,
}

#[derive(Debug, Clone)]
struct HistoryEntry {
    /// Already lowercased at record time.
    tool_name: String,
    /// Kept for future rule types that condition on the verdict of prior calls.
    #[allow(dead_code)]
    verdict: PolicyVerdict,
}

impl CallHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed call.  `tool_name` must already be lowercased.
    pub fn record(&mut self, tool_name: &str, verdict: PolicyVerdict) {
        self.entries.push(HistoryEntry {
            tool_name: tool_name.to_string(),
            verdict,
        });
    }

    /// Count history entries whose tool name matches `pattern`.
    /// Both the pattern and stored names are already lowercase.
    pub fn count_matching(&self, pattern: &str) -> usize {
        self.entries
            .iter()
            .filter(|e| tool_name_matches(pattern, &e.tool_name))
            .count()
    }
}

// ─── Engine ─────────────────────────────────────────────────────────────────

pub struct PolicyEngine {
    pub document: PolicyDocument,
    pub digest: String,
}

impl PolicyEngine {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::allow_all());
        }

        let raw = std::fs::read(path)
            .with_context(|| format!("failed to read policy file: {}", path.display()))?;

        let digest = {
            let mut h = Sha256::new();
            h.update(&raw);
            crate::hex::encode(&h.finalize())
        };

        let text = std::str::from_utf8(&raw)
            .with_context(|| format!("policy file is not valid UTF-8: {}", path.display()))?;

        let mut document: PolicyDocument = toml::from_str(text)
            .with_context(|| format!("failed to parse policy file: {}", path.display()))?;
        if document.schema_version != POLICY_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported policy schema_version {} in {} (expected {})",
                document.schema_version,
                path.display(),
                POLICY_SCHEMA_VERSION
            );
        }

        // Lowercase all patterns at load time so runtime matching is always
        // case-insensitive without paying the allocation cost per call.
        for rule in &mut document.rules {
            rule.tool_pattern = rule.tool_pattern.to_lowercase();
            if let Some(ref mut p) = rule.history_tool_pattern {
                *p = p.to_lowercase();
            }
        }

        Ok(Self { document, digest })
    }

    pub fn allow_all() -> Self {
        Self {
            document: PolicyDocument::default(),
            digest: String::new(),
        }
    }

    /// Evaluate a request against the policy.
    ///
    /// `request.tool_name` **must** already be lowercased by the caller
    /// (`ToolTranscript::execute` does this at the gate).
    pub fn evaluate(
        &self,
        request: &ToolRequest,
        history: &CallHistory,
    ) -> (PolicyVerdict, Option<String>, String) {
        for rule in &self.document.rules {
            if !tool_name_matches(&rule.tool_pattern, &request.tool_name) {
                continue;
            }

            if let (Some(hist_pattern), Some(max)) =
                (&rule.history_tool_pattern, rule.history_max_calls)
            {
                let count = history.count_matching(hist_pattern);
                // Allow while the history count is within budget (count <= max).
                // Block fires when count strictly exceeds max.
                // Example: max = 2 → calls 1, 2, & 3 are allowed, call 4 is blocked.
                if count <= max {
                    continue;
                }
            }

            return (
                rule.verdict.clone(),
                Some(rule.id.clone()),
                rule.reason.clone(),
            );
        }

        (PolicyVerdict::Allow, None, String::new())
    }
}

// ─── Glob matching ──────────────────────────────────────────────────────────
//
// Patterns and inputs are always lowercased before reaching these functions.
//
// Rules:
//   *   matches one or more characters within a single dot-separated segment
//       (will not consume a '.')
//   **  matches any sequence of characters including '.' (crosses segment
//       boundaries), matching zero or more characters
//   any other character matches literally

fn tool_name_matches(pattern: &str, tool_name: &str) -> bool {
    glob_match(pattern, tool_name)
}

/// Iterative glob match using backtracking via explicit stacks.
/// Avoids fragile recursive slice-pattern arms.
fn glob_match(pattern: &str, input: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let inp: Vec<char> = input.chars().collect();

    // dp(pi, ii) — can pat[pi..] match inp[ii..]?
    // Use an explicit stack of (pi, ii) states to explore rather than recursion.
    // We deduplicate visited states to avoid exponential blowup.
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    let mut visited = std::collections::HashSet::new();

    while let Some((pi, ii)) = stack.pop() {
        if !visited.insert((pi, ii)) {
            continue;
        }

        if pi == pat.len() {
            if ii == inp.len() {
                return true;
            }
            continue;
        }

        let pc = pat[pi];

        if pi + 1 < pat.len() && pc == '*' && pat[pi + 1] == '*' {
            // ** — greedy wildcard, crosses dots. Try matching zero chars
            // (skip **) or consume one input char and stay on **.
            stack.push((pi + 2, ii)); // skip ** entirely
            if ii < inp.len() {
                stack.push((pi, ii + 1)); // consume one char, stay on **
            }
        } else if pc == '*' {
            // Single * — matches any char except '.'
            stack.push((pi + 1, ii)); // skip * (match zero chars)
            if ii < inp.len() && inp[ii] != '.' {
                stack.push((pi, ii + 1)); // consume one non-dot char, stay on *
            }
        } else {
            // Literal character match
            if ii < inp.len() && inp[ii] == pc {
                stack.push((pi + 1, ii + 1));
            }
        }
    }

    false
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tooling::ToolRequest;

    fn req(name: &str) -> ToolRequest {
        ToolRequest {
            tool_name: name.to_lowercase(),
            arguments: serde_json::Value::Null,
        }
    }

    fn block_engine(pattern: &str) -> PolicyEngine {
        PolicyEngine {
            document: PolicyDocument {
                schema_version: 1,
                rules: vec![PolicyRule {
                    id: "test-block".to_string(),
                    tool_pattern: pattern.to_lowercase(),
                    history_tool_pattern: None,
                    history_max_calls: None,
                    verdict: PolicyVerdict::Block,
                    reason: "blocked".to_string(),
                }],
            },
            digest: String::new(),
        }
    }

    fn budget_engine(max: usize) -> PolicyEngine {
        PolicyEngine {
            document: PolicyDocument {
                schema_version: 1,
                rules: vec![PolicyRule {
                    id: "trade-limit".to_string(),
                    tool_pattern: "trade.*".to_string(),
                    history_tool_pattern: Some("trade.*".to_string()),
                    history_max_calls: Some(max),
                    verdict: PolicyVerdict::Block,
                    reason: "exceeded trade budget".to_string(),
                }],
            },
            digest: String::new(),
        }
    }

    #[test]
    fn allow_all_engine_permits_everything() {
        let engine = PolicyEngine::allow_all();
        let history = CallHistory::new();
        let (verdict, rule_id, _) = engine.evaluate(&req("trade.buy"), &history);
        assert_eq!(verdict, PolicyVerdict::Allow);
        assert!(rule_id.is_none());
    }

    #[test]
    fn block_rule_matches_tool_pattern() {
        let engine = block_engine("trade.*");
        let history = CallHistory::new();
        let (verdict, rule_id, reason) = engine.evaluate(&req("trade.buy"), &history);
        assert_eq!(verdict, PolicyVerdict::Block);
        assert_eq!(rule_id.as_deref(), Some("test-block"));
        assert_eq!(reason, "blocked");
    }

    // ── Case normalisation ───────────────────────────────────────────────────

    #[test]
    fn block_rule_is_case_insensitive() {
        // Pattern is lowercase (as stored after load-time normalisation).
        // Incoming names arrive lowercased from execute() gate.
        let engine = block_engine("trade.*");
        let history = CallHistory::new();
        // req() lowercases the name, mirroring what execute() does.
        assert_eq!(
            engine.evaluate(&req("Trade.Buy"), &history).0,
            PolicyVerdict::Block
        );
        assert_eq!(
            engine.evaluate(&req("TRADE.BUY"), &history).0,
            PolicyVerdict::Block
        );
        assert_eq!(
            engine.evaluate(&req("trade.BUY"), &history).0,
            PolicyVerdict::Block
        );
    }

    // ── Glob semantics ───────────────────────────────────────────────────────

    #[test]
    fn wildcard_does_not_cross_dot_boundary() {
        assert!(tool_name_matches("trade.*", "trade.buy"));
        assert!(!tool_name_matches("trade.*", "trade.buy.v2"));
        assert!(tool_name_matches("trade.**", "trade.buy.v2"));
    }

    // ── Budget guard ─────────────────────────────────────────────────────────

    #[test]
    fn history_guard_triggers_at_budget_boundary() {
        // max = 2: allow calls 1, 2, & 3, block call 4.
        let engine = budget_engine(2);
        let mut history = CallHistory::new();

        // Call 1 — allowed (count = 0, 0 <= 2).
        let (v, _, _) = engine.evaluate(&req("trade.buy"), &history);
        assert_eq!(v, PolicyVerdict::Allow);
        history.record("trade.buy", PolicyVerdict::Allow);

        // Call 2 — allowed (count = 1, 1 <= 2).
        let (v, _, _) = engine.evaluate(&req("trade.sell"), &history);
        assert_eq!(v, PolicyVerdict::Allow);
        history.record("trade.sell", PolicyVerdict::Allow);

        // Call 3 — allowed (count = 2, 2 <= 2).
        let (v, _, _) = engine.evaluate(&req("trade.buy"), &history);
        assert_eq!(v, PolicyVerdict::Allow);
        history.record("trade.buy", PolicyVerdict::Allow);

        // Call 4 — blocked (count = 3, 3 > 2).
        let (v, rule_id, _) = engine.evaluate(&req("trade.buy"), &history);
        assert_eq!(v, PolicyVerdict::Block);
        assert_eq!(rule_id.as_deref(), Some("trade-limit"));
    }

    #[test]
    fn budget_boundary_is_exact() {
        // max = 1: allow calls 1 & 2, block call 3.
        let engine = budget_engine(1);
        let mut history = CallHistory::new();

        // Call 1 — allowed (count = 0, 0 <= 1).
        let (v, _, _) = engine.evaluate(&req("trade.buy"), &history);
        assert_eq!(v, PolicyVerdict::Allow);
        history.record("trade.buy", PolicyVerdict::Allow);

        // Call 2 — allowed (count = 1, 1 <= 1).
        let (v, _, _) = engine.evaluate(&req("trade.buy"), &history);
        assert_eq!(v, PolicyVerdict::Allow);
        history.record("trade.buy", PolicyVerdict::Allow);

        // Call 3 — blocked (count = 2, 2 > 1).
        let (v, _, _) = engine.evaluate(&req("trade.buy"), &history);
        assert_eq!(v, PolicyVerdict::Block);
    }

    // ── Phantom verdict ──────────────────────────────────────────────────────

    #[test]
    fn phantom_verdict_recorded() {
        let engine = PolicyEngine {
            document: PolicyDocument {
                schema_version: 1,
                rules: vec![PolicyRule {
                    id: "observe-only".to_string(),
                    tool_pattern: "research.**".to_string(),
                    history_tool_pattern: None,
                    history_max_calls: None,
                    verdict: PolicyVerdict::Phantom,
                    reason: "research tools are phantom-only".to_string(),
                }],
            },
            digest: String::new(),
        };
        let history = CallHistory::new();
        let (verdict, _, _) = engine.evaluate(&req("research.fetch"), &history);
        assert_eq!(verdict, PolicyVerdict::Phantom);
    }

    #[test]
    fn load_rejects_unknown_schema_version() {
        let temp = tempfile::tempdir().expect("tempdir");
        let policy_path = temp.path().join("policy.toml");
        std::fs::write(
            &policy_path,
            r#"
schema_version = 999
rules = []
"#,
        )
        .expect("write policy");

        let err = PolicyEngine::load(&policy_path).expect_err("must reject unsupported schema");
        assert!(err
            .to_string()
            .contains("unsupported policy schema_version"));
    }
}
