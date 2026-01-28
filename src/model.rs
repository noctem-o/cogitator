use serde::{Deserialize, Serialize};

use crate::chaos::FaultRates;

pub const TRACE_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThoughtEvent {
    pub step: u32,
    pub role: String,
    pub content: String,
    pub entropy_bits: u32,
    pub rng_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseResult {
    pub run_id: u32,
    pub case_id: String,
    pub difficulty: f32,
    pub score: f32,
    pub passed: bool,
    pub rng_calls: u32,
    pub thoughts: Vec<ThoughtEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub pass_rate: f32,
    pub avg_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifest {
    pub meta_json: String,
    pub trace_jsonl: String,
    pub results_csv: String,
    pub results_json: String,
    pub summary_json: String,
    pub witness_root_txt: String,
    pub analysis_json: String,
    pub agent_trace_json: Option<String>,
    pub tool_transcript_json: Option<String>,
    pub witness_manifest_json: Option<String>,
    pub hash_chain_txt: Option<String>,
    pub drift_report_json: Option<String>,
    pub chaos_profile_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisBundle {
    pub metadata: RunMetadata,
    pub summary: Summary,
    pub results: Vec<CaseResult>,
    pub witness_root: String,
    pub artifacts: ArtifactManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunMetadata {
    pub witnessed: WitnessedMetadata,
    pub provenance: ProvenanceMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WitnessedMetadata {
    pub schema_version: u32,
    pub seed: u64,
    pub requested_runs: u32,
    pub executed_runs: u32,
    pub parallel: bool,
    pub parallel_strategy: String,
    pub case_filter: Option<u32>,
    pub entropy_sources: Vec<String>,
    pub total_rng_calls: u64,
    pub chaos_profile: Option<ChaosProfileSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceMetadata {
    pub created_at: String,
    pub git_rev: Option<String>,
    pub rustc_version: Option<String>,
    pub cargo_version: Option<String>,
    pub nix_store_path: Option<String>,
    pub variability_factors: Vec<String>,
}

pub const WITNESS_MANIFEST_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChaosProfileSummary {
    pub enabled: bool,
    pub profile: String,
    pub schedule_version: u32,
    pub rates: FaultRates,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WitnessManifest {
    pub schema_version: u32,
    pub run_id: u32,
    pub agent: String,
    pub mode: String,
    pub meta_json: String,
    pub agent_trace_json: String,
    pub tool_transcript_json: String,
    pub drift_report_json: String,
    pub hash_chain_txt: String,
    pub chaos_profile_json: Option<String>,
    pub witness_root_txt: Option<String>,
    pub artifact_hashes: std::collections::BTreeMap<String, String>,
    pub bundle_hash: String,
    pub replay_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceEvent {
    pub schema_version: u32,
    pub run_id: u32,
    pub case_id: String,
    pub step: u32,
    pub role: String,
    pub content: String,
    pub entropy_bits: u32,
    pub rng_calls: u32,
}
