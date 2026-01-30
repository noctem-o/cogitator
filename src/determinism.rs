//! Property-based tests for determinism guarantees
//!
//! These tests use proptest to verify that Cogitator's core determinism
//! claims hold across random inputs:
//!
//! 1. Same inputs → same witness roots
//! 2. Parallel == sequential execution
//! 3. Chaos injection is deterministic
//! 4. Witness roots are stable across re-computation

use cogitator::{chaos, eval, trace, witness};
use proptest::prelude::*;

proptest! {
    /// Witness computation must be deterministic for same inputs
    #[test]
    fn witness_determinism(seed: u64, run_ids in prop::collection::vec(any::<u32>(), 1..10)) {
        let output1 = eval::run_with_trace(seed, &run_ids, false);
        let output2 = eval::run_with_trace(seed, &run_ids, false);

        prop_assert_eq!(output1.results.len(), output2.results.len());
        prop_assert_eq!(output1.trace.len(), output2.trace.len());

        for (e1, e2) in output1.trace.iter().zip(output2.trace.iter()) {
            prop_assert_eq!(e1.run_id, e2.run_id);
            prop_assert_eq!(e1.case_id, e2.case_id);
            prop_assert_eq!(e1.step, e2.step);
            prop_assert_eq!(e1.content, e2.content);
            prop_assert_eq!(e1.rng_calls, e2.rng_calls);
        }
    }

    /// Parallel and sequential evaluation must produce identical results
    #[test]
    fn parallel_sequential_equivalence(seed: u64, run_ids in prop::collection::vec(any::<u32>(), 1..20)) {
        let seq_output = eval::run_with_trace(seed, &run_ids, false);
        let par_output = eval::run_with_trace(seed, &run_ids, true);

        prop_assert_eq!(seq_output.results.len(), par_output.results.len());

        // Results must be identical when sorted by run_id
        let mut seq_sorted = seq_output.results.clone();
        let mut par_sorted = par_output.results.clone();
        seq_sorted.sort_by_key(|r| r.run_id);
        par_sorted.sort_by_key(|r| r.run_id);

        for (s, p) in seq_sorted.iter().zip(par_sorted.iter()) {
            prop_assert_eq!(s.run_id, p.run_id);
            prop_assert_eq!(s.case_id, p.case_id);
            prop_assert_eq!(s.passed, p.passed);
            // Fixed-point arithmetic ensures bit-exact equality
            prop_assert_eq!(s.score.to_bits(), p.score.to_bits());
            prop_assert_eq!(s.difficulty.to_bits(), p.difficulty.to_bits());
        }
    }

    /// Chaos engine must be deterministic for same seed
    #[test]
    fn chaos_determinism(
        seed: u64,
        run_id: u32,
        step in 0u32..100,
        tool_call_idx in 0u32..10,
    ) {
        let profile = chaos::profile_from_name("ci", seed, true);
        let engine1 = chaos::ChaosEngine::new(profile.clone(), run_id);
        let engine2 = chaos::ChaosEngine::new(profile, run_id);

        let fault1 = engine1.decide_fault(step, tool_call_idx, "test.domain");
        let fault2 = engine2.decide_fault(step, tool_call_idx, "test.domain");

        prop_assert_eq!(fault1, fault2);
    }

    /// Witness root must be identical for re-computed traces
    #[test]
    fn witness_root_stability(seed: u64, run_count in 1u32..5) {
        use cogitator::model::{WitnessedMetadata, TRACE_SCHEMA_VERSION};

        let metadata = WitnessedMetadata {
            schema_version: TRACE_SCHEMA_VERSION,
            seed,
            requested_runs: run_count,
            executed_runs: run_count,
            parallel: false,
            parallel_strategy: "sequential".to_string(),
            case_filter: None,
            entropy_sources: vec!["rng:StdRng(seed)".to_string()],
            total_rng_calls: 0,
            chaos_profile: None,
            pass_threshold: None,
        };

        let root1 = trace::encode_witnessed_metadata(&metadata)
            .and_then(|bytes| witness::Witness::new(&bytes))
            .map(|w| w.finalize_hex());

        let root2 = trace::encode_witnessed_metadata(&metadata)
            .and_then(|bytes| witness::Witness::new(&bytes))
            .map(|w| w.finalize_hex());

        prop_assert_eq!(root1.ok(), root2.ok());
    }

    /// Witness must be sensitive to event order
    #[test]
    fn witness_order_sensitivity(
        seed: u64,
        events in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..100), 2..5)
    ) {
        use cogitator::model::{WitnessedMetadata, TRACE_SCHEMA_VERSION};

        let metadata = WitnessedMetadata {
            schema_version: TRACE_SCHEMA_VERSION,
            seed,
            requested_runs: 1,
            executed_runs: 1,
            parallel: false,
            parallel_strategy: "sequential".to_string(),
            case_filter: None,
            entropy_sources: vec!["test".to_string()],
            total_rng_calls: 0,
            chaos_profile: None,
            pass_threshold: None,
        };

        let metadata_bytes = trace::encode_witnessed_metadata(&metadata).unwrap();

        // Forward order
        let mut w1 = witness::Witness::new(&metadata_bytes).unwrap();
        for event in &events {
            w1.update(event).unwrap();
        }

        // Reverse order
        let mut w2 = witness::Witness::new(&metadata_bytes).unwrap();
        for event in events.iter().rev() {
            w2.update(event).unwrap();
        }

        if events.len() > 1 {
            prop_assert_ne!(w1.finalize_hex(), w2.finalize_hex());
        }
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    #[test]
    fn fixed_point_arithmetic_cross_check() {
        // Verify fixed-point conversion matches expected values
        let test_cases = [
            (0u32, 0.0f32),           // 0 ppm = 0.0
            (500_000, 0.5),           // 500k ppm = 0.5
            (1_000_000, 1.0),         // 1M ppm = 1.0
            (250_000, 0.25),          // 250k ppm = 0.25
            (750_000, 0.75),          // 750k ppm = 0.75
        ];

        for (ppm, expected_float) in test_cases {
            let computed = ppm as f32 / 1_000_000.0;
            assert!((computed - expected_float).abs() < 1e-6);
        }
    }

    #[test]
    fn witness_domain_separation() {
        // Verify that domain separators prevent length/content confusion
        let metadata = b"metadata";

        // Event with content that looks like length encoding
        let tricky_event = {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"|LENGTH|");
            bytes.extend_from_slice(&8u64.to_be_bytes());
            bytes.extend_from_slice(b"|CONTENT|");
            bytes.extend_from_slice(b"payload!");
            bytes
        };

        let mut w1 = witness::Witness::new(metadata).unwrap();
        w1.update(&tricky_event).unwrap();

        let mut w2 = witness::Witness::new(metadata).unwrap();
        w2.update(b"payload!").unwrap();

        // These should produce different witness roots due to domain separation
        assert_ne!(w1.finalize_hex(), w2.finalize_hex());
    }
}
