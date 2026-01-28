use proptest::prelude::*;
use rayon::ThreadPoolBuilder;

use cogitator::agent::AgentTraceEntry;
use cogitator::chaos::{
    ChaosEngine, ChaosProfile, FaultRates, CHAOS_PROFILE_SCHEMA_VERSION, CHAOS_SCHEDULE_VERSION,
};
use cogitator::drift;
use cogitator::eval;
use cogitator::model::{ChaosProfileSummary, WitnessedMetadata, TRACE_SCHEMA_VERSION};
use cogitator::tooling::{
    ToolCall, ToolMode, ToolRequest, ToolResponse, ToolTranscript, ToolTranscriptRecord,
    TOOL_TRANSCRIPT_SCHEMA_VERSION,
};
use cogitator::{trace, witness};

fn witness_root_for_trace(
    metadata: &WitnessedMetadata,
    events: &[cogitator::model::TraceEvent],
) -> String {
    let metadata_bytes = trace::encode_witnessed_metadata(metadata).expect("metadata bytes");
    let mut w = witness::Witness::new(&metadata_bytes).expect("witness");
    for event in events {
        let bytes = trace::encode_event(event).expect("event bytes");
        w.update(&bytes).expect("update witness");
    }
    w.finalize_hex()
}

fn witness_root_for_agent(
    metadata: &WitnessedMetadata,
    agent_trace: &[AgentTraceEntry],
    tool_calls: &[ToolCall],
) -> String {
    let metadata_bytes = trace::encode_witnessed_metadata(metadata).expect("metadata bytes");
    let mut w = witness::Witness::new(&metadata_bytes).expect("witness");
    for entry in agent_trace {
        let bytes = trace::encode_agent_trace_entry(entry).expect("agent trace bytes");
        w.update(&bytes).expect("update witness");
        for call in tool_calls.iter().filter(|call| call.step == entry.step) {
            let call_bytes = trace::encode_tool_call(call).expect("tool call bytes");
            w.update(&call_bytes).expect("update witness");
        }
    }
    w.finalize_hex()
}

proptest! {
    #[test]
    fn prop_witness_root_same_across_threads(seed in any::<u64>(), runs in 1u32..6) {
        let run_ids: Vec<u32> = (0..runs).collect();

        let roots: Vec<String> = [1usize, 2, 4]
            .iter()
            .map(|threads| {
                let output = ThreadPoolBuilder::new()
                    .num_threads(*threads)
                    .build()
                    .unwrap()
                    .install(|| eval::run_with_trace(seed, &run_ids, true));

                let metadata = WitnessedMetadata {
                    schema_version: TRACE_SCHEMA_VERSION,
                    seed,
                    requested_runs: runs,
                    executed_runs: runs,
                    parallel: true,
                    parallel_strategy: "rayon/ordered-run-ids".to_string(),
                    case_filter: None,
                    entropy_sources: vec!["rng:StdRng(seed)".to_string()],
                    total_rng_calls: output.total_rng_calls,
                    chaos_profile: None,
                };
                witness_root_for_trace(&metadata, &output.trace)
            })
            .collect();

        prop_assert!(roots.iter().all(|root| root == &roots[0]));
    }

    #[test]
    fn prop_record_replay_stable_with_faults(
        seed in any::<u64>(),
        timeout_rate in 0u32..5000,
        corrupt_rate in 0u32..2000,
        drop_rate in 0u32..2000,
        latency_rate in 0u32..5000,
    ) {
        let rates = FaultRates {
            timeout_per_million: timeout_rate,
            corrupt_per_million: corrupt_rate,
            drop_per_million: drop_rate,
            latency_sim_per_million: latency_rate,
        };
        let profile = ChaosProfile {
            schema_version: CHAOS_PROFILE_SCHEMA_VERSION,
            schedule_version: CHAOS_SCHEDULE_VERSION,
            enabled: true,
            profile: "custom".to_string(),
            seed,
            rates: rates.clone(),
        };

        let run_id = 0u32;
        let metadata = WitnessedMetadata {
            schema_version: TRACE_SCHEMA_VERSION,
            seed,
            requested_runs: 1,
            executed_runs: 1,
            parallel: false,
            parallel_strategy: "sequential".to_string(),
            case_filter: Some(0),
            entropy_sources: vec![
                "rng:StdRng(seed)".to_string(),
                "tooling:stubbed-or-replay".to_string(),
                "chaos:fault-schedule".to_string(),
            ],
            total_rng_calls: 0,
            chaos_profile: Some(ChaosProfileSummary {
                enabled: true,
                profile: "custom".to_string(),
                schedule_version: CHAOS_SCHEDULE_VERSION,
                rates,
            }),
        };

        let mut agent_trace = Vec::new();
        let tool_request = ToolRequest {
            tool_name: "clawdbot.lookup".to_string(),
            arguments: serde_json::json!({"case": "alpha", "seed": seed}),
        };
        agent_trace.push(AgentTraceEntry {
            step: 0,
            role: "assistant".to_string(),
            thought: "probe".to_string(),
            action: "lookup".to_string(),
            tool_requests: vec![tool_request.clone()],
            is_final: false,
        });
        agent_trace.push(AgentTraceEntry {
            step: 1,
            role: "assistant".to_string(),
            thought: "summarize".to_string(),
            action: "finalize".to_string(),
            tool_requests: vec![ToolRequest {
                tool_name: "clawdbot.inspect".to_string(),
                arguments: serde_json::json!({"case": "alpha", "pass": true}),
            }],
            is_final: true,
        });

        let mut live_transcript = ToolTranscript::new_live(Some(ChaosEngine::new(profile, run_id)));
        for entry in &agent_trace {
            for request in entry.tool_requests.clone() {
                live_transcript.execute(entry.step, request);
            }
        }
        let live_record = live_transcript.into_record();

        let mut replay_transcript = ToolTranscript::new_replay(live_record.clone());
        for entry in &agent_trace {
            for request in entry.tool_requests.clone() {
                replay_transcript.execute(entry.step, request);
            }
        }
        let replay_record = replay_transcript.into_record();

        let live_root = witness_root_for_agent(&metadata, &agent_trace, &live_record.entries);
        let replay_root = witness_root_for_agent(&metadata, &agent_trace, &replay_record.entries);
        prop_assert_eq!(live_root, replay_root);

        let drift_report = drift::detect_transcript_drift(&live_record, &replay_record);
        prop_assert!(!drift_report.drifted, "drifted: {:?}", drift_report.issues);
    }

    #[test]
    fn prop_request_mismatch_reports_issue(seed in any::<u64>()) {
        let request = ToolRequest {
            tool_name: "clawdbot.lookup".to_string(),
            arguments: serde_json::json!({"seed": seed}),
        };
        let response = ToolResponse {
            tool_name: "clawdbot.lookup".to_string(),
            output: serde_json::json!({"ok": true}),
            success: true,
            simulated_latency_ms: None,
        };
        let expected = ToolTranscriptRecord {
            schema_version: TOOL_TRANSCRIPT_SCHEMA_VERSION,
            mode: ToolMode::Live,
            entries: vec![ToolCall {
                step: 0,
                tool_call_idx: 0,
                request: request.clone(),
                response: response.clone(),
                fault: None,
            }],
        };

        let actual = ToolTranscriptRecord {
            schema_version: TOOL_TRANSCRIPT_SCHEMA_VERSION,
            mode: ToolMode::Live,
            entries: vec![ToolCall {
                step: 0,
                tool_call_idx: 0,
                request: ToolRequest {
                    tool_name: request.tool_name.clone(),
                    arguments: serde_json::json!({"seed": seed.wrapping_add(1)}),
                },
                response,
                fault: None,
            }],
        };

        let report = drift::detect_transcript_drift(&expected, &actual);
        prop_assert!(report
            .issues
            .iter()
            .any(|issue| issue.contains("tool request mismatch")));
    }
}
