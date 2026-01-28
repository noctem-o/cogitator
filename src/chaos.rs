use anyhow::Result;
use blake3::Hasher;
use serde::{Deserialize, Serialize};

use crate::tooling::{ToolRequest, ToolResponse};

pub const CHAOS_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const CHAOS_SCHEDULE_VERSION: u32 = 1;

const PER_MILLION: u64 = 1_000_000;
const LATENCY_MIN_MS: u64 = 5;
const LATENCY_MAX_MS: u64 = 250;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultRates {
    pub timeout_per_million: u32,
    pub corrupt_per_million: u32,
    pub drop_per_million: u32,
    pub latency_sim_per_million: u32,
}

impl FaultRates {
    pub fn none() -> Self {
        Self {
            timeout_per_million: 0,
            corrupt_per_million: 0,
            drop_per_million: 0,
            latency_sim_per_million: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChaosProfile {
    pub schema_version: u32,
    pub schedule_version: u32,
    pub enabled: bool,
    pub profile: String,
    pub seed: u64,
    pub rates: FaultRates,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    Timeout,
    Corrupt,
    Drop,
    LatencySim,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FaultParams {
    pub mask: Option<u64>,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FaultRecord {
    pub kind: FaultKind,
    pub step: u32,
    pub tool_call_idx: u32,
    pub domain: String,
    pub params: FaultParams,
}

#[derive(Debug, Clone)]
pub struct ChaosEngine {
    profile: ChaosProfile,
    run_id: u32,
}

impl ChaosEngine {
    pub fn new(profile: ChaosProfile, run_id: u32) -> Self {
        Self { profile, run_id }
    }

    pub fn profile(&self) -> &ChaosProfile {
        &self.profile
    }

    pub fn decide_fault(
        &self,
        step: u32,
        tool_call_idx: u32,
        domain: &str,
    ) -> Option<FaultRecord> {
        if !self.profile.enabled {
            return None;
        }
        let candidates = [
            (FaultKind::Timeout, self.profile.rates.timeout_per_million),
            (FaultKind::Drop, self.profile.rates.drop_per_million),
            (FaultKind::Corrupt, self.profile.rates.corrupt_per_million),
            (FaultKind::LatencySim, self.profile.rates.latency_sim_per_million),
        ];

        for (kind, rate) in candidates {
            if rate == 0 {
                continue;
            }
            let decision = hash_u64(
                self.profile.seed,
                self.run_id,
                step,
                tool_call_idx,
                domain,
                &format!("{:?}", kind),
            );
            if decision % PER_MILLION < rate as u64 {
                let params = match kind {
                    FaultKind::Corrupt => FaultParams {
                        mask: Some(hash_u64(
                            self.profile.seed,
                            self.run_id,
                            step,
                            tool_call_idx,
                            domain,
                            "corrupt_mask",
                        )),
                        latency_ms: None,
                    },
                    FaultKind::LatencySim => FaultParams {
                        mask: None,
                        latency_ms: Some(latency_value(
                            self.profile.seed,
                            self.run_id,
                            step,
                            tool_call_idx,
                            domain,
                        )),
                    },
                    _ => FaultParams {
                        mask: None,
                        latency_ms: None,
                    },
                };
                return Some(FaultRecord {
                    kind,
                    step,
                    tool_call_idx,
                    domain: domain.to_string(),
                    params,
                });
            }
        }

        None
    }
}

pub fn apply_fault(
    request: &ToolRequest,
    mut response: ToolResponse,
    fault: &FaultRecord,
) -> Result<ToolResponse> {
    match fault.kind {
        FaultKind::Timeout => {
            response.success = false;
            response.output = serde_json::json!({
                "error": "timeout",
                "tool": request.tool_name,
            });
        }
        FaultKind::Drop => {
            response.success = false;
            response.output = serde_json::Value::Null;
        }
        FaultKind::Corrupt => {
            let mask = fault.params.mask.unwrap_or_default();
            response.output = corrupt_value(response.output, mask);
        }
        FaultKind::LatencySim => {
            response.simulated_latency_ms = fault.params.latency_ms;
        }
    }
    Ok(response)
}

pub fn profile_from_name(name: &str, seed: u64, enabled: bool) -> ChaosProfile {
    let rates = match name {
        "ci" => FaultRates {
            timeout_per_million: 500,
            corrupt_per_million: 100,
            drop_per_million: 100,
            latency_sim_per_million: 5_000,
        },
        "stress" => FaultRates {
            timeout_per_million: 10_000,
            corrupt_per_million: 2_000,
            drop_per_million: 2_000,
            latency_sim_per_million: 50_000,
        },
        _ => FaultRates::none(),
    };

    ChaosProfile {
        schema_version: CHAOS_PROFILE_SCHEMA_VERSION,
        schedule_version: CHAOS_SCHEDULE_VERSION,
        enabled,
        profile: name.to_string(),
        seed,
        rates,
    }
}

pub fn with_overrides(
    mut profile: ChaosProfile,
    timeout_rate: Option<u32>,
    corrupt_rate: Option<u32>,
    drop_rate: Option<u32>,
    latency_rate: Option<u32>,
) -> ChaosProfile {
    if let Some(rate) = timeout_rate {
        profile.rates.timeout_per_million = rate;
    }
    if let Some(rate) = corrupt_rate {
        profile.rates.corrupt_per_million = rate;
    }
    if let Some(rate) = drop_rate {
        profile.rates.drop_per_million = rate;
    }
    if let Some(rate) = latency_rate {
        profile.rates.latency_sim_per_million = rate;
    }
    profile
}

pub fn rate_to_per_million(rate: f64) -> u32 {
    if !rate.is_finite() {
        return 0;
    }
    let scaled = (rate.clamp(0.0, 1.0) * PER_MILLION as f64).round() as u64;
    scaled.min(PER_MILLION) as u32
}

fn hash_u64(
    seed: u64,
    run_id: u32,
    step: u32,
    tool_call_idx: u32,
    domain: &str,
    kind: &str,
) -> u64 {
    let mut hasher = Hasher::new();
    hasher.update(&seed.to_le_bytes());
    hasher.update(&run_id.to_le_bytes());
    hasher.update(&step.to_le_bytes());
    hasher.update(&tool_call_idx.to_le_bytes());
    hasher.update(domain.as_bytes());
    hasher.update(kind.as_bytes());
    let bytes = hasher.finalize();
    u64::from_le_bytes(bytes.as_bytes()[0..8].try_into().unwrap())
}

fn latency_value(seed: u64, run_id: u32, step: u32, tool_call_idx: u32, domain: &str) -> u64 {
    let base = hash_u64(
        seed,
        run_id,
        step,
        tool_call_idx,
        domain,
        "latency",
    );
    let span = LATENCY_MAX_MS - LATENCY_MIN_MS + 1;
    LATENCY_MIN_MS + (base % span)
}

fn corrupt_value(value: serde_json::Value, mask: u64) -> serde_json::Value {
    match value {
        serde_json::Value::String(mut s) => {
            if !s.is_empty() {
                let idx = (mask as usize) % s.len();
                let mut bytes = s.into_bytes();
                bytes[idx] ^= 0x1;
                s = String::from_utf8_lossy(&bytes).to_string();
            }
            serde_json::Value::String(s)
        }
        serde_json::Value::Number(num) => {
            if let Some(n) = num.as_i64() {
                serde_json::Value::Number((n ^ (mask as i64 & 0xFF)).into())
            } else if let Some(n) = num.as_u64() {
                serde_json::Value::Number((n ^ (mask as u64 & 0xFF)).into())
            } else if let Some(n) = num.as_f64() {
                let delta = (mask as i64 % 7) as f64;
                serde_json::Value::Number(
                    serde_json::Number::from_f64(n + delta).unwrap_or(num),
                )
            } else {
                serde_json::Value::Number(num)
            }
        }
        serde_json::Value::Bool(b) => serde_json::Value::Bool(!b),
        serde_json::Value::Array(mut values) => {
            if !values.is_empty() {
                let idx = (mask as usize) % values.len();
                values[idx] = corrupt_value(values[idx].clone(), mask.rotate_left(13));
            }
            serde_json::Value::Array(values)
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            if keys.is_empty() {
                return serde_json::Value::Object(map);
            }
            if let Some(key) = keys.get((mask as usize) % keys.len()).cloned() {
                if let Some(value) = map.get(&key).cloned() {
                    let mut output = map;
                    output.insert(key, corrupt_value(value, mask.rotate_left(7)));
                    return serde_json::Value::Object(output);
                }
            }
            serde_json::Value::Object(map)
        }
        other => other,
    }
}
