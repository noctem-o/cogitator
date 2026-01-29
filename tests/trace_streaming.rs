use cogitator::eval;
use cogitator::model::{TraceEvent, WitnessedMetadata, TRACE_SCHEMA_VERSION};
use cogitator::trace;
use cogitator::witness;
use tempfile::tempdir;

#[test]
fn trace_streaming_matches_manual_witness_root() {
    let seed = 42u64;
    let runs = 3u32;
    let run_ids: Vec<u32> = (0..runs).collect();
    let output = eval::run_with_trace(seed, &run_ids, true);

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

    let expected = manual_witness_root(&metadata, &output.trace);
    let temp = tempdir().expect("tempdir");
    let trace_path = temp.path().join("trace.jsonl");
    let actual = trace::write_trace_and_compute_witness_root(&trace_path, &metadata, &output.trace)
        .expect("trace witness root");

    assert_eq!(expected, actual);
}

fn manual_witness_root(metadata: &WitnessedMetadata, events: &[TraceEvent]) -> String {
    let mut ordered: Vec<&TraceEvent> = events.iter().collect();
    ordered.sort_by_key(|event| (event.run_id, event.step));

    let metadata_bytes = trace::encode_witnessed_metadata(metadata).expect("metadata bytes");
    let mut w = witness::Witness::new(&metadata_bytes).expect("witness");
    for event in ordered {
        let bytes = trace::encode_event(event).expect("event bytes");
        w.update(&bytes).expect("update witness");
    }
    w.finalize_hex()
}
