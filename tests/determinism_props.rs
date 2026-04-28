use proptest::prelude::*;
use rayon::ThreadPoolBuilder;

use cogitator::agent::{Agent, AgentTraceEntry};
use cogitator::chaos::{
    ChaosEngine, ChaosProfile, FaultRates, CHAOS_PROFILE_SCHEMA_VERSION, CHAOS_SCHEDULE_VERSION,
};
use cogitator::drift;
use cogitator::eval;
use cogitator::llm;
use cogitator::model::{ChaosProfileSummary, WitnessedMetadata, TRACE_SCHEMA_VERSION};
use cogitator::report::DriftIssue;
use cogitator::tooling::{
    ToolCall, ToolMode, ToolOutcome, ToolRequest, ToolResponse, ToolTranscript,
    ToolTranscriptRecord, TOOL_TRANSCRIPT_SCHEMA_VERSION,
};
use cogitator::trace;
use cogitator::witness;

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
    trace::compute_agent_witness_root(metadata, agent_trace, tool_calls, &[])
        .expect("agent witness root")
}

fn llm_agent_trace(seed: u64) -> Vec<AgentTraceEntry> {
    let llm_request = llm::LlmRequest {
        schema_version: llm::LLM_REQUEST_SCHEMA_VERSION,
        model: "stub".to_string(),
        messages: vec![llm::LlmMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
        }],
        temperature: None,
        max_tokens: None,
        seed: Some(seed),
    };
    let tool_request = llm::make_tool_request(&llm_request).expect("llm tool request");
    vec![AgentTraceEntry {
        step: 0,
        role: "assistant".to_string(),
        thought: "query".to_string(),
        action: "llm".to_string(),
        tool_requests: vec![tool_request],
        is_final: true,
    }]
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
                    entropy_sources: vec!["rng:StdRng(seed)".to_string()],
                    total_rng_calls: output.total_rng_calls,
                    ..Default::default()
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
            ..Default::default()
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
        let outcome = ToolOutcome::Ok {
            output: response.output.clone(),
            simulated_latency_ms: response.simulated_latency_ms,
        };
        let expected = ToolTranscriptRecord {
            schema_version: TOOL_TRANSCRIPT_SCHEMA_VERSION,
            mode: ToolMode::Live,
            entries: vec![ToolCall {
                step: 0,
                tool_call_idx: 0,
                tool_name: request.tool_name.clone(),
                request: request.arguments.clone(),
                outcome: outcome.clone(),
                fault: None,
            }],
            ..Default::default()
        };

        let actual = ToolTranscriptRecord {
            schema_version: TOOL_TRANSCRIPT_SCHEMA_VERSION,
            mode: ToolMode::Live,
            entries: vec![ToolCall {
                step: 0,
                tool_call_idx: 0,
                tool_name: request.tool_name.clone(),
                request: serde_json::json!({"seed": seed.wrapping_add(1)}),
                outcome,
                fault: None,
            }],
            ..Default::default()
        };

        let report = drift::detect_transcript_drift(&expected, &actual);
        let mismatch = report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::ToolRequestMismatch { .. }));
        prop_assert!(mismatch);
    }
}

#[test]
fn witness_root_sorts_tool_calls_by_index() {
    let metadata = WitnessedMetadata {
        seed: 7,
        requested_runs: 1,
        executed_runs: 1,
        parallel: false,
        parallel_strategy: "sequential".to_string(),
        ..Default::default()
    };

    let agent_trace = vec![AgentTraceEntry {
        step: 0,
        role: "assistant".to_string(),
        thought: "probe".to_string(),
        action: "lookup".to_string(),
        tool_requests: vec![],
        is_final: false,
    }];

    let response = ToolResponse {
        tool_name: "clawdbot.lookup".to_string(),
        output: serde_json::json!({"ok": true}),
        success: true,
        simulated_latency_ms: None,
    };
    let outcome = ToolOutcome::Ok {
        output: response.output.clone(),
        simulated_latency_ms: response.simulated_latency_ms,
    };

    let call_a = ToolCall {
        step: 0,
        tool_call_idx: 1,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"order": "second"}),
        outcome: outcome.clone(),
        fault: None,
    };

    let call_b = ToolCall {
        step: 0,
        tool_call_idx: 0,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"order": "first"}),
        outcome,
        fault: None,
    };

    let root_unsorted =
        witness_root_for_agent(&metadata, &agent_trace, &[call_a.clone(), call_b.clone()]);
    let root_sorted = witness_root_for_agent(&metadata, &agent_trace, &[call_b, call_a]);

    assert_eq!(root_unsorted, root_sorted);
}

#[test]
fn llm_stub_record_replay_deterministic() {
    let seed = 7u64;
    let metadata = WitnessedMetadata {
        seed,
        requested_runs: 1,
        executed_runs: 1,
        parallel: false,
        parallel_strategy: "sequential".to_string(),
        case_filter: Some(0),
        entropy_sources: vec![
            "rng:StdRng(seed)".to_string(),
            "tooling:stubbed-or-replay".to_string(),
        ],
        ..Default::default()
    };

    let agent_trace = llm_agent_trace(seed);

    let mut live_transcript = ToolTranscript::new_live(None);
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
    assert_eq!(live_root, replay_root);

    let drift_report = drift::detect_transcript_drift(&live_record, &replay_record);
    assert!(!drift_report.drifted, "drifted: {:?}", drift_report.issues);
}

#[test]
fn llm_stub_thread_invariance() {
    let seed = 9u64;
    let metadata = WitnessedMetadata {
        seed,
        requested_runs: 1,
        executed_runs: 1,
        parallel: false,
        parallel_strategy: "sequential".to_string(),
        case_filter: Some(0),
        entropy_sources: vec![
            "rng:StdRng(seed)".to_string(),
            "tooling:stubbed-or-replay".to_string(),
        ],
        ..Default::default()
    };

    let roots: Vec<String> = [1usize, 4]
        .iter()
        .map(|threads| {
            ThreadPoolBuilder::new()
                .num_threads(*threads)
                .build()
                .unwrap()
                .install(|| {
                    let agent_trace = llm_agent_trace(seed);
                    let mut transcript = ToolTranscript::new_live(None);
                    for entry in &agent_trace {
                        for request in entry.tool_requests.clone() {
                            transcript.execute(entry.step, request);
                        }
                    }
                    let record = transcript.into_record();
                    witness_root_for_agent(&metadata, &agent_trace, &record.entries)
                })
        })
        .collect();

    assert!(roots.iter().all(|root| root == &roots[0]));
}

#[test]
fn stub_hash_is_canonical_for_request_arguments() {
    let mut map_a = serde_json::Map::new();
    map_a.insert("a".to_string(), serde_json::json!(1));
    map_a.insert("b".to_string(), serde_json::json!(2));
    let mut map_b = serde_json::Map::new();
    map_b.insert("b".to_string(), serde_json::json!(2));
    map_b.insert("a".to_string(), serde_json::json!(1));

    let request_a = ToolRequest {
        tool_name: "clawdbot.lookup".to_string(),
        arguments: serde_json::Value::Object(map_a),
    };
    let request_b = ToolRequest {
        tool_name: "clawdbot.lookup".to_string(),
        arguments: serde_json::Value::Object(map_b),
    };

    let mut transcript = ToolTranscript::new_live(None);
    let response_a = transcript.execute(0, request_a);
    let response_b = transcript.execute(0, request_b);

    let hash_a = response_a
        .output
        .get("hash")
        .and_then(|value| value.as_str())
        .expect("hash a");
    let hash_b = response_b
        .output
        .get("hash")
        .and_then(|value| value.as_str())
        .expect("hash b");

    assert_eq!(hash_a, hash_b);
}

#[test]
fn agent_witness_root_invariant_across_thread_counts() {
    let roots: Vec<String> = [1usize, 2, 4, 8]
        .iter()
        .map(|threads| {
            ThreadPoolBuilder::new()
                .num_threads(*threads)
                .build()
                .unwrap()
                .install(|| {
                    let metadata = WitnessedMetadata {
                        seed: 13,
                        requested_runs: 1,
                        executed_runs: 1,
                        parallel: false,
                        parallel_strategy: "sequential".to_string(),
                        entropy_sources: vec![
                            "rng:StdRng(seed)".to_string(),
                            "tooling:stubbed-or-replay".to_string(),
                        ],
                        ..Default::default()
                    };

                    let mut agent = cogitator::agent::ClawdbotAgent::new(
                        cogitator::agent::LlmConfig::default(),
                    );
                    let mut agent_trace = Vec::new();
                    let mut transcript = ToolTranscript::new_live(None);
                    let mut prior_outputs = Vec::new();

                    for step in 0..2u32 {
                        let input = cogitator::agent::AgentInput {
                            run_id: 0,
                            case_id: "case".to_string(),
                            step,
                            seed: 13,
                            prior_tool_outputs: prior_outputs.clone(),
                        };
                        let output = agent.step(input);
                        agent_trace.push(cogitator::agent::trace_entry_from_output(step, &output));
                        for request in output.tool_requests {
                            let response = transcript.execute(step, request);
                            prior_outputs.push(response);
                        }
                        if output.is_final {
                            break;
                        }
                    }

                    let record = transcript.into_record();
                    trace::compute_agent_witness_root(
                        &metadata,
                        &agent_trace,
                        &record.entries,
                        &record.phantom_entries,
                    )
                    .expect("root")
                })
        })
        .collect();

    assert!(roots.iter().all(|root| root == &roots[0]));
}

#[test]
fn witness_root_changes_on_semantic_output_change() {
    let metadata = WitnessedMetadata {
        seed: 3,
        requested_runs: 1,
        executed_runs: 1,
        parallel: false,
        parallel_strategy: "sequential".to_string(),
        case_filter: Some(0),
        ..Default::default()
    };
    let agent_trace = vec![AgentTraceEntry {
        step: 0,
        role: "assistant".to_string(),
        thought: "x".to_string(),
        action: "y".to_string(),
        tool_requests: vec![],
        is_final: true,
    }];

    let call_a = ToolCall {
        step: 0,
        tool_call_idx: 0,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"k": "v"}),
        outcome: ToolOutcome::Ok {
            output: serde_json::json!({"result": 1}),
            simulated_latency_ms: None,
        },
        fault: None,
    };

    let call_b = ToolCall {
        outcome: ToolOutcome::Ok {
            output: serde_json::json!({"result": 2}),
            simulated_latency_ms: None,
        },
        ..call_a.clone()
    };

    let root_a = witness_root_for_agent(&metadata, &agent_trace, &[call_a]);
    let root_b = witness_root_for_agent(&metadata, &agent_trace, &[call_b]);
    assert_ne!(root_a, root_b);
}

#[test]
fn witness_root_ignores_simulated_latency() {
    let metadata = WitnessedMetadata {
        seed: 1,
        requested_runs: 1,
        executed_runs: 1,
        parallel: false,
        parallel_strategy: "sequential".to_string(),
        case_filter: Some(0),
        ..Default::default()
    };

    let agent_trace = vec![AgentTraceEntry {
        step: 0,
        role: "assistant".to_string(),
        thought: "x".to_string(),
        action: "y".to_string(),
        tool_requests: vec![],
        is_final: true,
    }];

    let call_with_latency = ToolCall {
        step: 0,
        tool_call_idx: 0,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"k": "v"}),
        outcome: ToolOutcome::Ok {
            output: serde_json::json!({"result": 1}),
            simulated_latency_ms: Some(123),
        },
        fault: None,
    };

    let call_without_latency = ToolCall {
        outcome: ToolOutcome::Ok {
            output: serde_json::json!({"result": 1}),
            simulated_latency_ms: None,
        },
        ..call_with_latency.clone()
    };

    let root_a = witness_root_for_agent(&metadata, &agent_trace, &[call_with_latency]);
    let root_b = witness_root_for_agent(&metadata, &agent_trace, &[call_without_latency]);
    assert_eq!(root_a, root_b);
}

#[test]
fn witness_root_ignores_fault_metadata_shape_details() {
    let metadata = WitnessedMetadata {
        seed: 2,
        requested_runs: 1,
        executed_runs: 1,
        parallel: false,
        parallel_strategy: "sequential".to_string(),
        case_filter: Some(0),
        ..Default::default()
    };
    let agent_trace = vec![AgentTraceEntry {
        step: 0,
        role: "assistant".to_string(),
        thought: "x".to_string(),
        action: "y".to_string(),
        tool_requests: vec![],
        is_final: true,
    }];

    let base = ToolCall {
        step: 0,
        tool_call_idx: 0,
        tool_name: "clawdbot.lookup".to_string(),
        request: serde_json::json!({"k": "v"}),
        outcome: ToolOutcome::Err {
            error: cogitator::tooling::ToolError {
                error_kind: cogitator::tooling::ToolErrorKind::Timeout,
                message: Some("timeout".to_string()),
            },
            simulated_latency_ms: Some(90),
        },
        fault: Some(cogitator::tooling::TranscriptFault::Timeout {
            domain: "tooling".to_string(),
            timeout_ms: Some(200),
        }),
    };

    let changed_fault_details = ToolCall {
        fault: Some(cogitator::tooling::TranscriptFault::Timeout {
            domain: "tooling".to_string(),
            timeout_ms: None,
        }),
        ..base.clone()
    };

    let no_fault = ToolCall {
        fault: None,
        ..base.clone()
    };

    let root_a = witness_root_for_agent(&metadata, &agent_trace, &[base]);
    let root_b = witness_root_for_agent(&metadata, &agent_trace, &[changed_fault_details]);
    let root_c = witness_root_for_agent(&metadata, &agent_trace, &[no_fault]);

    assert_eq!(root_a, root_b);
    assert_ne!(root_a, root_c);
}
