use serde::{Deserialize, Serialize};

pub const TRACE_SCHEMA_VERSION: u32 = 2;

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
